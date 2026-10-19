//! Native video: decode H.264 and show it in the PiP window, under a
//! transparent web page that only holds the controls (see HANDOFF.md).
//!
//! The shared core (message routing, pacing, keyframe and latency reports)
//! drives a per-platform `VideoSink`; only decoding and showing differ.

#![allow(dead_code)]

pub mod annexb;
#[cfg(windows)]
pub mod win_decoder;

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

pub trait VideoSink {
    /// New stream or new resolution.
    fn configure(&mut self, cfg: &StreamConfig) -> anyhow::Result<()>;
    /// Called by the pacing core at the moment the frame should appear.
    fn submit(&mut self, frame: &Frame) -> anyhow::Result<()>;
    /// Drop everything queued (seek / stream restart).
    fn flush(&mut self);
}
