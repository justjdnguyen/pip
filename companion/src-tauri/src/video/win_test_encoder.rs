//! Test-only: encode a synthetic picture sequence to H.264 Annex B with the
//! Media Foundation (software) encoder, so tests and spikes need no files or ffmpeg.

use std::mem::ManuallyDrop;

use windows::core::{Interface, Result};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Variant::*;

pub struct Unit {
    pub data: Vec<u8>,
    pub key: bool,
    pub ts_us: i64,
}

/// Encode `frames` pictures whose luma is `luma(x, y, frame)` (chroma grey).
/// Keyframe every `gop` frames. Call `init_thread()` first.
pub fn encode(w: u32, h: u32, fps: u32, frames: usize, gop: u32, luma: impl Fn(usize, usize, usize) -> u8) -> Result<Vec<Unit>> {
    unsafe {
        let enc: IMFTransform = CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER)?;
        let common = |t: &IMFMediaType| -> Result<()> {
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetUINT64(&MF_MT_FRAME_RATE, (fps as u64) << 32 | 1)?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, (w as u64) << 32 | h as u64)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, 1 << 32 | 1)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        };
        let out_t = MFCreateMediaType()?;
        common(&out_t)?;
        out_t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        out_t.SetUINT32(&MF_MT_AVG_BITRATE, 8_000_000)?;
        out_t.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
        enc.SetOutputType(0, &out_t, 0)?;
        let in_t = MFCreateMediaType()?;
        common(&in_t)?;
        in_t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        enc.SetInputType(0, &in_t, 0)?;
        if let Ok(api) = enc.cast::<ICodecAPI>() {
            let _ = api.SetValue(&CODECAPI_AVEncMPVGOPSize, &variant_u32(gop));
            let _ = api.SetValue(&CODECAPI_AVLowLatencyMode, &variant_bool(true));
        }
        enc.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
        enc.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
        let out_size = enc.GetOutputStreamInfo(0)?.cbSize.max(w * h * 2);

        let (wu, hu) = (w as usize, h as usize);
        let mut units = Vec::new();
        for i in 0..frames {
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
            sample.SetSampleTime(i as i64 * 10_000_000 / fps as i64)?;
            sample.SetSampleDuration(10_000_000 / fps as i64)?;
            enc.ProcessInput(0, &sample, 0)?;
            pull(&enc, out_size, &mut units)?;
        }
        enc.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?;
        enc.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?;
        pull(&enc, out_size, &mut units)?;
        Ok(units)
    }
}

unsafe fn pull(enc: &IMFTransform, size: u32, units: &mut Vec<Unit>) -> Result<()> {
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
                units.push(Unit { data, key, ts_us: sample.GetSampleTime()? / 10 });
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
