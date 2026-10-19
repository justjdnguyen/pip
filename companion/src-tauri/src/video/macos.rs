// Native video sink for macOS: an AVSampleBufferDisplayLayer under the web view.
//
// The PiP window's content view holds the WKWebView. We add a layer-hosting
// NSView below it whose layer is an AVSampleBufferDisplayLayer, autoresizing
// to fill the window. The page is transparent except for the controls, so
// the video shows through. On resize AppKit sets the layer's frame in the same
// Core Animation transaction as the window, so the picture follows the edge.
//
// The layer is fed compressed H.264: each Annex B access unit is converted to
// AVCC (4-byte lengths), wrapped in a CMSampleBuffer with a format description
// built from the stream's SPS/PPS, and enqueued with DisplayImmediately (or
// DoNotDisplay for decode-only frames). The layer decodes in hardware.
//
// The page needs no private API to be transparent: whenever a window has a
// background colour, wry already turns off the WKWebView's own background.

// The layer's own enqueue/flush/status calls are deprecated since macOS 14 in
// favour of `sampleBufferRenderer`, which needs 14.0; we support 13.3.
#![allow(deprecated)]

use anyhow::{anyhow, bail, Context, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{msg_send, MainThreadMarker};
use objc2_av_foundation::{
    AVLayerVideoGravityResizeAspect, AVQueuedSampleBufferRenderingStatus, AVSampleBufferDisplayLayer,
};
use objc2_core_foundation::{kCFBooleanTrue, CFArray, CFMutableDictionary, CFRetained};
use objc2_core_graphics::CGColor;
use objc2_core_media::{
    kCMSampleAttachmentKey_DisplayImmediately, kCMSampleAttachmentKey_DoNotDisplay, kCMSampleAttachmentKey_NotSync,
    kCMTimeInvalid, CMBlockBuffer, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime,
};
use std::ffi::c_void;
use std::ptr::{self, NonNull};

use super::annexb::{nal_type, nal_units};
use super::{Frame, NeedKeyframe, StreamConfig, VideoSink};

const NS_WINDOW_BELOW: isize = -1;
const NS_VIEW_WIDTH_SIZABLE: usize = 1 << 1;
const NS_VIEW_HEIGHT_SIZABLE: usize = 1 << 4;

pub struct MacSink {
    window: tauri::WebviewWindow,
    layer: Retained<AVSampleBufferDisplayLayer>,
    /// The NSView holding the layer. Only touched on the main thread.
    view: Option<Retained<AnyObject>>,
    format: Option<CFRetained<CMFormatDescription>>,
    sps: Vec<u8>,
    pps: Vec<u8>,
    /// After a flush or reconfigure, decoding can only restart at a keyframe.
    need_key: bool,
    /// Reused AVCC buffer.
    avcc: Vec<u8>,
}

// SAFETY: off the main thread we only call the layer's sample-buffer methods
// (enqueue/flush/status/error/requiresFlushToResumeDecoding), which are
// documented as thread-safe. The view is only used on the main thread (Drop
// sends it there). CF objects (format description) are thread-safe.
unsafe impl Send for MacSink {}

impl MacSink {
    /// Must be called on the main thread.
    pub fn new(window: &tauri::WebviewWindow) -> Result<Self> {
        MainThreadMarker::new().context("create_sink must run on the main thread")?;
        let ns_window = window.ns_window().map_err(|e| anyhow!("no NSWindow: {e}"))? as *const AnyObject;
        // SAFETY: ns_window() is this window's live NSWindow, and we're on the main thread.
        unsafe {
            let ns_window = &*ns_window;
            let content: Option<Retained<AnyObject>> = msg_send![ns_window, contentView];
            let content = content.context("window has no content view")?;

            let layer = AVSampleBufferDisplayLayer::new();
            layer.setVideoGravity(AVLayerVideoGravityResizeAspect.context("AVLayerVideoGravityResizeAspect")?);
            // Letterbox bars.
            let black = CGColor::new_generic_rgb(0.0, 0.0, 0.0, 1.0);
            layer.setBackgroundColor(Some(&black));

            let bounds: objc2_foundation::NSRect = msg_send![&*content, bounds];
            let view: Retained<AnyObject> = msg_send![msg_send![objc2::class!(NSView), alloc], initWithFrame: bounds];
            // Layer-hosting view: set the layer first, then wantsLayer. AppKit
            // keeps the layer's frame equal to the view's bounds.
            let _: () = msg_send![&*view, setLayer: &*layer];
            let _: () = msg_send![&*view, setWantsLayer: true];
            let _: () = msg_send![&*view, setAutoresizingMask: NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
            // Below every existing subview (the WKWebView).
            let _: () = msg_send![&*content, addSubview: &*view, positioned: NS_WINDOW_BELOW, relativeTo: ptr::null::<AnyObject>()];

            Ok(Self {
                window: window.clone(),
                layer,
                view: Some(view),
                format: None,
                sps: Vec::new(),
                pps: Vec::new(),
                need_key: true,
                avcc: Vec::new(),
            })
        }
    }

    /// Error out if the layer can't decode any more (the core then requests a
    /// keyframe or falls back to the web path).
    fn check_layer(&mut self) -> Result<()> {
        // SAFETY: thread-safe sample-buffer methods (see `unsafe impl Send`).
        unsafe {
            if self.layer.status() == AVQueuedSampleBufferRenderingStatus::Failed {
                let msg = self.layer.error().map(|e| e.localizedDescription().to_string());
                self.layer.flush();
                self.need_key = true;
                bail!("display layer failed: {}", msg.unwrap_or_else(|| "unknown error".into()));
            }
            if self.layer.requiresFlushToResumeDecoding() {
                // E.g. after the decoder was taken away while the window was hidden.
                self.layer.flush();
                self.need_key = true;
                return Err(NeedKeyframe.into());
            }
        }
        Ok(())
    }

    fn update_format(&mut self, sps: &[u8], pps: &[u8]) -> Result<()> {
        if self.format.is_some() && sps == self.sps && pps == self.pps {
            return Ok(());
        }
        let ptrs = [NonNull::from(&sps[0]), NonNull::from(&pps[0])];
        let sizes = [sps.len(), pps.len()];
        let mut out: *const CMFormatDescription = ptr::null();
        // SAFETY: the pointers/sizes describe two live byte slices; `out` receives a +1 reference.
        let status = unsafe {
            objc2_core_media::CMVideoFormatDescriptionCreateFromH264ParameterSets(
                None,
                2,
                NonNull::from(&ptrs[0]),
                NonNull::from(&sizes[0]),
                4,
                NonNull::from(&mut out),
            )
        };
        if status != 0 || out.is_null() {
            bail!("CMVideoFormatDescriptionCreateFromH264ParameterSets failed: {status}");
        }
        // SAFETY: created above with a +1 retain count.
        self.format = Some(unsafe { CFRetained::from_raw(NonNull::new_unchecked(out as *mut _)) });
        self.sps = sps.to_vec();
        self.pps = pps.to_vec();
        Ok(())
    }

    fn make_sample(&self, frame: &Frame, format: &CMFormatDescription) -> Result<CFRetained<CMSampleBuffer>> {
        let len = self.avcc.len();
        // SAFETY: plain CoreMedia calls; out-pointers receive +1 references we wrap right away.
        unsafe {
            let mut block: *mut CMBlockBuffer = ptr::null_mut();
            let status = objc2_core_media::CMBlockBufferCreateWithMemoryBlock(
                None,
                ptr::null_mut(), // let CoreMedia allocate…
                len,
                None,
                ptr::null(),
                0,
                len,
                objc2_core_media::kCMBlockBufferAssureMemoryNowFlag,
                NonNull::from(&mut block),
            );
            if status != 0 || block.is_null() {
                bail!("CMBlockBufferCreateWithMemoryBlock failed: {status}");
            }
            let block = CFRetained::from_raw(NonNull::new_unchecked(block));
            // …then copy the AVCC data in.
            let status = objc2_core_media::CMBlockBufferReplaceDataBytes(
                NonNull::new_unchecked(self.avcc.as_ptr() as *mut c_void),
                &block,
                0,
                len,
            );
            if status != 0 {
                bail!("CMBlockBufferReplaceDataBytes failed: {status}");
            }

            let timing = CMSampleTimingInfo {
                duration: kCMTimeInvalid,
                presentationTimeStamp: CMTime::new(frame.ts_us, 1_000_000),
                decodeTimeStamp: kCMTimeInvalid,
            };
            let mut sample: *mut CMSampleBuffer = ptr::null_mut();
            let status = objc2_core_media::CMSampleBufferCreateReady(
                None,
                Some(&block),
                Some(format),
                1,
                1,
                &timing,
                1,
                &len,
                NonNull::from(&mut sample),
            );
            if status != 0 || sample.is_null() {
                bail!("CMSampleBufferCreateReady failed: {status}");
            }
            let sample = CFRetained::from_raw(NonNull::new_unchecked(sample));

            let attachments: CFRetained<CFArray> =
                sample.sample_attachments_array(true).context("no sample attachments")?;
            let dict = attachments.value_at_index(0) as *const CFMutableDictionary;
            let yes = kCFBooleanTrue.context("kCFBooleanTrue")? as *const _ as *const c_void;
            let key = if frame.display {
                kCMSampleAttachmentKey_DisplayImmediately
            } else {
                kCMSampleAttachmentKey_DoNotDisplay
            };
            CFMutableDictionary::set_value(dict.as_ref(), key as *const _ as *const c_void, yes);
            if !frame.key {
                CFMutableDictionary::set_value(
                    dict.as_ref(),
                    kCMSampleAttachmentKey_NotSync as *const _ as *const c_void,
                    yes,
                );
            }
            Ok(sample)
        }
    }
}

impl VideoSink for MacSink {
    fn configure(&mut self, cfg: &StreamConfig) -> Result<()> {
        if !cfg.codec.starts_with("avc") {
            bail!("unsupported codec {:?} (native video is H.264 only)", cfg.codec);
        }
        // New stream: the next keyframe's SPS/PPS make a fresh format description.
        // The old picture stays up until then (no black flash).
        self.format = None;
        self.need_key = true;
        Ok(())
    }

    fn submit(&mut self, frame: &Frame) -> Result<()> {
        self.check_layer()?;

        let mut sps: Option<&[u8]> = None;
        let mut pps: Option<&[u8]> = None;
        self.avcc.clear();
        for nal in nal_units(frame.data) {
            match nal_type(nal) {
                7 => sps = sps.or(Some(nal)),
                8 => pps = pps.or(Some(nal)),
                9 => {} // access unit delimiter: not needed in AVCC samples
                _ => {
                    self.avcc.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                    self.avcc.extend_from_slice(nal);
                }
            }
        }
        if let (Some(sps), Some(pps)) = (sps, pps) {
            self.update_format(sps, pps)?;
        }
        if self.need_key {
            if !frame.key || self.format.is_none() {
                return Err(NeedKeyframe.into());
            }
            self.need_key = false;
        }
        if self.avcc.is_empty() {
            return Ok(());
        }
        let format = self.format.clone().ok_or(NeedKeyframe)?;
        let sample = self.make_sample(frame, &format)?;
        // SAFETY: enqueue is thread-safe (see `unsafe impl Send`).
        unsafe { self.layer.enqueueSampleBuffer(&sample) };
        self.check_layer()
    }

    fn flush(&mut self) {
        // Keep the last picture on screen; just drop what's queued.
        // SAFETY: thread-safe (see `unsafe impl Send`).
        unsafe { self.layer.flush() };
        self.need_key = true;
    }
}

impl Drop for MacSink {
    fn drop(&mut self) {
        struct MainOnly(Retained<AnyObject>, Retained<AVSampleBufferDisplayLayer>);
        // SAFETY: only used inside the main-thread closure below.
        unsafe impl Send for MainOnly {}
        let Some(view) = self.view.take() else { return };
        let parts = MainOnly(view, self.layer.clone());
        let _ = self.window.run_on_main_thread(move || {
            let parts = parts;
            // SAFETY: on the main thread now.
            unsafe {
                parts.1.flushAndRemoveImage();
                let _: () = msg_send![&*parts.0, removeFromSuperview];
            }
        });
    }
}
