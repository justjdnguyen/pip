// Spike: native video under a transparent web page (macOS).
//
// Checks the riskiest part of the native-video plan, using the real sink
// (video/macos.rs):
//   * an AVSampleBufferDisplayLayer in a layer-hosting NSView placed under the
//     WKWebView, autoresizing to fill the window;
//   * the web view transparent, so the page's empty areas show the video and
//     its controls draw on top;
//   * fed real H.264 the way the extension sends it (Annex B access units,
//     SPS/PPS on keyframes), converted to AVCC by the sink.
//
// The test clip is made on the fly: a moving test pattern drawn into pixel
// buffers and encoded with VideoToolbox, converted to Annex B, then handed to
// the sink at 60 fps. One frame in 30 is sent decode-only (DoNotDisplay).
//
// Run: cargo run --bin avsb_spike
//   BP_SPIKE_SECONDS=n   quit after n seconds
//   BP_SPIKE_SWITCH=n    alternate 16:9 and 4:3 streams every n seconds (tests configure + pillarboxing)

#![cfg_attr(not(target_os = "macos"), allow(unused))]

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("avsb_spike is macOS-only");
}

#[cfg(target_os = "macos")]
#[path = "../video/macos.rs"]
#[allow(dead_code)]
mod sink;

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(target_os = "macos")]
mod mac {
    use super::sink::{create_sink, Frame, NeedKeyframe, StreamConfig, VideoSink};
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_core_foundation::{kCFBooleanFalse, kCFBooleanTrue, CFDictionary, CFNumber, CFRetained, CFString, CFType};
    use objc2_core_media::{CMSampleBuffer, CMTime};
    use objc2_core_video::CVPixelBuffer;
    use objc2_foundation::NSSize;
    use objc2_video_toolbox::*;
    use std::ffi::c_void;
    use std::ptr::{self, NonNull};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    pub fn run() {
        tauri::Builder::default()
            .setup(|app| {
                let window = WebviewWindowBuilder::new(app, "pip-spike", WebviewUrl::App("spike.html".into()))
                    .title("AVSB spike")
                    .inner_size(640.0, 360.0)
                    .decorations(false)
                    .always_on_top(true)
                    .resizable(true)
                    .shadow(true)
                    // As in main.rs. On macOS a background colour already makes
                    // wry turn off the WKWebView's own background (drawsBackground).
                    .background_color(tauri::window::Color(0, 0, 0, 255))
                    .build()?;
                let ns_window = window.ns_window()? as *const AnyObject;
                // SAFETY: live NSWindow, main thread (setup runs there).
                unsafe {
                    let w = &*ns_window;
                    let _: () = msg_send![w, setContentAspectRatio: NSSize::new(16.0, 9.0)];
                    let n: isize = msg_send![w, windowNumber];
                    println!("window number {n}");
                }
                let sink = create_sink(&window).map_err(|e| format!("sink setup failed: {e:#}"))?;
                std::thread::spawn(move || feed(sink));
                if std::env::var_os("BP_SPIKE_DUMP").is_some() {
                    let w2 = window.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(3));
                        let w3 = w2.clone();
                        let _ = w2.run_on_main_thread(move || unsafe { dump_views(w3.ns_window().unwrap() as *const AnyObject) });
                    });
                }
                if let Some(secs) = env_u64("BP_SPIKE_SECONDS") {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(secs));
                        handle.exit(0);
                    });
                }
                Ok(())
            })
            .run(tauri::generate_context!())
            .expect("spike failed");
    }

    /// Print the window's view tree (debugging the layering).
    unsafe fn dump_views(ns_window: *const AnyObject) {
        unsafe fn walk(v: &AnyObject, depth: usize) {
            let desc: objc2::rc::Retained<objc2_foundation::NSString> = msg_send![v, description];
            let layer: *const AnyObject = msg_send![v, layer];
            let ld = if layer.is_null() { "no layer".to_string() } else {
                let d: objc2::rc::Retained<objc2_foundation::NSString> = msg_send![&*layer, description];
                d.to_string()
            };
            println!("{}{} [{}]", "  ".repeat(depth), desc, ld);
            let subs: objc2::rc::Retained<objc2_foundation::NSArray<AnyObject>> = msg_send![v, subviews];
            for s in subs.iter() { walk(&s, depth + 1); }
        }
        let frame_view: *const AnyObject = msg_send![&*ns_window, contentView];
        let sup: *const AnyObject = msg_send![&*frame_view, superview];
        walk(&*sup, 0);
    }

    fn env_u64(name: &str) -> Option<u64> {
        std::env::var(name).ok().and_then(|s| s.parse().ok())
    }

    /// Encoded access units (Annex B, keyframe?) from the encoder callback.
    static ENCODED: Mutex<Vec<(Vec<u8>, bool)>> = Mutex::new(Vec::new());

    /// Encode a test pattern and feed it to the sink at 60 fps.
    fn feed(mut sink: Box<dyn VideoSink>) {
        let switch = env_u64("BP_SPIKE_SWITCH").map(Duration::from_secs);
        let sizes = [(1280usize, 720usize), (960, 720)];
        let start = Instant::now();
        let mut frame_no: u64 = 0;
        for round in 0.. {
            let (w, h) = sizes[if switch.is_some() { round % 2 } else { 0 }];
            let encoder = Encoder::new(w, h).expect("encoder");
            sink.configure(&StreamConfig { codec: "avc1.4d0028".into(), width: w as u32, height: h as u32 })
                .expect("configure");
            let seg_start = Instant::now();
            let mut force_key = true;
            let mut shown = 0u64;
            let mut skipped = 0u64;
            loop {
                let t = start.elapsed().as_secs_f64();
                encoder.encode(t, frame_no, force_key);
                force_key = false;
                for (data, key) in ENCODED.lock().unwrap().drain(..) {
                    let display = frame_no % 30 != 15;
                    let res = sink.submit(&Frame { data: &data, key, ts_us: (t * 1e6) as i64, display });
                    match res {
                        Ok(()) if display => shown += 1,
                        Ok(()) => skipped += 1,
                        Err(e) if e.downcast_ref::<NeedKeyframe>().is_some() => force_key = true,
                        Err(e) => {
                            eprintln!("submit failed: {e:#}");
                            force_key = true;
                        }
                    }
                }
                frame_no += 1;
                if frame_no % 300 == 0 {
                    println!("{w}x{h}: {shown} shown, {skipped} decode-only");
                }
                // 60 fps pacing.
                let next = Duration::from_secs_f64(frame_no as f64 / 60.0);
                if let Some(d) = next.checked_sub(start.elapsed()) {
                    std::thread::sleep(d);
                }
                if switch.is_some_and(|s| seg_start.elapsed() >= s) {
                    break;
                }
            }
        }
    }

    struct Encoder {
        session: CFRetained<VTCompressionSession>,
        w: usize,
        h: usize,
        /// Static part of the pattern (checkerboard + border), BGRA.
        background: Vec<u8>,
    }

    impl Encoder {
        fn new(w: usize, h: usize) -> Result<Self, i32> {
            let mut session: *mut VTCompressionSession = ptr::null_mut();
            // SAFETY: plain VideoToolbox calls; `session` receives a +1 reference.
            #[allow(deprecated)]
            unsafe {
                let status = VTCompressionSession::create(
                    None,
                    w as i32,
                    h as i32,
                    objc2_core_media::kCMVideoCodecType_H264,
                    None,
                    None,
                    None,
                    Some(on_encoded),
                    ptr::null_mut(),
                    NonNull::from(&mut session),
                );
                if status != 0 || session.is_null() {
                    return Err(status);
                }
                let session = CFRetained::from_raw(NonNull::new_unchecked(session));
                let yes: &CFType = kCFBooleanTrue.unwrap();
                let no: &CFType = kCFBooleanFalse.unwrap();
                let interval = CFNumber::new_i32(120);
                VTSessionSetProperty(&session, kVTCompressionPropertyKey_RealTime, Some(yes));
                VTSessionSetProperty(&session, kVTCompressionPropertyKey_AllowFrameReordering, Some(no));
                VTSessionSetProperty(&session, kVTCompressionPropertyKey_MaxKeyFrameInterval, Some(&interval));
                VTSessionSetProperty(&session, kVTCompressionPropertyKey_ProfileLevel, Some(kVTProfileLevel_H264_Main_AutoLevel));

                let mut background = vec![0u8; w * h * 4];
                let cell = 80;
                for y in 0..h {
                    for x in 0..w {
                        let border = x < 8 || y < 8 || x >= w - 8 || y >= h - 8;
                        let light = ((x / cell) + (y / cell)) % 2 == 0;
                        let (r, g, b) = if border {
                            (25, 230, 75)
                        } else if light {
                            (50, 60, 128)
                        } else {
                            (25, 30, 76)
                        };
                        background[(y * w + x) * 4..][..4].copy_from_slice(&[b, g, r, 255]);
                    }
                }
                Ok(Self { session, w, h, background })
            }
        }

        /// Draw frame `n` (a sweeping yellow bar) and encode it synchronously.
        fn encode(&self, t: f64, n: u64, force_key: bool) {
            let (w, h) = (self.w, self.h);
            let mut pb: *mut CVPixelBuffer = ptr::null_mut();
            // SAFETY: plain CoreVideo/VideoToolbox calls on buffers we own.
            #[allow(deprecated)]
            unsafe {
                use objc2_core_video::*;
                if CVPixelBufferCreate(None, w, h, kCVPixelFormatType_32BGRA, None, NonNull::from(&mut pb)) != 0 {
                    return;
                }
                let pb = CFRetained::from_raw(NonNull::new_unchecked(pb));
                objc2_core_video::CVPixelBufferLockBaseAddress(&pb, objc2_core_video::CVPixelBufferLockFlags(0));
                let base = CVPixelBufferGetBaseAddress(&pb) as *mut u8;
                let stride = CVPixelBufferGetBytesPerRow(&pb);
                let bar = 120;
                let x0 = ((t * 400.0) as usize) % (w + bar);
                for y in 0..h {
                    let row = std::slice::from_raw_parts_mut(base.add(y * stride), w * 4);
                    row.copy_from_slice(&self.background[y * w * 4..][..w * 4]);
                    for x in x0.saturating_sub(bar)..x0.min(w) {
                        row[x * 4..][..4].copy_from_slice(&[25, 205, 255, 255]);
                    }
                }
                // A white square that hops every frame (shows skipped frames).
                let sx = 40 + (n % 30) as usize * 30;
                for y in h - 80..h - 50 {
                    let row = std::slice::from_raw_parts_mut(base.add(y * stride), w * 4);
                    for x in sx..(sx + 24).min(w) {
                        row[x * 4..][..4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
                objc2_core_video::CVPixelBufferUnlockBaseAddress(&pb, objc2_core_video::CVPixelBufferLockFlags(0));

                let props = force_key.then(|| {
                    let key: &CFString = kVTEncodeFrameOptionKey_ForceKeyFrame;
                    let yes: &CFType = kCFBooleanTrue.unwrap();
                    CFDictionary::<CFString, CFType>::from_slices(&[key], &[yes])
                });
                let pts = CMTime::new((t * 1e6) as i64, 1_000_000);
                self.session.encode_frame(
                    &pb,
                    pts,
                    objc2_core_media::kCMTimeInvalid,
                    props.as_deref().map(|d| d.as_opaque()),
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                self.session.complete_frames(pts);
            }
        }
    }

    /// VideoToolbox output: AVCC sample → Annex B access unit (SPS/PPS on keyframes).
    unsafe extern "C-unwind" fn on_encoded(
        _refcon: *mut c_void,
        _frame_refcon: *mut c_void,
        status: i32,
        _flags: VTEncodeInfoFlags,
        sample: *mut CMSampleBuffer,
    ) {
        if status != 0 || sample.is_null() {
            return;
        }
        let sample = &*sample;
        let key = match sample.sample_attachments_array(false) {
            Some(arr) if arr.count() > 0 => {
                let dict = &*(arr.value_at_index(0) as *const CFDictionary);
                !dict.contains_ptr_key(objc2_core_media::kCMSampleAttachmentKey_NotSync as *const _ as *const c_void)
            }
            _ => true,
        };
        let Some(block) = sample.data_buffer() else { return };
        let len = block.data_length();
        let mut avcc = vec![0u8; len];
        if block.copy_data_bytes(0, len, NonNull::new_unchecked(avcc.as_mut_ptr().cast())) != 0 {
            return;
        }
        let mut out = Vec::with_capacity(len + 64);
        if key {
            let Some(fmt) = sample.format_description() else { return };
            for i in 0..2 {
                let mut p: *const u8 = ptr::null();
                let mut size = 0usize;
                #[allow(deprecated)]
                let st = objc2_core_media::CMVideoFormatDescriptionGetH264ParameterSetAtIndex(
                    &fmt,
                    i,
                    &mut p,
                    &mut size,
                    ptr::null_mut(),
                    ptr::null_mut(),
                );
                if st != 0 {
                    return;
                }
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(std::slice::from_raw_parts(p, size));
            }
        }
        let mut i = 0;
        while i + 4 <= len {
            let n = u32::from_be_bytes(avcc[i..i + 4].try_into().unwrap()) as usize;
            i += 4;
            if i + n > len {
                break;
            }
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(&avcc[i..i + n]);
            i += n;
        }
        ENCODED.lock().unwrap().push((out, key));
    }
}
