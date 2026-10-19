// Test: Media Foundation H.264 decoding on the GPU (Windows).
//
// Encodes a moving checkerboard with the Media Foundation H.264 encoder (so no
// test files or ffmpeg are needed), then decodes it with `video::win_decoder`
// and checks: one picture out per access unit in (low latency), the output is
// NV12 on the GPU, the visible size is right (incl. cropping for sizes that
// aren't whole macroblocks), and the pixels match what was encoded.
//
// Run: cargo run --bin mf_decode_test

#![cfg_attr(not(windows), allow(unused))]

#[path = "../video/mod.rs"]
mod video;

#[cfg(not(windows))]
fn main() {
    eprintln!("mf_decode_test is Windows-only");
}

#[cfg(windows)]
fn main() {
    let mut ok = true;
    for (w, h) in [(1280, 720), (854, 480), (1920, 1080)] {
        match win::run(w, h) {
            Ok(true) => println!("PASS {w}x{h}\n"),
            Ok(false) => {
                println!("FAIL {w}x{h}\n");
                ok = false;
            }
            Err(e) => {
                println!("ERROR {w}x{h}: {e}\n");
                ok = false;
            }
        }
    }
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(windows)]
mod win {
    use std::mem::ManuallyDrop;
    use std::time::Instant;

    use super::video::annexb;
    use super::video::win_decoder::{create_device, init_thread, DecodedFrame, Decoder};
    use windows::core::{Interface, Result};
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Dxgi::Common::*;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::System::Variant::*;

    const FPS: u32 = 60;
    const FRAMES: usize = 120;
    const CELL: usize = 64;

    /// Luma of the test pattern: a checkerboard that moves 8 px right per frame.
    fn luma(x: usize, y: usize, i: usize) -> u8 {
        if ((x + 8 * i) / CELL + y / CELL) % 2 == 0 {
            40
        } else {
            210
        }
    }

    pub fn run(w: u32, h: u32) -> Result<bool> {
        init_thread()?;
        println!("== {w}x{h}");
        let t = Instant::now();
        let units = encode(w, h)?;
        let keys = units.iter().filter(|u| u.1).count();
        let first: Vec<u8> = annexb::nal_units(&units[0].0).map(annexb::nal_type).collect();
        println!(
            "encoded {} frames in {:?}, {} keyframes, {} KB, first AU NAL types {first:?}",
            units.len(),
            t.elapsed(),
            keys,
            units.iter().map(|u| u.0.len()).sum::<usize>() / 1024
        );

        let (device, context) = create_device()?;
        let mut dec = Decoder::new(&device, w, h)?;
        let mut out = Vec::new();
        let mut lag = Vec::new(); // inputs fed before each picture came out
        let mut times = Vec::new();
        let mut checked = false;
        let mut ok = true;
        for (i, (data, key, ts)) in units.iter().enumerate() {
            let t = Instant::now();
            dec.decode(data, *key, *ts, &mut out)?;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            for f in out.drain(..) {
                let n = ((f.ts_us * FPS as i64 + 500_000) / 1_000_000) as usize;
                lag.push(i - n);
                if n == FRAMES / 2 && !checked {
                    checked = true;
                    ok &= check(&device, &context, &f, &dec, w, h, n)?;
                }
            }
        }
        let late: Vec<usize> = lag.iter().enumerate().filter(|(_, &l)| l > 0).map(|(n, _)| n).collect();
        if !late.is_empty() {
            println!("late frames: {late:?}");
        }
        let decoded = lag.len();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "decoded {decoded}/{} frames; coded {:?}, visible {:?}; frames of delay: max {}; decode call ms: median {:.2}, max {:.2}",
            units.len(),
            dec.coded_size(),
            dec.display_size(),
            lag.iter().max().unwrap_or(&0),
            times[times.len() / 2],
            times[times.len() - 1]
        );
        ok &= decoded == units.len();
        ok &= lag.iter().all(|&l| l == 0);
        ok &= dec.display_size() == (w, h);
        ok &= checked;
        Ok(ok)
    }

    /// Copy the picture to the CPU and compare luma at cell centres.
    fn check(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        f: &DecodedFrame,
        dec: &Decoder,
        w: u32,
        h: u32,
        n: usize,
    ) -> Result<bool> {
        unsafe {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            f.texture.GetDesc(&mut desc);
            println!(
                "frame {n}: texture {}x{} format {:?} array {} slice {} bind {:#x}",
                desc.Width, desc.Height, desc.Format, desc.ArraySize, f.subresource, desc.BindFlags
            );
            if desc.Format != DXGI_FORMAT_NV12 {
                return Ok(false);
            }
            let staging_desc = D3D11_TEXTURE2D_DESC {
                ArraySize: 1,
                BindFlags: 0,
                Usage: D3D11_USAGE_STAGING,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
                ..desc
            };
            let mut staging = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging))?;
            let staging = staging.unwrap();
            context.CopySubresourceRegion(&staging, 0, 0, 0, 0, &f.texture, f.subresource, None);
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            let y_plane = std::slice::from_raw_parts(map.pData as *const u8, map.RowPitch as usize * h as usize);
            let (dw, dh) = dec.display_size();
            let (mut worst, mut samples) = (0i32, 0);
            // Sample on a 16 px grid, skipping points near cell edges (where coding error is largest).
            let inside = |o: usize| (8..CELL - 8).contains(&(o % CELL));
            for y in (0..dh.min(h) as usize).step_by(16).filter(|&y| inside(y)) {
                for x in (0..dw.min(w) as usize).step_by(16).filter(|&x| inside(x + 8 * n)) {
                    let got = y_plane[y * map.RowPitch as usize + x] as i32;
                    worst = worst.max((got - luma(x, y, n) as i32).abs());
                    samples += 1;
                }
            }
            context.Unmap(&staging, 0);
            println!("frame {n}: {samples} luma samples, worst error {worst}");
            Ok(worst <= 24)
        }
    }

    /// Encode the test pattern to Annex B access units: (bytes, keyframe, timestamp Âµs).
    fn encode(w: u32, h: u32) -> Result<Vec<(Vec<u8>, bool, i64)>> {
        unsafe {
            let enc: IMFTransform = CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER)?;
            let out_t = MFCreateMediaType()?;
            out_t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out_t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out_t.SetUINT32(&MF_MT_AVG_BITRATE, 8_000_000)?;
            out_t.SetUINT64(&MF_MT_FRAME_RATE, (FPS as u64) << 32 | 1)?;
            out_t.SetUINT64(&MF_MT_FRAME_SIZE, (w as u64) << 32 | h as u64)?;
            out_t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, 1 << 32 | 1)?;
            out_t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            out_t.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
            enc.SetOutputType(0, &out_t, 0)?;
            let in_t = MFCreateMediaType()?;
            in_t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            in_t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            in_t.SetUINT64(&MF_MT_FRAME_RATE, (FPS as u64) << 32 | 1)?;
            in_t.SetUINT64(&MF_MT_FRAME_SIZE, (w as u64) << 32 | h as u64)?;
            in_t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, 1 << 32 | 1)?;
            in_t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            enc.SetInputType(0, &in_t, 0)?;
            if let Ok(api) = enc.cast::<ICodecAPI>() {
                let _ = api.SetValue(&CODECAPI_AVEncMPVGOPSize, &variant_u32(30));
                let _ = api.SetValue(&CODECAPI_AVLowLatencyMode, &variant_bool(true));
            }
            enc.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            enc.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            let out_size = enc.GetOutputStreamInfo(0)?.cbSize.max(w * h * 2);

            let (wu, hu) = (w as usize, h as usize);
            let mut units = Vec::new();
            for i in 0..FRAMES {
                let len = wu * hu * 3 / 2;
                let buffer = MFCreateMemoryBuffer(len as u32)?;
                let mut ptr = std::ptr::null_mut();
                buffer.Lock(&mut ptr, None, None)?;
                let px = std::slice::from_raw_parts_mut(ptr, len);
                for y in 0..hu {
                    for x in 0..wu {
                        px[y * wu + x] = luma(x, y, i);
                    }
                }
                px[wu * hu..].fill(128);
                buffer.Unlock()?;
                buffer.SetCurrentLength(len as u32)?;
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                sample.SetSampleTime(i as i64 * 10_000_000 / FPS as i64)?;
                sample.SetSampleDuration(10_000_000 / FPS as i64)?;
                enc.ProcessInput(0, &sample, 0)?;
                pull(&enc, out_size, &mut units)?;
            }
            enc.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
            enc.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
            pull(&enc, out_size, &mut units)?;
            Ok(units)
        }
    }

    unsafe fn pull(enc: &IMFTransform, size: u32, units: &mut Vec<(Vec<u8>, bool, i64)>) -> Result<()> {
        loop {
            let sample = MFCreateSample()?;
            sample.AddBuffer(&MFCreateMemoryBuffer(size)?)?;
            let mut buf = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(Some(sample.clone())),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0;
            let r = enc.ProcessOutput(0, &mut buf, &mut status);
            let [buf] = buf;
            drop(ManuallyDrop::into_inner(buf.pSample));
            drop(ManuallyDrop::into_inner(buf.pEvents));
            match r {
                Ok(()) => {
                    let b = sample.ConvertToContiguousBuffer()?;
                    let mut ptr = std::ptr::null_mut();
                    let mut len = 0;
                    b.Lock(&mut ptr, None, Some(&mut len))?;
                    let data = std::slice::from_raw_parts(ptr, len as usize).to_vec();
                    b.Unlock()?;
                    let key = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
                    units.push((data, key, sample.GetSampleTime()? / 10));
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    fn variant_u32(v: u32) -> VARIANT {
        let mut var = VARIANT::default();
        unsafe {
            (*var.Anonymous.Anonymous).vt = VT_UI4;
            (*var.Anonymous.Anonymous).Anonymous.ulVal = v;
        }
        var
    }

    fn variant_bool(v: bool) -> VARIANT {
        let mut var = VARIANT::default();
        unsafe {
            (*var.Anonymous.Anonymous).vt = VT_BOOL;
            (*var.Anonymous.Anonymous).Anonymous.boolVal = v.into();
        }
        var
    }
}
