//! Mac window gestures use AppKit screen points throughout, including across
//! displays with different backing scales. The geometry is tested on any OS;
//! only the small AppKit polling loop is platform-specific.
use std::time::Duration;

use super::snapping::Axis;

const RADIUS: f64 = 10.0;
const UNITS: f64 = 64.0;
fn fixed(value: f64) -> i32 { (value * UNITS).round() as i32 }
fn points(value: i32) -> f64 { value as f64 / UNITS }

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect { x: f64, y: f64, w: f64, h: f64 }
impl Rect {
    fn right(self) -> f64 { self.x + self.w }
    fn top(self) -> f64 { self.y + self.h }
    fn near(self, other: Self) -> bool {
        // AppKit rounds fractional point sizes when it applies setFrame.
        [self.x - other.x, self.y - other.y, self.w - other.w, self.h - other.h]
            .iter().all(|delta| delta.abs() < 0.75)
    }
}

#[derive(Clone, Copy, Debug)]
enum Edge { North, NorthEast, East, SouthEast, South, SouthWest, West, NorthWest }
impl Edge {
    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "North" => Self::North, "NorthEast" => Self::NorthEast,
            "East" => Self::East, "SouthEast" => Self::SouthEast,
            "South" => Self::South, "SouthWest" => Self::SouthWest,
            "West" => Self::West, "NorthWest" => Self::NorthWest,
            _ => return None,
        })
    }
    fn left(self) -> bool { matches!(self, Self::West | Self::NorthWest | Self::SouthWest) }
    fn top(self) -> bool { matches!(self, Self::North | Self::NorthWest | Self::NorthEast) }
    fn horizontal(self) -> bool { !matches!(self, Self::North | Self::South) }
    fn vertical(self) -> bool { !matches!(self, Self::West | Self::East) }
}

struct Gesture {
    original: Rect,
    cursor: (f64, f64),
    edge: Option<Edge>,
    previous: (i32, i32),
    previous_width: i32,
    x: Axis,
    y: Axis,
}
impl Gesture {
    fn new(original: Rect, cursor: (f64, f64), edge: Option<Edge>) -> Self {
        Self { original, cursor, edge, previous: (fixed(original.x), fixed(original.y)),
            previous_width: fixed(original.w), x: Axis::default(), y: Axis::default() }
    }

    fn resized(&self, edge: Edge, width: f64) -> Rect {
        let o = self.original;
        let height = width * o.h / o.w;
        Rect { x: if edge.left() { o.right() - width } else { o.x },
            y: if edge.top() { o.y } else { o.top() - height }, w: width, h: height }
    }

    fn frame(&mut self, cursor: (f64, f64), monitors: &[Rect], windows: &[Rect], now: Duration) -> Rect {
        let o = self.original;
        let (dx, dy) = (cursor.0 - self.cursor.0, cursor.1 - self.cursor.1);
        let radius = fixed(RADIUS);
        if let Some(edge) = self.edge {
            let ratio = o.w / o.h;
            let minimum = 160.0_f64.max(90.0 * ratio);
            let width = if edge.horizontal() {
                o.w + if edge.left() { -dx } else { dx }
            } else { (o.h + if edge.top() { dy } else { -dy }) * ratio }.max(minimum);
            let raw = self.resized(edge, width);
            let mut targets = Vec::new();
            for r in monitors.iter().chain(windows) {
                if edge.horizontal() && raw.y < r.top() + RADIUS && raw.top() > r.y - RADIUS {
                    for x in [r.x, r.right()] {
                        targets.push(fixed(if edge.left() { o.right() - x } else { x - o.x }));
                    }
                }
                if edge.vertical() && raw.x < r.right() + RADIUS && raw.right() > r.x - RADIUS {
                    for y in [r.y, r.top()] {
                        targets.push(fixed((if edge.top() { y - o.y } else { o.top() - y }) * ratio));
                    }
                }
            }
            targets.retain(|&width| width >= fixed(minimum));
            let raw_width = fixed(width);
            let width = self.x.apply(self.previous_width, raw_width, &targets, radius, now);
            self.previous_width = raw_width;
            return self.resized(edge, points(width));
        }

        let (x, y) = (o.x + dx, o.y + dy);
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for r in monitors {
            if y < r.top() && y + o.h > r.y { xs.extend([fixed(r.x), fixed(r.right() - o.w)]); }
            if x < r.right() && x + o.w > r.x { ys.extend([fixed(r.y), fixed(r.top() - o.h)]); }
        }
        for r in windows {
            if y < r.top() + RADIUS && y + o.h > r.y - RADIUS {
                xs.extend([fixed(r.x - o.w), fixed(r.right()), fixed(r.x), fixed(r.right() - o.w)]);
            }
            if x < r.right() + RADIUS && x + o.w > r.x - RADIUS {
                ys.extend([fixed(r.y - o.h), fixed(r.top()), fixed(r.y), fixed(r.top() - o.h)]);
            }
        }
        let raw = (fixed(x), fixed(y));
        let x = self.x.apply(self.previous.0, raw.0, &xs, radius, now);
        let y = self.y.apply(self.previous.1, raw.1, &ys, radius, now);
        self.previous = raw;
        Rect { x: points(x), y: points(y), ..o }
    }
}

#[cfg(target_os = "macos")]
#[allow(unused_imports)] // Spike binaries include this module without starting gestures.
pub use platform::start;

/// Windows in simple fullscreen, by label -> original window level. Kept
/// here so the standalone video spikes can compile the shared window module.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) static MAC_FULLSCREEN: std::sync::Mutex<std::collections::BTreeMap<String, isize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

#[cfg(target_os = "macos")]
fn is_fullscreen(window: &tauri::WebviewWindow) -> bool {
    MAC_FULLSCREEN.lock().unwrap().contains_key(window.label())
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};
    use std::time::Instant;
    use objc2::{class, msg_send};
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize};
    use tauri::{Manager, WebviewWindow};

    static NEXT: AtomicU64 = AtomicU64::new(1);
    static ACTIVE: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());

    fn rect(r: NSRect) -> Rect { Rect { x: r.origin.x, y: r.origin.y, w: r.size.width, h: r.size.height } }

    // All Objective-C operations below run on the main dispatch queue. Raw
    // NSWindow pointers are fetched afresh each tick and never outlive a call.
    unsafe fn bounds(window: &WebviewWindow) -> (Vec<Rect>, Vec<Rect>) {
        let screens: Retained<NSArray<AnyObject>> = msg_send![class!(NSScreen), screens];
        let mut monitors = Vec::new();
        for screen in screens.iter() {
            // The menu bar and Dock reduce visibleFrame. Including the full
            // frame adds a second snap target behind those reserved areas.
            monitors.push(rect(msg_send![&*screen, visibleFrame]));
        }
        let windows = window.app_handle().webview_windows().values().filter_map(|other| {
            if other.label() == window.label() || is_fullscreen(other) { return None; }
            let ptr = other.ns_window().ok()?;
            let native = &*(ptr as *const AnyObject);
            let visible: bool = msg_send![native, isVisible];
            let minimized: bool = msg_send![native, isMiniaturized];
            (visible && !minimized).then(|| rect(msg_send![native, frame]))
        }).collect();
        (monitors, windows)
    }

    struct Tracking { window: WebviewWindow, token: u64, gesture: Gesture, started: Instant }

    fn tick(mut state: Tracking) {
        if ACTIVE.lock().unwrap().get(state.window.label()) != Some(&state.token) { return; }
        let keep_tracking = unsafe {
            let buttons: usize = msg_send![class!(NSEvent), pressedMouseButtons];
            buttons & 1 != 0 && !is_fullscreen(&state.window) && state.window.ns_window().is_ok()
        };
        if !keep_tracking {
            ACTIVE.lock().unwrap().remove(state.window.label());
            return;
        }
        // SAFETY: main thread, valid NSWindow checked above; setFrame updates
        // the native video layer and web view in the same AppKit transaction.
        unsafe {
            let cursor: NSPoint = msg_send![class!(NSEvent), mouseLocation];
            let (monitors, windows) = bounds(&state.window);
            let frame = state.gesture.frame((cursor.x, cursor.y), &monitors, &windows, state.started.elapsed());
            if let Ok(ptr) = state.window.ns_window() {
                let native = &*(ptr as *const AnyObject);
                let current: NSRect = msg_send![native, frame];
                if !rect(current).near(frame) {
                    let frame = NSRect::new(NSPoint::new(frame.x, frame.y), NSSize::new(frame.w, frame.h));
                    let _: () = msg_send![native, setFrame: frame, display: true];
                }
            }
        }
        let when = dispatch2::DispatchTime::try_from(Duration::from_millis(8)).unwrap();
        let _ = dispatch2::DispatchQueue::main().after(when, move || tick(state));
    }

    pub fn start(window: &WebviewWindow, direction: Option<&str>) -> Result<(), String> {
        let edge = direction.map(|d| Edge::parse(d).ok_or_else(|| "Invalid resize direction".to_owned())).transpose()?;
        let win = window.clone();
        window.run_on_main_thread(move || {
            if is_fullscreen(&win) { return; }
            let Ok(ptr) = win.ns_window() else { return };
            unsafe {
                let native = &*(ptr as *const AnyObject);
                let original = rect(msg_send![native, frame]);
                if original.w <= 0.0 || original.h <= 0.0 { return; }
                let cursor: NSPoint = msg_send![class!(NSEvent), mouseLocation];
                let token = NEXT.fetch_add(1, Ordering::Relaxed);
                ACTIVE.lock().unwrap().insert(win.label().to_owned(), token);
                tick(Tracking { window: win, token, gesture: Gesture::new(original, (cursor.x, cursor.y), edge), started: Instant::now() });
            }
        }).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: 1080.0, h: 1920.0 };
    const WINDOW: Rect = Rect { x: 100.0, y: 100.0, w: 480.0, h: 270.0 };

    #[test]
    fn move_holds_screen_edge_then_pushes_through_without_cursor_feedback() {
        let mut g = Gesture::new(WINDOW, (0.0, 0.0), None);
        assert_eq!(g.frame((-105.0, 0.0), &[SCREEN], &[], Duration::ZERO).x, 0.0);
        assert_eq!(g.frame((-130.0, 0.0), &[SCREEN], &[], Duration::from_millis(219)).x, 0.0);
        assert_eq!(g.frame((-130.0, 0.0), &[SCREEN], &[], super::super::snapping::HOLD).x, -30.0);
    }

    #[test]
    fn other_pips_snap_and_allow_sliding_along_the_contact() {
        let neighbor = Rect { x: 700.0, y: 100.0, ..WINDOW };
        let mut g = Gesture::new(WINDOW, (0.0, 0.0), None);
        let r = g.frame((115.0, 0.0), &[], &[neighbor], Duration::ZERO);
        assert_eq!(r.right(), 700.0);
        let r = g.frame((125.0, 50.0), &[], &[neighbor], Duration::from_millis(100));
        assert_eq!((r.right(), r.y), (700.0, 150.0));
    }

    #[test]
    fn resize_spans_portrait_monitor_from_either_side_and_preserves_top() {
        for edge in [Edge::East, Edge::West] {
            let original = Rect { x: if edge.left() { 600.0 } else { 0.0 }, ..WINDOW };
            let mut g = Gesture::new(original, (0.0, 0.0), Some(edge));
            let r = g.frame((if edge.left() { -595.0 } else { 595.0 }, 0.0), &[SCREEN], &[], Duration::ZERO);
            assert_eq!((r.x, r.right(), r.top()), (0.0, 1080.0, original.top()));
            assert_eq!(r.w / r.h, original.w / original.h);
        }
    }

    #[test]
    fn corners_preserve_opposite_corner_on_negative_coordinate_displays() {
        let o = Rect { x: -1080.0, y: -600.0, ..WINDOW };
        for edge in [Edge::NorthWest, Edge::NorthEast, Edge::SouthWest, Edge::SouthEast] {
            let mut g = Gesture::new(o, (0.0, 0.0), Some(edge));
            let r = g.frame((if edge.left() { -480.0 } else { 480.0 }, 0.0), &[], &[], Duration::ZERO);
            assert_eq!(if edge.left() { r.right() } else { r.x }, if edge.left() { o.right() } else { o.x });
            assert_eq!(if edge.top() { r.y } else { r.top() }, if edge.top() { o.y } else { o.top() });
            assert_eq!((r.w, r.h), (960.0, 540.0));
        }
    }

    #[test]
    fn resize_hold_releases_without_moving_the_anchored_edge() {
        let o = Rect { x: 0.0, ..WINDOW };
        let mut g = Gesture::new(o, (0.0, 0.0), Some(Edge::East));
        let held = g.frame((595.0, 0.0), &[SCREEN], &[], Duration::ZERO);
        assert_eq!(held.w, 1080.0);
        let held = g.frame((625.0, 0.0), &[SCREEN], &[], Duration::from_millis(219));
        assert_eq!(held.w, 1080.0);
        let released = g.frame((625.0, 0.0), &[SCREEN], &[], super::super::snapping::HOLD);
        assert_eq!((released.x, released.w, released.top()), (0.0, 1105.0, o.top()));
    }

    #[test]
    fn distant_displays_and_windows_do_not_capture_a_drag() {
        let far = Rect { x: 0.0, y: 3000.0, ..SCREEN };
        let mut g = Gesture::new(WINDOW, (0.0, 0.0), None);
        let r = g.frame((-105.0, 0.0), &[far], &[far], Duration::ZERO);
        assert_eq!(r.x, -5.0);
    }

    #[test]
    fn north_and_south_resize_from_pointer_height_and_obey_minimum() {
        for edge in [Edge::North, Edge::South] {
            let mut g = Gesture::new(WINDOW, (0.0, 0.0), Some(edge));
            let r = g.frame((0.0, if edge.top() { 270.0 } else { -270.0 }), &[], &[], Duration::ZERO);
            assert_eq!((r.w, r.h), (960.0, 540.0));
            assert_eq!(if edge.top() { r.y } else { r.top() }, if edge.top() { WINDOW.y } else { WINDOW.top() });
            let r = g.frame((0.0, if edge.top() { -1000.0 } else { 1000.0 }), &[], &[], Duration::ZERO);
            assert_eq!((r.w, r.h), (160.0, 90.0));
        }
    }

    #[test]
    fn parses_every_ui_resize_direction() {
        for direction in ["North", "NorthEast", "East", "SouthEast", "South", "SouthWest", "West", "NorthWest"] {
            assert!(Edge::parse(direction).is_some());
        }
        assert!(Edge::parse("bad").is_none());
    }
}
