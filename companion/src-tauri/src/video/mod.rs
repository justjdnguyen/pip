//! Native video: decode H.264 and show it in the PiP window, under a
//! transparent web page that only holds the controls (see HANDOFF.md).
//!
//! The shared core (message routing, pacing, keyframe and latency reports)
//! drives a per-platform `VideoSink`; only decoding and showing differ.

#![allow(dead_code)]

pub mod annexb;
pub mod core;
#[cfg(any(windows, target_os = "macos", test))]
mod snapping;
#[cfg(any(target_os = "macos", test))]
pub mod mac_window;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(windows)]
pub mod win_decoder;
#[cfg(windows)]
pub mod win_sink;
#[cfg(windows)]
pub mod win_test_encoder;
#[cfg(windows)]
pub mod win_window;

pub struct StreamConfig {
    pub codec: String,
    pub width: u32,
    pub height: u32,
}

pub struct Frame<'a> {
    /// One access unit, Annex B (start codes); keyframes carry SPS/PPS.
    pub data: &'a [u8],
    pub key: bool,
    pub ts_us: i64,
    /// false = decode only, don't show (catching up after a stall).
    pub display: bool,
}

/// Runs on the window's video thread (hence `Send`).
pub trait VideoSink: Send {
    /// New stream or new resolution.
    fn configure(&mut self, cfg: &StreamConfig) -> anyhow::Result<()>;
    /// Called by the pacing core at the moment the frame should appear.
    fn submit(&mut self, frame: &Frame) -> anyhow::Result<()>;
    /// Drop everything queued (seek / stream restart).
    fn flush(&mut self);
}

/// A `submit` error meaning "restart me at a keyframe" rather than "this is
/// broken": the core asks the tab for a keyframe but doesn't count it towards
/// giving up on native video. Check with `err.downcast_ref::<NeedKeyframe>()`.
#[derive(Debug)]
pub struct NeedKeyframe;

impl std::fmt::Display for NeedKeyframe {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("waiting for a keyframe")
    }
}
impl std::error::Error for NeedKeyframe {}

/// Set up native video for a PiP window. Call on the main (UI) thread.
#[cfg(windows)]
pub fn create_sink(window: &tauri::WebviewWindow) -> anyhow::Result<Box<dyn VideoSink>> {
    let hwnd = ::windows::Win32::Foundation::HWND(window.hwnd()?.0 as _);
    Ok(Box::new(win_sink::WinSink::new(hwnd)?))
}

/// Set up native video for a PiP window. Call on the main thread.
#[cfg(target_os = "macos")]
pub fn create_sink(window: &tauri::WebviewWindow) -> anyhow::Result<Box<dyn VideoSink>> {
    Ok(Box::new(macos::MacSink::new(window)?))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn create_sink(_window: &tauri::WebviewWindow) -> anyhow::Result<Box<dyn VideoSink>> {
    anyhow::bail!("native video isn't supported on this platform")
}
