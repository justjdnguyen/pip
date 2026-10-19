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
                .visible(false)
                .build()?;
            let report = |label: &str, w: &tauri::WebviewWindow| {
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
            };
            report("built", &w);
            w.set_size(LogicalSize::new(480.0, 270.0))?;
            report("set_size", &w);
            video::win_window::set_client_size(HWND(w.hwnd()?.0 as _), 480, 270);
            report("set_client_size", &w);
            video::win_window::set_client_pos(HWND(w.hwnd()?.0 as _), 1000, 500);
            let mut p = windows::Win32::Foundation::POINT::default();
            unsafe { let _ = windows::Win32::Graphics::Gdi::ClientToScreen(HWND(w.hwnd()?.0 as _), &mut p); }
            println!("client origin {},{} (want 1000,500)", p.x, p.y);
            w.show()?;
            report("shown", &w);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let w2 = w.clone();
                let _ = w.run_on_main_thread(move || {
                    report("later", &w2);
                });
                std::thread::sleep(std::time::Duration::from_millis(300));
                handle.exit(0);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("probe failed");
}
