// Better PiP companion app.
//
// The browser starts this program through Native Messaging when the extension
// connects, and talks to it over stdin/stdout (each message is a 4-byte
// length followed by JSON). For each PiP the extension asks for, we show a
// borderless, always-on-top window and forward that PiP's messages (video
// data, playback state, captions) to it. Messages from windows (play, seek…)
// go back out on stdout. When the browser disconnects, stdin closes and we exit.
//
// Creating a window and its web view takes macOS a second or two, so we keep
// one hidden "spare" window loaded and ready, and hand it out on request.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::ipc::Channel;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};

#[derive(Default)]
struct Sessions {
    /// Live connection to each window's page, by window label.
    channels: HashMap<String, Channel<Value>>,
    /// Messages for a window whose page hasn't connected yet, by window label.
    pending: HashMap<String, Vec<Value>>,
    /// PiP id (from the extension) -> window label, and back.
    label_of: HashMap<String, String>,
    id_of: HashMap<String, String>,
    /// A hidden, already-loaded window ready to be handed out.
    spare: Option<String>,
    next_window: u64,
}

struct AppState(Mutex<Sessions>);

static STDOUT_LOCK: Mutex<()> = Mutex::new(());

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Set BP_DEBUG_LOG=/path/to/file to log message traffic (video chunks are skipped).
fn debug_log(direction: &str, msg: &Value) {
    let Some(path) = std::env::var_os("BP_DEBUG_LOG") else { return };
    if msg["type"] == "chunk" {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{:.0} {direction} {msg}", now_ms());
    }
}

/// Send one message to the extension.
fn write_native(msg: &Value) {
    debug_log(">", msg);
    let Ok(bytes) = serde_json::to_vec(msg) else { return };
    let _guard = STDOUT_LOCK.lock().unwrap();
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&(bytes.len() as u32).to_ne_bytes());
    let _ = out.write_all(&bytes);
    let _ = out.flush();
}

/// Called by a window's page once it's ready to receive messages.
#[tauri::command]
fn attach(window: WebviewWindow, channel: Channel<Value>, state: tauri::State<AppState>) {
    let label = window.label().to_owned();
    let mut s = state.0.lock().unwrap();
    for msg in s.pending.remove(&label).unwrap_or_default() {
        let _ = channel.send(msg);
    }
    s.channels.insert(label, channel);
}

/// Called by a window's page to message the extension. We add the PiP id.
#[tauri::command]
fn send(window: WebviewWindow, mut msg: Value, state: tauri::State<AppState>) {
    let id = state.0.lock().unwrap().id_of.get(window.label()).cloned();
    match id {
        Some(id) => msg["id"] = json!(id),
        // A spare window that hasn't been handed out yet: only log it.
        None => return debug_log("spare", &msg),
    }
    write_native(&msg);
}

/// Lock the window to the video's shape while the user resizes it.
#[tauri::command]
fn set_aspect(window: WebviewWindow, width: f64, height: f64) {
    set_aspect_ratio(&window, width, height);
}

#[cfg(target_os = "macos")]
fn set_aspect_ratio(window: &WebviewWindow, width: f64, height: f64) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSSize;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let win = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(ptr) = win.ns_window() {
            // SAFETY: ns_window() is the live NSWindow for this window, and we're on the main thread.
            unsafe {
                let ns_window = &*(ptr as *const AnyObject);
                let _: () = msg_send![ns_window, setContentAspectRatio: NSSize::new(width, height)];
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn set_aspect_ratio(_window: &WebviewWindow, _width: f64, _height: f64) {
    // TODO(windows): handle WM_SIZING to keep the aspect ratio while resizing.
}

/// Let the window float over other apps' fullscreen Spaces too.
#[cfg(target_os = "macos")]
fn float_over_fullscreen_apps(window: &WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    if let Ok(ptr) = window.ns_window() {
        // SAFETY: as above; called on the main thread.
        unsafe {
            let ns_window = &*(ptr as *const AnyObject);
            let current: usize = msg_send![ns_window, collectionBehavior];
            let _: () = msg_send![ns_window, setCollectionBehavior: current | CAN_JOIN_ALL_SPACES | FULL_SCREEN_AUXILIARY];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn float_over_fullscreen_apps(_window: &WebviewWindow) {}

/// Show a window without taking keyboard focus from the browser.
#[cfg(target_os = "macos")]
fn show_without_focus(window: &WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    match window.ns_window() {
        // SAFETY: as above; called on the main thread.
        Ok(ptr) => unsafe {
            let ns_window = &*(ptr as *const AnyObject);
            let _: () = msg_send![ns_window, orderFrontRegardless];
        },
        Err(_) => {
            let _ = window.show();
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn show_without_focus(window: &WebviewWindow) {
    let _ = window.show();
}

/// Create a (hidden) PiP window. Must run on the main thread.
fn create_window(app: &AppHandle) -> Option<WebviewWindow> {
    let label = {
        let state = app.state::<AppState>();
        let mut s = state.0.lock().unwrap();
        s.next_window += 1;
        format!("pip-{}", s.next_window)
    };
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("Picture-in-Picture")
        .inner_size(480.0, 270.0)
        .min_inner_size(160.0, 90.0)
        .decorations(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .resizable(true)
        .shadow(true)
        // Black behind the video, so if the page lags a frame behind the
        // window edge while resizing, the gap blends in instead of flashing white.
        .background_color(tauri::window::Color(0, 0, 0, 255))
        .visible(false)
        .focused(false)
        .on_page_load(|webview, payload| {
            debug_log("page", &json!({ "label": webview.label(), "event": format!("{:?}", payload.event()) }));
            // BP_DEBUG_SHOW_CONTROLS=1 pins the controls on, for screenshots.
            if std::env::var_os("BP_DEBUG_SHOW_CONTROLS").is_some() {
                let _ = webview.eval("setInterval(() => document.querySelector('.pip')?.classList.add('active'), 200)");
            }
        })
        .build()
        .ok()?;
    float_over_fullscreen_apps(&window);

    let handle = app.clone();
    let closed_label = label.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            let state = handle.state::<AppState>();
            let mut s = state.0.lock().unwrap();
            s.channels.remove(&closed_label);
            s.pending.remove(&closed_label);
            if s.spare.as_deref() == Some(closed_label.as_str()) {
                s.spare = None;
            }
            if let Some(id) = s.id_of.remove(&closed_label) {
                s.label_of.remove(&id);
                write_native(&json!({ "type": "closed", "id": id }));
            }
        }
    });
    Some(window)
}

/// Make sure a hidden spare window exists (created in the background).
fn ensure_spare(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<AppState>();
        if state.0.lock().unwrap().spare.is_some() {
            return;
        }
        if let Some(w) = create_window(&handle) {
            state.0.lock().unwrap().spare = Some(w.label().to_owned());
        }
    });
}

/// Hand out a window for a new PiP (the spare if there is one).
fn open_window(app: &AppHandle, msg: &Value) {
    let Some(id) = msg["id"].as_str().map(str::to_owned) else { return };
    let width = msg["width"].as_f64().unwrap_or(480.0).max(160.0);
    let height = msg["height"].as_f64().unwrap_or(270.0).max(90.0);
    let title = msg["title"].as_str().unwrap_or("Picture-in-Picture").to_owned();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<AppState>();
        if state.0.lock().unwrap().label_of.contains_key(&id) {
            return;
        }
        let spare = state.0.lock().unwrap().spare.take();
        let window = spare
            .and_then(|label| handle.get_webview_window(&label))
            .or_else(|| create_window(&handle));
        let Some(window) = window else {
            write_native(&json!({ "type": "closed", "id": id, "error": "couldn't create window" }));
            return;
        };
        let label = window.label().to_owned();
        {
            let mut s = state.0.lock().unwrap();
            s.label_of.insert(id.clone(), label.clone());
            s.id_of.insert(label.clone(), id.clone());
        }

        let _ = window.set_title(&title);
        let _ = window.set_size(LogicalSize::new(width, height));
        set_aspect_ratio(&window, width, height);
        // Bottom-right corner of the main screen, like Firefox.
        if let Ok(Some(monitor)) = window.primary_monitor() {
            let scale = monitor.scale_factor();
            let area = monitor.work_area();
            let pos = area.position.to_logical::<f64>(scale);
            let size = area.size.to_logical::<f64>(scale);
            let margin = 24.0;
            let _ = window.set_position(LogicalPosition::new(
                pos.x + size.width - width - margin,
                pos.y + size.height - height - margin,
            ));
        }
        show_without_focus(&window);
        deliver_to_label(&handle, &label, json!({ "type": "assigned" }));

        // Get the next spare ready.
        ensure_spare(&handle);
    });
}

fn close_window(app: &AppHandle, id: &str) {
    let handle = app.clone();
    let id = id.to_owned();
    let _ = app.run_on_main_thread(move || {
        let label = handle.state::<AppState>().0.lock().unwrap().label_of.get(&id).cloned();
        if let Some(w) = label.and_then(|l| handle.get_webview_window(&l)) {
            let _ = w.destroy();
        }
    });
}

fn deliver_to_label(app: &AppHandle, label: &str, msg: Value) {
    let state = app.state::<AppState>();
    let mut s = state.0.lock().unwrap();
    if let Some(channel) = s.channels.get(label) {
        let _ = channel.send(msg);
    } else {
        let queue = s.pending.entry(label.to_owned()).or_default();
        // Don't hoard video while a window is still loading; the rest matters.
        if msg["type"] != "chunk" {
            queue.push(msg);
        }
    }
}

/// Hand a message from the extension to the window it belongs to.
fn deliver(app: &AppHandle, id: &str, msg: Value) {
    let label = app.state::<AppState>().0.lock().unwrap().label_of.get(id).cloned();
    if let Some(label) = label {
        deliver_to_label(app, &label, msg);
    }
}

fn handle_message(app: &AppHandle, msg: Value) {
    let id = msg["id"].as_str().unwrap_or_default().to_owned();
    match msg["type"].as_str().unwrap_or_default() {
        "open" => open_window(app, &msg),
        "close" => close_window(app, &id),
        "ping" => write_native(&json!({ "type": "hello", "version": env!("CARGO_PKG_VERSION") })),
        _ if !id.is_empty() => deliver(app, &id, msg),
        _ => {}
    }
}

/// Read messages from the browser until it disconnects, then quit.
fn read_stdin(app: AppHandle) {
    let mut stdin = std::io::stdin().lock();
    loop {
        let mut len = [0u8; 4];
        if stdin.read_exact(&mut len).is_err() {
            break;
        }
        let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
        if stdin.read_exact(&mut buf).is_err() {
            break;
        }
        if let Ok(mut msg) = serde_json::from_slice::<Value>(&buf) {
            // When this message reached the app (for latency stats).
            if msg["type"] == "chunk" {
                msg["rx"] = json!(now_ms());
            }
            debug_log("<", &msg);
            handle_message(&app, msg);
        }
    }
    app.exit(0);
}

fn main() {
    let app = tauri::Builder::default()
        .manage(AppState(Mutex::new(Sessions::default())))
        .invoke_handler(tauri::generate_handler![attach, send, set_aspect])
        .setup(|app| {
            // No Dock icon or menu bar: this app only ever shows PiP windows.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            std::thread::spawn(move || read_stdin(handle));
            ensure_spare(app.handle());
            write_native(&json!({ "type": "hello", "version": env!("CARGO_PKG_VERSION") }));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start Better PiP");

    app.run(|_, event| {
        // Stay running with no windows open (until the browser disconnects).
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
