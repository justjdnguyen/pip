//! Per-window glue on Windows: a window subclass that keeps the video's shape
//! while the user drags an edge (WM_SIZING), and scales the native video
//! layer to the window on WM_SIZE. The scale is a DirectComposition transform
//! committed in the same message, so the picture follows the window edge
//! without waiting for a new frame.
//!
//! The video layer sits under the web view (DComp target with topmost=false);
//! the window must be built with `.transparent(true)` and
//! `.no_redirection_bitmap(true)` for the page's empty areas to show it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use windows::core::Result;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_numerics::Matrix3x2;

/// Posted by the video thread when the picture size changes.
pub const WM_APP_FIT: u32 = WM_APP + 0x50;
const SUBCLASS_ID: usize = 0x4250; // "BP"
const BACKDROP: u32 = 16;

/// The DirectComposition tree for a window. Only touched on the UI thread.
struct Comp {
    device: IDCompositionDevice,
    backdrop: IDCompositionVisual,
    video: IDCompositionVisual,
    _target: IDCompositionTarget,
}

#[derive(Default)]
pub struct WindowState {
    /// Width/height the user's resizing is locked to (0 = free).
    aspect: Mutex<(f64, f64)>,
    /// Size of the picture in the video layer.
    video: Mutex<(u32, u32)>,
    comp: Mutex<Option<Comp>>,
}

// SAFETY: the COM objects inside are only used on the window's UI thread.
unsafe impl Send for WindowState {}
unsafe impl Sync for WindowState {}

static WINDOWS: Mutex<Option<HashMap<isize, Arc<WindowState>>>> = Mutex::new(None);

/// Subclass the window (once). Call on the window's UI thread.
pub fn install(hwnd: HWND) -> Arc<WindowState> {
    let mut map = WINDOWS.lock().unwrap();
    let map = map.get_or_insert_with(HashMap::new);
    if let Some(s) = map.get(&(hwnd.0 as isize)) {
        return s.clone();
    }
    let state = Arc::new(WindowState::default());
    map.insert(hwnd.0 as isize, state.clone());
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
    }
    state
}

fn get(hwnd: HWND) -> Option<Arc<WindowState>> {
    WINDOWS.lock().unwrap().as_ref()?.get(&(hwnd.0 as isize)).cloned()
}

/// Lock resizing to this shape (any thread).
pub fn set_aspect(hwnd: HWND, width: f64, height: f64) {
    if let Some(s) = get(hwnd) {
        *s.aspect.lock().unwrap() = (width, height);
    }
}

/// Make the client area exactly `width`×`height` physical pixels, keeping the
/// window's position. (Tauri's set_size is off by a title bar's height for our
/// borderless, transparent windows: 480×270 came out 480×300.)
pub fn set_client_size(hwnd: HWND, width: i32, height: i32) {
    let flags = SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE;
    unsafe {
        // A new window's client rect can still include the title bar it was
        // created with, until its frame is recalculated: refresh it first.
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags | SWP_NOSIZE | SWP_FRAMECHANGED);
        // Then size by the measured border, and once more in case it moved.
        for _ in 0..2 {
            let (mut wr, mut cr) = (RECT::default(), RECT::default());
            let _ = GetWindowRect(hwnd, &mut wr);
            let _ = GetClientRect(hwnd, &mut cr);
            if cr.right - cr.left == width && cr.bottom - cr.top == height {
                return;
            }
            let fw = (wr.right - wr.left) - (cr.right - cr.left);
            let fh = (wr.bottom - wr.top) - (cr.bottom - cr.top);
            let _ = SetWindowPos(hwnd, None, 0, 0, width + fw, height + fh, flags);
        }
    }
}

/// Put the client area's top-left corner at (x, y) in screen pixels. (Window
/// positions include invisible resize borders, which differ per side.)
pub fn set_client_pos(hwnd: HWND, x: i32, y: i32) {
    let mut wr = RECT::default();
    let mut origin = windows::Win32::Foundation::POINT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut wr);
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin);
        let _ = SetWindowPos(
            hwnd,
            None,
            x - (origin.x - wr.left),
            y - (origin.y - wr.top),
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Record the picture size and ask the UI thread to refit (any thread).
pub fn set_video_size(hwnd: HWND, width: u32, height: u32) {
    if let Some(s) = get(hwnd) {
        *s.video.lock().unwrap() = (width, height);
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_APP_FIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// Build the composition tree: a black backdrop filling the window, and the
/// video layer (`content`, a composition swap chain) on top, both under the
/// web view. Call on the UI thread.
pub unsafe fn attach_video(
    hwnd: HWND,
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    content: &windows::core::IUnknown,
) -> Result<()> {
    let state = install(hwnd);
    let dxgi: IDXGIDevice = windows::core::Interface::cast(device)?;
    let comp_device: IDCompositionDevice = DCompositionCreateDevice(&dxgi)?;
    // topmost = false: under the window's children (the web view).
    let target = comp_device.CreateTargetForHwnd(hwnd, false)?;
    let root = comp_device.CreateVisual()?;

    // Backdrop: a tiny black surface stretched to the window (letterbox bars).
    let surface = comp_device.CreateSurface(BACKDROP, BACKDROP, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_IGNORE)?;
    let mut offset = Default::default();
    let tex: ID3D11Texture2D = surface.BeginDraw(None, &mut offset)?;
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    device.CreateRenderTargetView(&tex, None, Some(&mut rtv))?;
    windows::core::Interface::cast::<windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext1>(context)?
        .ClearView(&rtv.unwrap(), &[0.0, 0.0, 0.0, 1.0], None);
    surface.EndDraw()?;
    let backdrop = comp_device.CreateVisual()?;
    backdrop.SetContent(&surface)?;
    backdrop.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR)?;
    root.AddVisual(&backdrop, false, None)?;

    let video = comp_device.CreateVisual()?;
    video.SetContent(content)?;
    video.SetBitmapInterpolationMode(DCOMPOSITION_BITMAP_INTERPOLATION_MODE_LINEAR)?;
    root.AddVisual(&video, false, None)?; // on top of the backdrop
    target.SetRoot(&root)?;

    *state.comp.lock().unwrap() = Some(Comp { device: comp_device, backdrop, video, _target: target });
    fit(hwnd, &state);
    Ok(())
}

/// Scale the video layer to fit the client area (letterboxed, centred).
fn fit(hwnd: HWND, state: &WindowState) {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    fit_to(state, r.right - r.left, r.bottom - r.top);
}

fn fit_to(state: &WindowState, w: i32, h: i32) {
    let comp = state.comp.lock().unwrap();
    let Some(c) = comp.as_ref() else { return };
    let (vw, vh) = *state.video.lock().unwrap();
    let (w, h) = (w as f32, h as f32);
    let b = BACKDROP as f32;
    let full = Matrix3x2 { M11: w / b, M12: 0.0, M21: 0.0, M22: h / b, M31: 0.0, M32: 0.0 };
    let m = if vw == 0 || vh == 0 {
        Matrix3x2 { M11: 0.0, M12: 0.0, M21: 0.0, M22: 0.0, M31: 0.0, M32: 0.0 }
    } else {
        let s = (w / vw as f32).min(h / vh as f32);
        Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: (w - vw as f32 * s) / 2.0, M32: (h - vh as f32 * s) / 2.0 }
    };
    unsafe {
        let _ = c.backdrop.SetTransform2(&full);
        let _ = c.video.SetTransform2(&m);
        let _ = c.device.Commit();
    }
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
            if let Some(s) = get(hwnd) {
                fit_to(&s, (lparam.0 & 0xffff) as i32, ((lparam.0 >> 16) & 0xffff) as i32);
            }
        }
        WM_APP_FIT => {
            if let Some(s) = get(hwnd) {
                fit(hwnd, &s);
            }
            return LRESULT(0);
        }
        WM_SIZING => {
            if let Some(s) = get(hwnd) {
                let (aw, ah) = *s.aspect.lock().unwrap();
                if aw > 0.0 && ah > 0.0 {
                    keep_aspect(hwnd, &mut *(lparam.0 as *mut RECT), wparam.0 as u32, aw / ah);
                    return LRESULT(1);
                }
            }
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            if let Some(map) = WINDOWS.lock().unwrap().as_mut() {
                map.remove(&(hwnd.0 as isize));
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Adjust a window rect being dragged so its client area keeps `ratio`.
fn keep_aspect(hwnd: HWND, r: &mut RECT, edge: u32, ratio: f64) {
    // Border sizes (window rect minus client rect); 0 for our borderless windows,
    // but keep it right if that changes.
    let (mut wr, mut cr) = (RECT::default(), RECT::default());
    unsafe {
        let _ = GetWindowRect(hwnd, &mut wr);
        let _ = GetClientRect(hwnd, &mut cr);
    }
    let fw = (wr.right - wr.left) - (cr.right - cr.left);
    let fh = (wr.bottom - wr.top) - (cr.bottom - cr.top);
    let w = (r.right - r.left - fw) as f64;
    let h = (r.bottom - r.top - fh) as f64;
    match edge {
        // Top or bottom edge: height leads.
        WMSZ_TOP | WMSZ_BOTTOM => r.right = r.left + (h * ratio).round() as i32 + fw,
        // Left/right edges and corners: width leads; grow from the dragged side.
        _ => {
            let new_h = (w / ratio).round() as i32 + fh;
            if matches!(edge, WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
                r.top = r.bottom - new_h;
            } else {
                r.bottom = r.top + new_h;
            }
        }
    }
}
