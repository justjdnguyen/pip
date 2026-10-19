// Spike: the real Windows video sink end to end, without the browser.
//
// Encodes a moving test pattern (MF H.264 encoder), then plays it at 60 fps
// through `video::create_sink` (MF decoder → video processor → swap chain under
// the transparent web view), with the aspect lock on. Use it to check the
// picture, letterboxing and resize smoothness by hand or by screenshot.
//
// Run: cargo run --bin native_video_spike
//   BP_SPIKE_SECONDS=n  quit after n s
//   BP_SPIKE_SIZE=WxH   video size (default 1280x720)

#![cfg_attr(not(windows), allow(unused))]

#[path = "../video/mod.rs"]
mod video;

#[cfg(not(windows))]
fn main() {
    eprintln!("native_video_spike is Windows-only");
}

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    use video::{win_decoder, win_test_encoder, win_window, Frame, StreamConfig};

    let (w, h) = std::env::var("BP_SPIKE_SIZE")
        .ok()
        .and_then(|s| {
            let (a, b) = s.split_once('x')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1280u32, 720u32));

    tauri::Builder::default()
        .setup(move |app| {
            let window = WebviewWindowBuilder::new(app, "pip-spike", WebviewUrl::App("spike.html".into()))
                .title("Native video spike")
                .inner_size(640.0, 640.0 * h as f64 / w as f64)
                .decorations(false)
                .always_on_top(true)
                .resizable(true)
                .shadow(true)
                .transparent(true)
                .no_redirection_bitmap(true)
                .build()?;
            let hwnd = windows::Win32::Foundation::HWND(window.hwnd()?.0 as _);
            win_window::install(hwnd);
            win_window::set_aspect(hwnd, w as f64, h as f64);
            let mut sink = video::create_sink(&window).map_err(|e| format!("sink: {e:#}"))?;

            std::thread::spawn(move || {
                win_decoder::init_thread().unwrap();
                // Checkerboard drifting right, plus a bright bar sweeping across.
                let units = win_test_encoder::encode(w, h, 60, 240, 60, |x, y, i| {
                    let bar = (i * w as usize / 240).abs_diff(x) < 24;
                    if bar {
                        235
                    } else if ((x + 4 * i) / 80 + y / 80) % 2 == 0 {
                        60
                    } else {
                        150
                    }
                })
                .unwrap();
                println!("encoded {} frames", units.len());
                sink.configure(&StreamConfig { codec: "avc1.42001f".into(), width: w, height: h }).unwrap();
                let start = Instant::now();
                let frame_time = Duration::from_micros(1_000_000 / 60);
                for n in 0.. {
                    let u = &units[n % units.len()];
                    let due = start + frame_time * n as u32;
                    if let Some(wait) = due.checked_duration_since(Instant::now()) {
                        std::thread::sleep(wait);
                    }
                    let ts = n as i64 * 1_000_000 / 60;
                    if let Err(e) = sink.submit(&Frame { data: &u.data, key: u.key, ts_us: ts, display: true }) {
                        eprintln!("submit {n}: {e:#}");
                        return;
                    }
                }
            });

            // BP_SPIKE_FULLSCREEN=ms: go fullscreen after ms, and log window events.
            window.on_window_event(|e| eprintln!("[event] {e:?}"));
            if let Some(ms) = std::env::var("BP_SPIKE_FULLSCREEN").ok().and_then(|s| s.parse().ok()) {
                let w = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(ms));
                    eprintln!("[spike] set_fullscreen(true) -> {:?}", w.set_fullscreen(true));
                    std::thread::sleep(Duration::from_millis(1500));
                    eprintln!("[spike] fullscreen={:?} visible={:?} size={:?}", w.is_fullscreen(), w.is_visible(), w.inner_size());
                });
            }
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
