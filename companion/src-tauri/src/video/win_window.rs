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
use std::time::Instant;

use windows::core::{BOOL, Result};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::Graphics::DirectComposition::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
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
    drag: Mutex<Option<Drag>>,
}

struct Drag {
    cursor: POINT,
    original: RECT,
    previous: POINT,
    previous_width: i32,
    started: Instant,
    x: super::snapping::Axis,
    y: super::snapping::Axis,
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
        WM_ENTERSIZEMOVE => {
            if let Some(s) = get(hwnd) {
                let (mut cursor, mut original) = (POINT::default(), RECT::default());
                if GetCursorPos(&mut cursor).is_ok() && GetWindowRect(hwnd, &mut original).is_ok() {
                    *s.drag.lock().unwrap() = Some(Drag {
                        cursor, original,
                        previous: POINT { x: original.left, y: original.top },
                        previous_width: original.right - original.left,
                        started: Instant::now(),
                        x: Default::default(), y: Default::default(),
                    });
                }
            }
        }
        WM_MOVING => {
            if let Some(s) = get(hwnd) {
                if let Some(drag) = s.drag.lock().unwrap().as_mut() {
                    snap_move(hwnd, &mut *(lparam.0 as *mut RECT), drag);
                    return LRESULT(1);
                }
            }
        }
        WM_EXITSIZEMOVE => {
            if let Some(s) = get(hwnd) {
                *s.drag.lock().unwrap() = None;
            }
        }
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
                    let r = &mut *(lparam.0 as *mut RECT);
                    let edge = wparam.0 as u32;
                    if let Some(drag) = s.drag.lock().unwrap().as_mut() {
                        snap_resize(hwnd, r, edge, aw / ah, drag);
                    } else {
                        keep_aspect(hwnd, r, edge, aw / ah);
                    }
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

/// Visible bounds, excluding the invisible resize frame.
unsafe fn client_bounds(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    GetClientRect(hwnd, &mut rect).ok()?;
    if !ClientToScreen(hwnd, &mut origin).as_bool() {
        return None;
    }
    Some(RECT { left: origin.x, top: origin.y, right: origin.x + rect.right, bottom: origin.y + rect.bottom })
}

unsafe extern "system" fn collect_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        let bounds = &mut *(data.0 as *mut Vec<RECT>);
        bounds.push(info.rcMonitor);
        bounds.push(info.rcWork); // Also stop above the taskbar.
    }
    BOOL(1)
}

unsafe fn snap_bounds(hwnd: HWND) -> (Vec<RECT>, Vec<RECT>) {
    let mut monitors = Vec::<RECT>::new();
    let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut monitors as *mut _ as isize));
    // Copy handles before Win32 calls that could reenter a window procedure.
    let others: Vec<isize> = WINDOWS.lock().unwrap().as_ref()
        .map(|m| m.keys().copied().filter(|&key| key != hwnd.0 as isize).collect())
        .unwrap_or_default();
    let windows = others.into_iter().filter_map(|key| {
        let other = HWND(key as *mut _);
        if !IsWindowVisible(other).as_bool() || IsIconic(other).as_bool() {
            return None;
        }
        client_bounds(other)
    }).collect();
    (monitors, windows)
}

unsafe fn snap_move(hwnd: HWND, proposed: &mut RECT, drag: &mut Drag) {
    let mut cursor = POINT::default();
    let mut outer = RECT::default();
    if GetCursorPos(&mut cursor).is_err() || GetWindowRect(hwnd, &mut outer).is_err() {
        return;
    }
    let Some(client) = client_bounds(hwnd) else { return };
    // Derive raw motion from the cursor, never from previously snapped rects.
    // Preserve the OS's proposed size in case a monitor DPI transition resized it.
    let (w, h) = (proposed.right - proposed.left, proposed.bottom - proposed.top);
    let (x, y) = (drag.original.left + cursor.x - drag.cursor.x, drag.original.top + cursor.y - drag.cursor.y);
    let (left, top) = (client.left - outer.left, client.top - outer.top);
    let (right, bottom) = (outer.right - client.right, outer.bottom - client.bottom);
    let visible = RECT { left: x + left, top: y + top, right: x + w - right, bottom: y + h - bottom };
    let radius = ((GetDpiForWindow(hwnd).max(96) as f64 / 96.0) * 10.0).round() as i32;
    let (monitors, windows) = snap_bounds(hwnd);
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for r in monitors {
        if visible.top < r.bottom && visible.bottom > r.top {
            xs.extend([r.left - left, r.right - w + right]);
        }
        if visible.left < r.right && visible.right > r.left {
            ys.extend([r.top - top, r.bottom - h + bottom]);
        }
    }
    for r in windows {
        if visible.top < r.bottom + radius && visible.bottom > r.top - radius {
            xs.extend([r.left - w + right, r.right - left, r.left - left, r.right - w + right]);
        }
        if visible.left < r.right + radius && visible.right > r.left - radius {
            ys.extend([r.top - h + bottom, r.bottom - top, r.top - top, r.bottom - h + bottom]);
        }
    }
    let now = drag.started.elapsed();
    let snapped_x = drag.x.apply(drag.previous.x, x, &xs, radius, now);
    let snapped_y = drag.y.apply(drag.previous.y, y, &ys, radius, now);
    drag.previous = POINT { x, y };
    *proposed = RECT { left: snapped_x, top: snapped_y, right: snapped_x + w, bottom: snapped_y + h };
}

fn grows_left(edge: u32) -> bool {
    matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT)
}

fn grows_top(edge: u32) -> bool {
    matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT)
}

/// Resize while preserving the opposite edge/corner. Width includes the frame;
/// the aspect ratio applies to the visible client area only.
fn resize_to_width(r: &mut RECT, edge: u32, width: i32, ratio: f64, fw: i32, fh: i32) {
    if grows_left(edge) {
        r.left = r.right - width;
    } else {
        r.right = r.left + width;
    }
    let height = ((width - fw) as f64 / ratio).round() as i32 + fh;
    if grows_top(edge) {
        r.top = r.bottom - height;
    } else {
        r.bottom = r.top + height;
    }
}

unsafe fn snap_resize(hwnd: HWND, proposed: &mut RECT, edge: u32, ratio: f64, drag: &mut Drag) {
    let mut cursor = POINT::default();
    let mut outer = RECT::default();
    if GetCursorPos(&mut cursor).is_err() || GetWindowRect(hwnd, &mut outer).is_err() {
        return;
    }
    let Some(client) = client_bounds(hwnd) else { return };
    let (left, top) = (client.left - outer.left, client.top - outer.top);
    let (right, bottom) = (outer.right - client.right, outer.bottom - client.bottom);
    let (fw, fh) = (left + right, top + bottom);
    let scale = GetDpiForWindow(hwnd).max(96) as f64 / 96.0;
    let radius = (10.0 * scale).round() as i32;
    let min_width = (160.0_f64.max(90.0 * ratio) * scale).ceil() as i32 + fw;
    let mut raw = drag.original;
    let dx = cursor.x - drag.cursor.x;
    let dy = cursor.y - drag.cursor.y;
    let original_width = raw.right - raw.left;
    let width = if matches!(edge, WMSZ_TOP | WMSZ_BOTTOM) {
        let height = raw.bottom - raw.top + if grows_top(edge) { -dy } else { dy };
        ((height - fh) as f64 * ratio).round() as i32 + fw
    } else {
        original_width + if grows_left(edge) { -dx } else { dx }
    }.max(min_width);
    resize_to_width(&mut raw, edge, width, ratio, fw, fh);
    let (monitors, windows) = snap_bounds(hwnd);
    let mut targets = Vec::new();
    let horizontal = !matches!(edge, WMSZ_TOP | WMSZ_BOTTOM);
    let vertical = !matches!(edge, WMSZ_LEFT | WMSZ_RIGHT);
    for r in monitors.into_iter().chain(windows) {
        if horizontal && raw.top + top < r.bottom + radius && raw.bottom - bottom > r.top - radius {
            for x in [r.left, r.right] {
                targets.push(if grows_left(edge) { drag.original.right - x + left } else { x - drag.original.left + right });
            }
        }
        if vertical && raw.left + left < r.right + radius && raw.right - right > r.left - radius {
            for y in [r.top, r.bottom] {
                let height = if grows_top(edge) { drag.original.bottom - y + top } else { y - drag.original.top + bottom };
                targets.push(((height - fh) as f64 * ratio).round() as i32 + fw);
            }
        }
    }
    targets.retain(|&w| w >= min_width);
    let snapped = drag.x.apply(drag.previous_width, width, &targets, radius, drag.started.elapsed());
    drag.previous_width = width;
    resize_to_width(&mut raw, edge, snapped, ratio, fw, fh);
    *proposed = raw;
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
    let width = if matches!(edge, WMSZ_TOP | WMSZ_BOTTOM) {
        ((r.bottom - r.top - fh) as f64 * ratio).round() as i32 + fw
    } else {
        r.right - r.left
    };
    resize_to_width(r, edge, width, ratio, fw, fh);
}

#[cfg(test)]
mod resize_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn portrait_monitor_width_snaps_from_either_side_with_invisible_borders() {
        // A 1080px portrait display, with 8px invisible frame on each side.
        for edge in [WMSZ_RIGHT, WMSZ_LEFT] {
            let mut r = if edge == WMSZ_RIGHT {
                RECT { left: -8, top: 200, right: 488, bottom: 470 }
            } else {
                RECT { left: 592, top: 200, right: 1088, bottom: 470 }
            };
            let mut snap = super::super::snapping::Axis::default();
            let width = snap.apply(1000, 1092, &[1096], 10, Duration::ZERO);
            resize_to_width(&mut r, edge, width, 16.0 / 9.0, 16, 0);
            assert_eq!((r.left + 8, r.right - 8), (0, 1080));
            assert_eq!(r.top, 200);
            assert_eq!(r.bottom - r.top, 608);
            // Breaking through still keeps the opposite edge anchored.
            let width = snap.apply(1092, 1120, &[1096], 10, super::super::snapping::HOLD);
            resize_to_width(&mut r, edge, width, 16.0 / 9.0, 16, 0);
            assert_eq!(if edge == WMSZ_RIGHT { r.left } else { r.right }, if edge == WMSZ_RIGHT { -8 } else { 1088 });
            assert_eq!(r.right - r.left, 1120);
        }
    }

    #[test]
    fn all_corners_preserve_the_opposite_corner_on_negative_coordinate_monitors() {
        let original = RECT { left: -1080, top: -600, right: -600, bottom: -330 };
        for edge in [WMSZ_TOPLEFT, WMSZ_TOPRIGHT, WMSZ_BOTTOMLEFT, WMSZ_BOTTOMRIGHT] {
            let mut r = original;
            resize_to_width(&mut r, edge, 960, 16.0 / 9.0, 0, 0);
            assert_eq!(if grows_left(edge) { r.right } else { r.left }, if grows_left(edge) { original.right } else { original.left });
            assert_eq!(if grows_top(edge) { r.bottom } else { r.top }, if grows_top(edge) { original.bottom } else { original.top });
            assert_eq!((r.right - r.left, r.bottom - r.top), (960, 540));
        }
    }

    #[test]
    fn vertical_edges_keep_the_opposite_edge_anchored() {
        for edge in [WMSZ_TOP, WMSZ_BOTTOM] {
            let mut r = RECT { left: 0, top: 100, right: 480, bottom: 370 };
            resize_to_width(&mut r, edge, 960, 16.0 / 9.0, 0, 0);
            assert_eq!(if edge == WMSZ_TOP { r.bottom } else { r.top }, if edge == WMSZ_TOP { 370 } else { 100 });
            assert_eq!(r.left, 0);
            assert_eq!(r.bottom - r.top, 540);
        }
    }
}
