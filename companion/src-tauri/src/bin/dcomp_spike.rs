// Spike: native video under a transparent web page (Windows).
//
// Checks the riskiest part of the native-video plan before building on it:
//   * a DirectComposition target on the top-level window, under its child
//     windows (topmost = false), showing a composition swap chain;
//   * the WebView2 child set transparent, so the page's empty areas show the
//     swap chain and its controls draw on top;
//   * on resize, only the visual's scale transform changes (plus Commit), so
//     the picture follows the window edge without waiting for a redraw.
//
// The swap chain stays at a fixed 1280x720 and draws a moving test pattern.
// Run: cargo run --bin dcomp_spike   (BP_SPIKE_SECONDS=n to quit after n s)

#![cfg_attr(not(windows), allow(unused))]

#[cfg(not(windows))]
fn main() {
    eprintln!("dcomp_spike is Windows-only");
}

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(windows)]
mod win {
    use std::cell::RefCell;
    use std::time::{Duration, Instant};
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    use windows::core::Interface;
    use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::DirectComposition::*;
    use windows::Win32::Graphics::Dxgi::Common::*;
    use windows::Win32::Graphics::Dxgi::*;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::*;
    use windows_numerics::Matrix3x2;

    const VW: u32 = 1280;
    const VH: u32 = 720;
    const BACKDROP: u32 = 16;

    struct Comp {
        device: IDCompositionDevice,
        /// Black backdrop, scaled to fill the window (shows in the letterbox bars).
        backdrop: IDCompositionVisual,
        visual: IDCompositionVisual,
        _target: IDCompositionTarget,
    }

    thread_local! {
        // Lives on the UI thread, which is where the subclass proc runs.
        static COMP: RefCell<Option<Comp>> = const { RefCell::new(None) };
    }

    /// The swap chain and context, used only by the render thread.
    struct Renderer {
        context: ID3D11DeviceContext1,
        swap_chain: IDXGISwapChain1,
        rtv: ID3D11RenderTargetView,
    }
    // SAFETY: after creation these are only ever touched by the render thread.
    unsafe impl Send for Renderer {}

    pub fn run() {
        tauri::Builder::default()
            .setup(|app| {
                let window = WebviewWindowBuilder::new(app, "pip-spike", WebviewUrl::App("spike.html".into()))
                    .title("DComp spike")
                    .inner_size(640.0, 360.0)
                    .decorations(false)
                    .always_on_top(true)
                    .resizable(true)
                    .shadow(true)
                    .transparent(true)
                    .no_redirection_bitmap(true)
                    .build()?;
                let hwnd = HWND(window.hwnd()?.0 as _);
                let renderer = unsafe { init(hwnd) }.map_err(|e| format!("DComp setup failed: {e}"))?;
                std::thread::spawn(move || render_loop(renderer));
                if let Some(secs) = std::env::var("BP_SPIKE_SECONDS").ok().and_then(|s| s.parse().ok()) {
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

    unsafe fn init(hwnd: HWND) -> windows::core::Result<Renderer> {
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
        let context: ID3D11DeviceContext1 = context.unwrap().cast()?;
        let dxgi_device: IDXGIDevice = device.cast()?;

        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: VW,
            Height: VH,
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
        let swap_chain = factory.CreateSwapChainForComposition(&device, &desc, None)?;
        let back: ID3D11Texture2D = swap_chain.GetBuffer(0)?;
        let mut rtv = None;
        device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;

        let comp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
        // topmost = false: our visual goes under the window's children (the web view).
        let target = comp_device.CreateTargetForHwnd(hwnd, false)?;
        let root = comp_device.CreateVisual()?;

        // Backdrop: a small black surface, stretched to the window size in fit().
        let surface = comp_device.CreateSurface(BACKDROP, BACKDROP, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_IGNORE)?;
        let mut offset = Default::default();
        let tex: ID3D11Texture2D = surface.BeginDraw(None, &mut offset)?;
        let mut tex_rtv = None;
        device.CreateRenderTargetView(&tex, None, Some(&mut tex_rtv))?;
        context.ClearView(&tex_rtv.unwrap(), &[0.0, 0.0, 0.0, 1.0], None);
        surface.EndDraw()?;
        let backdrop = comp_device.CreateVisual()?;
        backdrop.SetContent(&surface)?;
        backdrop.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR)?;
        root.AddVisual(&backdrop, false, None)?;

        let visual = comp_device.CreateVisual()?;
        visual.SetContent(&swap_chain)?;
        visual.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR)?;
        // insertabove = false with no reference: on top of the backdrop.
        root.AddVisual(&visual, false, None)?;

        target.SetRoot(&root)?;
        COMP.with(|c| *c.borrow_mut() = Some(Comp { device: comp_device, backdrop, visual, _target: target }));

        let mut rect = RECT::default();
        GetClientRect(hwnd, &mut rect)?;
        fit(rect.right - rect.left, rect.bottom - rect.top);
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), 1, 0);

        Ok(Renderer { context, swap_chain, rtv: rtv.unwrap() })
    }

    /// Scale the video visual to fit the client area (letterboxed, centred).
    fn fit(w: i32, h: i32) {
        COMP.with(|c| {
            let c = c.borrow();
            let Some(c) = c.as_ref() else { return };
            let s = (w as f32 / VW as f32).min(h as f32 / VH as f32);
            let m = Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: (w as f32 - VW as f32 * s) / 2.0,
                M32: (h as f32 - VH as f32 * s) / 2.0,
            };
            let b = BACKDROP as f32;
            let full = Matrix3x2 { M11: w as f32 / b, M12: 0.0, M21: 0.0, M22: h as f32 / b, M31: 0.0, M32: 0.0 };
            unsafe {
                let _ = c.backdrop.SetTransform2(&full);
                let _ = c.visual.SetTransform2(&m);
                let _ = c.device.Commit();
            }
        });
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        match msg {
            WM_SIZE => {
                let w = (lparam.0 & 0xffff) as i32;
                let h = ((lparam.0 >> 16) & 0xffff) as i32;
                fit(w, h);
            }
            WM_SIZING => {
                keep_aspect(&mut *(lparam.0 as *mut RECT), wparam.0 as u32);
                return LRESULT(1);
            }
            WM_NCDESTROY => {
                let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), 1);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// Adjust a window rect being dragged so it keeps the video's shape.
    fn keep_aspect(r: &mut RECT, edge: u32) {
        let ratio = VW as f64 / VH as f64;
        let w = (r.right - r.left) as f64;
        let h = (r.bottom - r.top) as f64;
        match edge {
            // Top or bottom edge: height leads.
            WMSZ_TOP | WMSZ_BOTTOM => r.right = r.left + (h * ratio).round() as i32,
            // Left/right edges and corners: width leads; grow from the dragged side.
            _ => {
                let new_h = (w / ratio).round() as i32;
                if matches!(edge, WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
                    r.top = r.bottom - new_h;
                } else {
                    r.bottom = r.top + new_h;
                }
            }
        }
    }

    /// Draw a moving test pattern: checkerboard plus a sweeping bar and a border.
    fn render_loop(r: Renderer) {
        let start = Instant::now();
        let cell = 80i32;
        let mut light = Vec::new();
        for y in 0..(VH as i32 / cell + 1) {
            for x in 0..(VW as i32 / cell) {
                if (x + y) % 2 == 0 {
                    light.push(RECT { left: x * cell, top: y * cell, right: (x + 1) * cell, bottom: (y + 1) * cell });
                }
            }
        }
        let border = [
            RECT { left: 0, top: 0, right: VW as i32, bottom: 8 },
            RECT { left: 0, top: VH as i32 - 8, right: VW as i32, bottom: VH as i32 },
            RECT { left: 0, top: 0, right: 8, bottom: VH as i32 },
            RECT { left: VW as i32 - 8, top: 0, right: VW as i32, bottom: VH as i32 },
        ];
        loop {
            let t = start.elapsed().as_secs_f32();
            let x = ((t * 400.0) as i32) % (VW as i32 + 120) - 120;
            unsafe {
                r.context.ClearView(&r.rtv, &[0.10, 0.12, 0.30, 1.0], None);
                r.context.ClearView(&r.rtv, &[0.20, 0.24, 0.50, 1.0], Some(&light));
                r.context.ClearView(
                    &r.rtv,
                    &[1.0, 0.8, 0.1, 1.0],
                    Some(&[RECT { left: x.max(0), top: 0, right: (x + 120).max(0), bottom: VH as i32 }]),
                );
                r.context.ClearView(&r.rtv, &[0.1, 0.9, 0.3, 1.0], Some(&border));
                if r.swap_chain.Present(1, DXGI_PRESENT(0)).is_err() {
                    return;
                }
            }
        }
    }
}
