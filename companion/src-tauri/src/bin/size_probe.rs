// Probe: does a PiP-style window end up with the inner size we ask for? (Windows)
// Builds a window like main.rs does, calls set_size, and prints what Windows reports.
// Run: cargo run --bin size_probe

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
#[path = "../video/mod.rs"]
#[allow(dead_code)]
mod video;

#[cfg(windows)]
fn main() {
    use tauri::{LogicalSize, WebviewUrl, WebviewWindowBuilder};
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect};

    tauri::Builder::default()
        .setup(|app| {
            let w = WebviewWindowBuilder::new(app, "probe", WebviewUrl::App("spike.html".into()))
                .inner_size(480.0, 270.0)
                .min_inner_size(160.0, 90.0)
                .decorations(false)
                .always_on_top(true)
                .resizable(true)
                .shadow(true)
                .transparent(true)
                .no_redirection_bitmap(true)
                .skip_taskbar(true)
                .visible_on_all_workspaces(true)
                .focused(false)
                .visible(false)
                .build()?;
            fn report(label: &str, w: &tauri::WebviewWindow) {
                let hwnd = HWND(w.hwnd().unwrap().0 as _);
                let (mut c, mut r) = (RECT::default(), RECT::default());
                unsafe {
                    let _ = GetClientRect(hwnd, &mut c);
                    let _ = GetWindowRect(hwnd, &mut r);
                }
                println!(
                    "{label}: scale {} inner {:?} outer {:?} client {}x{} window {}x{}",
                    w.scale_factor().unwrap(),
                    w.inner_size().unwrap(),
                    w.outer_size().unwrap(),
                    c.right - c.left,
                    c.bottom - c.top,
                    r.right - r.left,
                    r.bottom - r.top
                );
            }
            report("built", &w);
            // Replay main.rs open_window on a hidden, already-loaded spare window.
            let w2 = w.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let w3 = w2.clone();
                let _ = w2.run_on_main_thread(move || {
                    let hwnd = HWND(w3.hwnd().unwrap().0 as _);
                    let _ = w3.set_title("probe title");
                    report("set_title", &w3);
                    video::win_window::set_client_size(hwnd, 480, 270);
                    report("set_client_size", &w3);
                    video::win_window::set_client_pos(hwnd, 1000, 500);
                    report("set_client_pos", &w3);
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                            hwnd,
                            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
                        );
                    }
                    report("shown", &w3);
                });
                for ms in [50u64, 500, 1500] {
                    std::thread::sleep(std::time::Duration::from_millis(ms));
                    let w4 = w2.clone();
                    let _ = w2.run_on_main_thread(move || report(&format!("+{ms}ms"), &w4));
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
                { use tauri::Manager; w2.app_handle().exit(0); }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("probe failed");
}
