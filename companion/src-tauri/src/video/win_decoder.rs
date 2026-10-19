//! H.264 decoding on Windows: the Media Foundation H.264 decoder MFT, given a
//! D3D11 device manager so it decodes on the GPU (DXVA) into NV12 textures.
//! Low-latency mode, so each access unit comes out as soon as it goes in.

use std::mem::ManuallyDrop;

use windows::core::{Interface, Result};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

/// Start COM (multithreaded) and Media Foundation on this thread. Safe to repeat.
pub fn init_thread() -> Result<()> {
    unsafe {
        // S_FALSE (already initialised) is fine; RPC_E_CHANGED_MODE means the
        // thread is STA, which also works for in-process MFTs.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)
    }
}

/// A hardware D3D11 device usable for video decode and processing, with
/// multithread protection on (the decoder uses it from its own threads).
pub fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    unsafe {
        let mut device = None;
        let mut context = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        let device = device.unwrap();
        let mt: ID3D11Multithread = device.cast()?;
        let _ = mt.SetMultithreadProtected(true);
        Ok((device, context.unwrap()))
    }
}

/// One decoded picture: a slice of the decoder's NV12 texture array.
/// Holding it keeps the surface out of the decoder's pool, so drop it soon.
pub struct DecodedFrame {
    pub texture: ID3D11Texture2D,
    pub subresource: u32,
    pub ts_us: i64,
    _sample: IMFSample,
}

pub struct Decoder {
    mft: IMFTransform,
    _manager: IMFDXGIDeviceManager,
    /// Size the encoder was configured with (the visible picture).
    width: u32,
    height: u32,
    /// Texture size (rounded up to whole macroblocks).
    coded: (u32, u32),
    /// Visible area within the texture.
    display: (u32, u32),
}

// SAFETY: the MFT and device manager are free-threaded COM objects; the decoder
// is used from one thread at a time.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(device: &ID3D11Device, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let mft: IMFTransform = CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)?;
            let attrs = mft.GetAttributes()?;
            if attrs.GetUINT32(&MF_SA_D3D11_AWARE).unwrap_or(0) == 0 {
                return Err(windows::core::Error::new(
                    windows::Win32::Foundation::E_NOTIMPL,
                    "H.264 decoder is not D3D11-aware",
                ));
            }
            attrs.SetUINT32(&MF_LOW_LATENCY, 1)?;

            let mut token = 0;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager = manager.unwrap();
            manager.ResetDevice(device, token)?;
            mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)?;

            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, pack(width, height))?;
            input.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            mft.SetInputType(0, &input, 0)?;

            let mut dec = Decoder { mft, _manager: manager, width, height, coded: (width, height), display: (width, height) };
            dec.set_output_type()?;

            let info = dec.mft.GetOutputStreamInfo(0)?;
            if info.dwFlags & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32 == 0 {
                return Err(windows::core::Error::new(
                    windows::Win32::Foundation::E_NOTIMPL,
                    "H.264 decoder is not decoding to GPU textures",
                ));
            }
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?;
            dec.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            Ok(dec)
        }
    }

    /// Texture size of decoded frames.
    pub fn coded_size(&self) -> (u32, u32) {
        self.coded
    }

    /// Visible picture size within the texture (top-left aligned).
    pub fn display_size(&self) -> (u32, u32) {
        self.display
    }

    /// Feed one access unit; append any pictures that are ready to `out`.
    pub fn decode(&mut self, data: &[u8], key: bool, ts_us: i64, out: &mut Vec<DecodedFrame>) -> Result<()> {
        unsafe {
            let buffer = MFCreateMemoryBuffer(data.len() as u32)?;
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            buffer.Unlock()?;
            buffer.SetCurrentLength(data.len() as u32)?;
            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(ts_us * 10)?;
            if key {
                sample.SetUINT32(&MFSampleExtension_CleanPoint, 1)?;
            }

            // The decoder refuses input while it holds a finished picture; take it first.
            for _ in 0..4 {
                match self.mft.ProcessInput(0, &sample, 0) {
                    Ok(()) => return self.collect(out),
                    Err(e) if e.code() == MF_E_NOTACCEPTING => self.collect(out)?,
                    Err(e) => return Err(e),
                }
            }
            Err(windows::core::Error::new(MF_E_NOTACCEPTING, "decoder stuck: not accepting input"))
        }
    }

    /// Drop everything in flight (seek / stream restart).
    pub fn flush(&mut self) {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }

    /// Take every finished picture.
    fn collect(&mut self, out: &mut Vec<DecodedFrame>) -> Result<()> {
        unsafe {
            loop {
                let mut buf = [MFT_OUTPUT_DATA_BUFFER {
                    dwStreamID: 0,
                    pSample: ManuallyDrop::new(None),
                    dwStatus: 0,
                    pEvents: ManuallyDrop::new(None),
                }];
                let mut status = 0;
                let r = self.mft.ProcessOutput(0, &mut buf, &mut status);
                let [buf] = buf;
                let sample = ManuallyDrop::into_inner(buf.pSample);
                drop(ManuallyDrop::into_inner(buf.pEvents));
                match r {
                    Ok(()) => {
                        if let Some(s) = sample {
                            out.push(self.wrap(s)?);
                        }
                    }
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    // New stream format (first keyframe, or a size change): pick NV12 again.
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.set_output_type()?,
                    Err(e) => return Err(e),
                }
            }
        }
    }

    unsafe fn wrap(&self, sample: IMFSample) -> Result<DecodedFrame> {
        let dxgi: IMFDXGIBuffer = sample.GetBufferByIndex(0)?.cast()?;
        let mut raw = std::ptr::null_mut();
        dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw)?;
        Ok(DecodedFrame {
            texture: ID3D11Texture2D::from_raw(raw),
            subresource: dxgi.GetSubresourceIndex()?,
            ts_us: sample.GetSampleTime()? / 10,
            _sample: sample,
        })
    }

    unsafe fn set_output_type(&mut self) -> Result<()> {
        for i in 0.. {
            let t = self.mft.GetOutputAvailableType(0, i)?;
            if t.GetGUID(&MF_MT_SUBTYPE)? != MFVideoFormat_NV12 {
                continue;
            }
            self.mft.SetOutputType(0, &t, 0)?;
            let size = t.GetUINT64(&MF_MT_FRAME_SIZE)?;
            self.coded = ((size >> 32) as u32, size as u32);
            self.display = aperture(&t).unwrap_or((self.width.min(self.coded.0), self.height.min(self.coded.1)));
            return Ok(());
        }
        unreachable!()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
        }
    }
}

fn pack(hi: u32, lo: u32) -> u64 {
    (hi as u64) << 32 | lo as u64
}

/// The visible area from a media type (H.264 cropping), if it starts at the top-left.
unsafe fn aperture(t: &IMFMediaType) -> Option<(u32, u32)> {
    let mut raw = [0u8; std::mem::size_of::<MFVideoArea>()];
    for key in [&MF_MT_MINIMUM_DISPLAY_APERTURE, &MF_MT_GEOMETRIC_APERTURE] {
        if t.GetBlob(key, &mut raw, None).is_ok() {
            let area: MFVideoArea = std::ptr::read_unaligned(raw.as_ptr().cast());
            if area.OffsetX.value == 0 && area.OffsetY.value == 0 && area.Area.cx > 0 && area.Area.cy > 0 {
                return Some((area.Area.cx as u32, area.Area.cy as u32));
            }
        }
    }
    None
}
