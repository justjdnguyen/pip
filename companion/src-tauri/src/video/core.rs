//! The shared native-video core: one thread per PiP window that receives the
//! tab's compressed frames, paces them, and hands them to the platform sink at
//! the moment they should appear. A port of the pacing in `ui/decoder-worker.js`:
//!
//! * Each frame is due a short, adaptive delay (15–150 ms) after the moment
//!   its capture timestamp maps to. The clock follows the fastest frame, so a
//!   frame that arrived late doesn't hold the rest back.
//! * When several frames are due at once, all are decoded (H.264 needs them)
//!   but only the newest is shown.
//! * Asks the tab for a keyframe when it starts, after errors, and when badly behind.
//! * Reports how far the picture lags the tab (for the tab's lip-sync audio delay).

use std::collections::VecDeque;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::{Frame, StreamConfig, VideoSink};

const MIN_DELAY_MS: f64 = 15.0;
const MAX_DELAY_MS: f64 = 150.0;
/// From handing a frame to the sink to it being on screen (about one refresh).
const PRESENT_MS: f64 = 20.0;
/// More than this many frames waiting = badly behind: skip to the next keyframe.
const MAX_QUEUED: usize = 30;
/// This many sink errors in a row with no frame shown in between = the sink
/// can't play this stream: fall back to the web path. (`NeedKeyframe` errors
/// don't count; they're a normal restart.)
const MAX_SINK_ERRORS: u32 = 3;

pub enum Input {
    Config(StreamConfig),
    Chunk {
        data: Vec<u8>,
        key: bool,
        ts_us: i64,
        /// Wall-clock ms when the tab showed this frame (0 = unknown).
        cap: f64,
    },
}

/// What the core tells the rest of the app.
pub trait Events: Send + 'static {
    fn keyframe(&self);
    fn latency(&self, ms: u32);
    fn first_frame(&self);
    /// The sink can't play this stream; switch the window to the web path.
    fn failed(&self, message: String);
}

/// Handle to a window's video thread. Dropping it stops the thread.
pub struct NativeVideo {
    tx: mpsc::Sender<Input>,
}

impl NativeVideo {
    pub fn start(sink: Box<dyn VideoSink>, events: Box<dyn Events>) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("bp-video".into())
            .spawn(move || Pacer::new(sink, events).run(rx))
            .expect("spawn video thread");
        NativeVideo { tx }
    }

    pub fn send(&self, input: Input) {
        let _ = self.tx.send(input);
    }
}

struct Queued {
    data: Vec<u8>,
    key: bool,
    ts_us: i64,
    cap: f64,
}

struct Pacer {
    sink: Box<dyn VideoSink>,
    events: Box<dyn Events>,
    epoch: Instant,
    configured: bool,
    failed: bool,
    waiting_for_key: bool,
    /// Sink errors since a frame was last shown (see MAX_SINK_ERRORS).
    sink_errors: u32,
    /// Maps capture time to local time: (ts_us, local ms).
    clock: Option<(i64, f64)>,
    delay_ms: f64,
    queue: VecDeque<Queued>,
    late: u32,
    first_frame: bool,
    latency: Option<f64>,
    latency_samples: u32,
    latency_sent: f64,
    last_adjust: f64,
}

fn wall_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

impl Pacer {
    fn new(sink: Box<dyn VideoSink>, events: Box<dyn Events>) -> Self {
        Pacer {
            sink,
            events,
            epoch: Instant::now(),
            configured: false,
            failed: false,
            waiting_for_key: true,
            sink_errors: 0,
            clock: None,
            delay_ms: 40.0,
            queue: VecDeque::new(),
            late: 0,
            first_frame: true,
            latency: None,
            latency_samples: 0,
            latency_sent: 0.0,
            last_adjust: 0.0,
        }
    }

    fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64() * 1000.0
    }

    fn due(&self, ts_us: i64, delay: f64) -> f64 {
        let (cts, at) = self.clock.unwrap();
        at + (ts_us - cts) as f64 / 1000.0 + delay
    }

    fn run(mut self, rx: mpsc::Receiver<Input>) {
        loop {
            let timeout = match self.queue.front() {
                Some(f) => Duration::from_secs_f64(((self.due(f.ts_us, self.delay_ms) - self.now()) / 1000.0).max(0.0)),
                None => Duration::from_millis(500),
            };
            match rx.recv_timeout(timeout) {
                Ok(input) => {
                    self.on_input(input);
                    // Take everything that's waiting, so a burst is paced as one.
                    while let Ok(input) = rx.try_recv() {
                        self.on_input(input);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.show_due();
            self.adjust_delay();
        }
    }

    fn on_input(&mut self, input: Input) {
        match input {
            Input::Config(cfg) => {
                self.queue.clear();
                self.sink.flush();
                match self.sink.configure(&cfg) {
                    Ok(()) => self.configured = true,
                    Err(e) => return self.fail(format!("configure {}x{} {}: {e:#}", cfg.width, cfg.height, cfg.codec)),
                }
                self.restart();
            }
            Input::Chunk { data, key, ts_us, cap } => {
                if !self.configured || self.failed {
                    return;
                }
                if self.waiting_for_key && !key {
                    return;
                }
                if self.queue.len() > MAX_QUEUED && !key {
                    // Badly behind: drop to the next keyframe.
                    self.queue.clear();
                    return self.restart();
                }
                self.waiting_for_key = false;
                let t = self.now();
                let lead = self.clock.map(|_| self.due(ts_us, 0.0) - t);
                match lead {
                    // First frame, or the timeline jumped (seek, new video): resync.
                    None => self.clock = Some((ts_us, t)),
                    Some(l) if l.abs() > 1000.0 => self.clock = Some((ts_us, t)),
                    // This frame came quicker than the one we synced to, so that one
                    // was held up. Sync to this one.
                    Some(l) if l > 0.0 => self.clock.as_mut().unwrap().1 -= l,
                    _ => {}
                }
                if self.due(ts_us, self.delay_ms) < t {
                    self.late += 1;
                }
                self.queue.push_back(Queued { data, key, ts_us, cap });
            }
        }
    }

    /// Wait for a keyframe and ask the tab for one.
    fn restart(&mut self) {
        self.waiting_for_key = true;
        self.events.keyframe();
    }

    fn fail(&mut self, message: String) {
        self.failed = true;
        self.queue.clear();
        self.events.failed(message);
    }

    fn show_due(&mut self) {
        let t = self.now();
        while self.queue.front().is_some_and(|f| self.due(f.ts_us, self.delay_ms) <= t + 1.0) {
            // Show only the last of the frames that are due.
            let f = self.queue.pop_front().unwrap();
            let display = !self.queue.front().is_some_and(|n| self.due(n.ts_us, self.delay_ms) <= t + 1.0);
            let frame = Frame { data: &f.data, key: f.key, ts_us: f.ts_us, display };
            if let Err(e) = self.sink.submit(&frame) {
                // Decoding broke (bad data, device lost…): start over from a keyframe.
                eprintln!("[native video] {e:#}");
                self.queue.clear();
                self.sink.flush();
                if e.downcast_ref::<super::NeedKeyframe>().is_none() {
                    self.sink_errors += 1;
                    if self.sink_errors >= MAX_SINK_ERRORS {
                        return self.fail(format!("{MAX_SINK_ERRORS} sink errors in a row, last: {e:#}"));
                    }
                }
                return self.restart();
            }
            if display {
                self.sink_errors = 0;
                self.track_latency(f.cap);
                if self.first_frame {
                    self.first_frame = false;
                    self.events.first_frame();
                }
            }
        }
    }

    fn track_latency(&mut self, cap: f64) {
        if cap <= 0.0 {
            return;
        }
        let ms = wall_ms() - cap + PRESENT_MS;
        // Skip the first half second: frames are slow while everything spins up.
        self.latency_samples += 1;
        if self.latency_samples < 30 {
            return;
        }
        // Smooth it so the sound delay doesn't jitter.
        let l = self.latency.map_or(ms, |l| l * 0.95 + ms * 0.05);
        self.latency = Some(l);
        let t = self.now();
        if self.latency_samples > 45 && t - self.latency_sent > 500.0 {
            self.latency_sent = t;
            self.events.latency(l.round() as u32);
        }
    }

    /// Every 2 s: more late frames → longer delay; otherwise slowly shrink it.
    fn adjust_delay(&mut self) {
        let t = self.now();
        if t - self.last_adjust < 2000.0 {
            return;
        }
        self.last_adjust = t;
        if self.late > 2 {
            self.delay_ms = (self.delay_ms + 10.0).min(MAX_DELAY_MS);
        } else if self.late == 0 {
            self.delay_ms = (self.delay_ms - 3.0).max(MIN_DELAY_MS);
        }
        self.late = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);
    struct Sink(Arc<Log>);
    impl VideoSink for Sink {
        fn configure(&mut self, _: &StreamConfig) -> anyhow::Result<()> {
            Ok(())
        }
        fn submit(&mut self, f: &Frame) -> anyhow::Result<()> {
            self.0 .0.lock().unwrap().push(format!("{}{}", f.ts_us / 1000, if f.display { "" } else { "-" }));
            Ok(())
        }
        fn flush(&mut self) {}
    }
    struct Ev(Arc<Log>);
    impl Events for Ev {
        fn keyframe(&self) {
            self.0 .0.lock().unwrap().push("K".into());
        }
        fn latency(&self, _: u32) {}
        fn first_frame(&self) {
            self.0 .0.lock().unwrap().push("F".into());
        }
        fn failed(&self, m: String) {
            self.0 .0.lock().unwrap().push(m);
        }
    }

    fn chunk(ms: i64, key: bool) -> Input {
        Input::Chunk { data: vec![], key, ts_us: ms * 1000, cap: 0.0 }
    }

    #[test]
    fn paces_skips_and_waits_for_keyframes() {
        let log = Arc::new(Log::default());
        let v = NativeVideo::start(Box::new(Sink(log.clone())), Box::new(Ev(log.clone())));
        v.send(Input::Config(StreamConfig { codec: "avc1".into(), width: 2, height: 2 }));
        v.send(chunk(0, false)); // dropped: no keyframe yet
        v.send(chunk(16, true));
        v.send(chunk(33, false));
        std::thread::sleep(Duration::from_millis(150));
        // Two frames arriving together after a stall: both decoded, the older one hidden.
        v.send(chunk(50, false));
        v.send(chunk(66, false));
        std::thread::sleep(Duration::from_millis(250));
        let got = log.0.lock().unwrap().join(" ");
        assert!(got.starts_with("K 16 F 33"), "{got}");
        // 50 is hidden if both were read together; the thread may also wake between the sends.
        assert!(got.ends_with("50- 66") || got.ends_with("50 66"), "{got}");
        assert!(!got.contains(" 0 "), "{got}");
    }

    struct BrokenSink;
    impl VideoSink for BrokenSink {
        fn configure(&mut self, _: &StreamConfig) -> anyhow::Result<()> {
            Ok(())
        }
        fn submit(&mut self, _: &Frame) -> anyhow::Result<()> {
            anyhow::bail!("decoder broke")
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn falls_back_after_repeated_sink_errors() {
        let log = Arc::new(Log::default());
        let v = NativeVideo::start(Box::new(BrokenSink), Box::new(Ev(log.clone())));
        v.send(Input::Config(StreamConfig { codec: "avc1".into(), width: 2, height: 2 }));
        for i in 0..5 {
            v.send(chunk(i * 16, true));
            std::thread::sleep(Duration::from_millis(80));
        }
        let got = log.0.lock().unwrap().join(" | ");
        // Start + two restarts, then it gives up (and ignores the rest).
        assert_eq!(got, "K | K | K | 3 sink errors in a row, last: decoder broke", "{got}");
    }
}
