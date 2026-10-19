//! The Windows video sink: H.264 → Media Foundation decoder (GPU) → D3D11
//! video processor (NV12 → BGRA, crop) → a composition swap chain the size of
//! the picture, shown under the web view by `win_window`.
//!
//! Created on the UI thread (it builds the composition tree), then used from
//! the video thread.

use std::collections::HashMap;
use std::mem::ManuallyDrop;

use anyhow::{anyhow, bail, Context};
use windows::core::Interface;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;

use super::win_decoder::{create_device, init_thread, DecodedFrame, Decoder};
use super::win_window;
use super::{Frame, StreamConfig, VideoSink};

/// Swap chain size before the first picture.
const INITIAL: (u32, u32) = (16, 16);

struct Processor {
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    output: ID3D11VideoProcessorOutputView,
    /// (decoder texture size, visible size, output size) it was built for.
    key: ((u32, u32), (u32, u32), (u32, u32)),
}

pub struct WinSink {
    hwnd: HWND,
    device: ID3D11Device,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext1,
    swap_chain: IDXGISwapChain1,
    size: (u32, u32),
    config: Option<(u32, u32)>,
    decoder: Option<Decoder>,
    processor: Option<Processor>,
    /// Input views by (decoder texture, array slice). The decoder reuses a fixed pool.
    views: HashMap<(usize, u32), ID3D11VideoProcessorInputView>,
    decoded: Vec<DecodedFrame>,
    com_ready: bool,
}

// SAFETY: D3D11 (with multithread protection on) and the swap chain are fine to
// use from the video thread; nothing here is touched by two threads at once.
unsafe impl Send for WinSink {}

impl WinSink {
    /// Set up the device, swap chain and composition tree. Call on the UI thread.
    pub fn new(hwnd: HWND) -> anyhow::Result<Self> {
        unsafe {
            let (device, context) = create_device().context("D3D11 device")?;
            let video_device: ID3D11VideoDevice = device.cast()?;
            let video_context: ID3D11VideoContext1 = context.cast()?;
            let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
            let desc = DXGI_SWAP_CHAIN_DESC1 {
                Width: INITIAL.0,
                Height: INITIAL.1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
                BufferCount: 2,
                Scaling: DXGI_SCALING_STRETCH,
                SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
                AlphaMode: DXGI_ALPHA_MODE_IGNORE,
                Flags: 0,
                ..Default::default()
            };
            let swap_chain = factory.CreateSwapChainForComposition(&device, &desc, None).context("swap chain")?;
            win_window::attach_video(hwnd, &device, &context, &swap_chain.cast()?).context("composition")?;
            Ok(WinSink {
                hwnd,
                device,
                video_device,
                video_context,
                swap_chain,
                size: INITIAL,
                config: None,
                decoder: None,
                processor: None,
                views: HashMap::new(),
                decoded: Vec::new(),
                com_ready: false,
            })
        }
    }

    /// Show one decoded picture.
    unsafe fn present(&mut self, f: &DecodedFrame) -> anyhow::Result<()> {
        let dec = self.decoder.as_ref().unwrap();
        let (coded, visible) = (dec.coded_size(), dec.display_size());
        if self.size != visible {
            // New picture size: resize the swap chain (all views on it must go first).
            self.processor = None;
            self.swap_chain.ResizeBuffers(0, visible.0, visible.1, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
            self.size = visible;
            win_window::set_video_size(self.hwnd, visible.0, visible.1);
        }
        if self.processor.as_ref().map(|p| p.key) != Some((coded, visible, self.size)) {
            self.processor = Some(self.make_processor(coded, visible)?);
            self.views.clear();
        }
        let p = self.processor.as_ref().unwrap();

        let key = (f.texture.as_raw() as usize, f.subresource);
        if !self.views.contains_key(&key) {
            let desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
                FourCC: 0,
                ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
                Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_VPIV { MipSlice: 0, ArraySlice: f.subresource },
                },
            };
            let mut view = None;
            self.video_device.CreateVideoProcessorInputView(&f.texture, &p.enumerator, &desc, Some(&mut view))?;
            // The decoder keeps a small pool; a growing map means it reallocated.
            if self.views.len() > 64 {
                self.views.clear();
            }
            self.views.insert(key, view.unwrap());
        }
        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            pInputSurface: ManuallyDrop::new(Some(self.views[&key].clone())),
            ..std::mem::zeroed()
        };
        let streams = [stream];
        let r = self.video_context.VideoProcessorBlt(&p.processor, &p.output, 0, &streams);
        let [stream] = streams;
        drop(ManuallyDrop::into_inner(stream.pInputSurface));
        r.context("video processor blit")?;
        // Sync interval 0: the pacing thread already times frames; don't block on vsync.
        self.swap_chain.Present(0, DXGI_PRESENT(0)).ok().context("present")?;
        Ok(())
    }

    unsafe fn make_processor(&self, coded: (u32, u32), visible: (u32, u32)) -> anyhow::Result<Processor> {
        let rate = DXGI_RATIONAL { Numerator: 60, Denominator: 1 };
        let desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: rate,
            InputWidth: coded.0,
            InputHeight: coded.1,
            OutputFrameRate: rate,
            OutputWidth: self.size.0,
            OutputHeight: self.size.1,
            Usage: D3D11_VIDEO_USAGE_PLAYBACK_NORMAL,
        };
        let enumerator = self.video_device.CreateVideoProcessorEnumerator(&desc)?;
        let processor = self.video_device.CreateVideoProcessor(&enumerator, 0)?;
        let vc = &self.video_context;
        let src = RECT { left: 0, top: 0, right: visible.0 as i32, bottom: visible.1 as i32 };
        let dst = RECT { left: 0, top: 0, right: self.size.0 as i32, bottom: self.size.1 as i32 };
        vc.VideoProcessorSetStreamFrameFormat(&processor, 0, D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE);
        vc.VideoProcessorSetStreamAutoProcessingMode(&processor, 0, false);
        vc.VideoProcessorSetStreamSourceRect(&processor, 0, true, Some(&src));
        vc.VideoProcessorSetStreamDestRect(&processor, 0, true, Some(&dst));
        vc.VideoProcessorSetOutputTargetRect(&processor, true, Some(&dst));
        // Browser encoders tag their output BT.709 limited range.
        vc.VideoProcessorSetStreamColorSpace1(&processor, 0, DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709);
        vc.VideoProcessorSetOutputColorSpace1(&processor, DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709);

        let back: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
        let out_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 { Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 } },
        };
        let mut output = None;
        self.video_device.CreateVideoProcessorOutputView(&back, &enumerator, &out_desc, Some(&mut output))?;
        Ok(Processor { enumerator, processor, output: output.unwrap(), key: (coded, visible, self.size) })
    }
}

impl VideoSink for WinSink {
    fn configure(&mut self, cfg: &StreamConfig) -> anyhow::Result<()> {
        if !cfg.codec.starts_with("avc1") {
            bail!("codec {} not supported natively", cfg.codec);
        }
        if !self.com_ready {
            init_thread().context("Media Foundation")?;
            self.com_ready = true;
        }
        let size = (cfg.width, cfg.height);
        if self.config != Some(size) || self.decoder.is_none() {
            self.decoder = None;
            self.views.clear();
            self.decoder = Some(Decoder::new(&self.device, size.0, size.1).context("H.264 decoder")?);
            self.config = Some(size);
        }
        Ok(())
    }

    fn submit(&mut self, frame: &Frame) -> anyhow::Result<()> {
        let dec = self.decoder.as_mut().ok_or_else(|| anyhow!("not configured"))?;
        let mut decoded = std::mem::take(&mut self.decoded);
        let r = dec.decode(frame.data, frame.key, frame.ts_us, &mut decoded).context("decode");
        // Show only the newest picture; the rest go straight back to the decoder.
        let shown = match (&r, frame.display, decoded.pop()) {
            (Ok(()), true, Some(f)) => unsafe { self.present(&f) },
            _ => Ok(()),
        };
        decoded.clear();
        self.decoded = decoded;
        r.and(shown)
    }

    fn flush(&mut self) {
        if let Some(d) = self.decoder.as_mut() {
            d.flush();
        }
    }
}
