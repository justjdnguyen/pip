'use strict';

// Decode and pace frames in a worker. WebKit can present a generated video
// track; WebView2 lacks that worker API, so draw into a transferred canvas.
let writer = null;
let canvas = null;
let canvasContext = null;
if (typeof VideoTrackGenerator === 'function') {
  const generator = new VideoTrackGenerator();
  writer = generator.writable.getWriter();
  self.postMessage({ type: 'track', track: generator.track }, [generator.track]);
} else {
  self.postMessage({ type: 'canvas' });
}

let decoder = null;
let decoderConfig = null;
let waitingForKey = true;

const perf = { decodedLag: 0, received: 0, decoded: 0, shown: 0, late: 0, toApp: 0, toPage: 0, toPageMax: 0 };

function configure(cfg) {
  decoderConfig = cfg;
  try {
    decoder?.close();
  } catch {}
  decoder = new VideoDecoder({
    output: onDecoded,
    error(err) {
      self.postMessage({ type: 'error', message: `decoder: ${err}` });
      setTimeout(() => decoderConfig && configure(decoderConfig), 0);
    },
  });
  decoder.configure({ codec: cfg.codec, codedWidth: cfg.width, codedHeight: cfg.height, optimizeForLatency: true });
  waitingForKey = true;
  self.postMessage({ type: 'keyframe' });
}

function base64ToBytes(b64) {
  if (Uint8Array.fromBase64) return Uint8Array.fromBase64(b64);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function decodeChunk(msg) {
  perf.received++;
  if (msg.sent && msg.rx) {
    perf.toApp += msg.rx - msg.sent;
    const t = Date.now() - msg.rx;
    perf.toPage += t;
    perf.toPageMax = Math.max(perf.toPageMax, t);
  }
  if (!decoder || decoder.state !== 'configured') return;
  if (waitingForKey && !msg.key) return;
  // Badly behind: skip ahead to the next keyframe.
  if (decoder.decodeQueueSize > 6 && !msg.key) {
    waitingForKey = true;
    self.postMessage({ type: 'keyframe' });
    return;
  }
  waitingForKey = false;
  if (msg.cap) {
    shownInTab.set(msg.ts, msg.cap);
    if (shownInTab.size > 300) shownInTab.delete(shownInTab.keys().next().value);
  }
  decoder.decode(new EncodedVideoChunk({ type: msg.key ? 'key' : 'delta', timestamp: msg.ts, data: base64ToBytes(msg.data) }));
}

// ---- paced output ----
// Each frame is shown a short, adaptive delay after the moment its capture
// timestamp maps to. That evens out uneven delivery.
// ---- lip sync ----
// How long after the tab showed a frame we show it here. The tab delays its
// sound by this much. On top of handing the frame over, the <video> element
// takes about a frame and a half to get it on screen.
const PRESENT_MS = 25;
const shownInTab = new Map(); // frame timestamp -> wall-clock time the tab showed it
let latency = null;
let latencySent = 0;
let latencySamples = 0;
function trackLatency(frame) {
  const cap = shownInTab.get(frame.timestamp);
  if (!cap) return;
  shownInTab.delete(frame.timestamp);
  const ms = Date.now() - cap + PRESENT_MS;
  // Skip the first half second: frames are slow while everything spins up.
  if (++latencySamples < 30) return;
  // Smooth it so the sound delay doesn't jitter.
  latency = latency == null ? ms : latency * 0.95 + ms * 0.05;
  if (latencySamples > 45 && performance.now() - latencySent > 500) {
    latencySent = performance.now();
    self.postMessage({ type: 'latency', ms: Math.round(latency) });
  }
}

const MIN_DELAY = 15;
const MAX_DELAY = 150;
let delayMs = 40;
let clock = null; // maps capture time (µs) to local time (ms)
const queued = [];
let timer = 0;
let firstFrame = true;

const dueAt = (frame, delay = delayMs) => clock.at + (frame.timestamp - clock.ts) / 1000 + delay;

function onDecoded(frame) {
  perf.decoded++;
  const t = performance.now();
  const lead = clock ? dueAt(frame, 0) - t : 0;
  if (!clock || Math.abs(lead) > 1000) {
    // First frame, or the timeline jumped (seek, new video): resync.
    clock = { ts: frame.timestamp, at: t };
  } else if (lead > 0) {
    // This frame arrived quicker than the one we synced to, so that one was
    // held up. Sync to this one, so frames aren't held back for no reason.
    clock.at -= lead;
  }
  if (shownInTab.has(frame.timestamp)) perf.decodedLag += Date.now() - shownInTab.get(frame.timestamp);
  if (dueAt(frame) < t) perf.late++;
  queued.push(frame);
  while (queued.length > 30) queued.shift().close();
  schedule();
}

function schedule() {
  if (timer || !queued.length) return;
  timer = setTimeout(() => {
    timer = 0;
    show();
    schedule();
  }, Math.max(0, dueAt(queued[0]) - performance.now()));
}

function show() {
  const t = performance.now();
  let frame = null;
  while (queued.length && dueAt(queued[0]) <= t + 1) {
    frame?.close();
    frame = queued.shift();
  }
  if (!frame) return;
  if (!writer && !canvasContext) {
    frame.close();
    return;
  }
  trackLatency(frame);
  if (writer) {
    // write() takes ownership of the frame.
    writer.write(frame).catch(() => {});
  } else {
    try {
      if (canvas.width !== frame.displayWidth) canvas.width = frame.displayWidth;
      if (canvas.height !== frame.displayHeight) canvas.height = frame.displayHeight;
      canvasContext.drawImage(frame, 0, 0);
    } finally {
      frame.close();
    }
  }
  perf.shown++;
  if (firstFrame) {
    firstFrame = false;
    self.postMessage({ type: 'first-frame' });
  }
}

setInterval(() => {
  // More late frames → longer delay; otherwise slowly shrink it.
  if (perf.late > 2) delayMs = Math.min(MAX_DELAY, delayMs + 10);
  else if (perf.late === 0) delayMs = Math.max(MIN_DELAY, delayMs - 3);
  self.postMessage({
    type: 'stats',
    ...perf,
    toApp: undefined,
    toPage: undefined,
    toAppMs: +(perf.toApp / Math.max(1, perf.received)).toFixed(1),
    toPageMs: +(perf.toPage / Math.max(1, perf.received)).toFixed(1),
    delayMs,
    latencyMs: latency && Math.round(latency),
    decodedLagMs: Math.round(perf.decodedLag / Math.max(1, perf.decoded)),
    decodeQueue: decoder?.decodeQueueSize,
    size: canvas ? `${canvas.width}x${canvas.height}` : undefined,
  });
  Object.keys(perf).forEach((k) => (perf[k] = 0));
}, 2000);

self.onmessage = ({ data: msg }) => {
  if (msg.type === 'canvas') {
    canvas = msg.canvas;
    canvasContext = canvas.getContext('2d', { alpha: false, desynchronized: true });
  } else if (msg.type === 'chunk') decodeChunk(msg);
  else if (msg.type === 'config') configure(msg);
};
