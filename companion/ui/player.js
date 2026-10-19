'use strict';

// One PiP window in the companion app. Compressed video arrives from the tab
// and goes to decoder-worker.js, which decodes it and feeds evenly paced frames
// into a <video> element here. The controls (shared with the extension) send
// commands back to the real video in the tab.

const { invoke, Channel } = window.__TAURI__.core;
const appWindow = window.__TAURI__.window.getCurrentWindow();

// The app pre-loads this page in a hidden window and hands it out when a PiP
// is requested ('assigned'). It adds our PiP id to everything we send.
const send = (msg) => invoke('send', { msg }).catch(() => {});
const command = (cmd, value) => send({ type: 'cmd', cmd, value });

BP.adoptCss(
  document,
  BP.PIP_CSS +
    `
  .stage video { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; }
  .waiting { position: absolute; inset: 0; display: grid; place-items: center; color: #aaa; font-size: 13px; }
`,
);

// ---- playback state mirrored from the tab ----
const listeners = new Map();
const emit = (...types) => types.forEach((t) => listeners.get(t)?.forEach((fn) => fn()));
const now = () => performance.now();

let st = { paused: true, ended: false, currentTime: 0, duration: 0, volume: 1, muted: false, rate: 1, bufferedEnd: 0 };
let stAt = now();
let localSeekUntil = 0;

const media = {
  get paused() { return st.paused; },
  get ended() { return st.ended; },
  // Advance the clock locally between updates so the scrubber moves smoothly.
  get currentTime() {
    if (st.paused || st.ended) return st.currentTime;
    const t = st.currentTime + ((now() - stAt) / 1000) * st.rate;
    return Number.isFinite(st.duration) ? Math.min(t, st.duration) : t;
  },
  get duration() { return st.duration; },
  get volume() { return st.volume; },
  get muted() { return st.muted; },
  seeking: false,
  get bufferedEnd() { return st.bufferedEnd; },
  play() {
    st = { ...st, currentTime: media.currentTime, paused: false, ended: false };
    stAt = now();
    command('play');
    emit('play');
  },
  pause() {
    st = { ...st, currentTime: media.currentTime, paused: true };
    stAt = now();
    command('pause');
    emit('pause');
  },
  seek(t) {
    st = { ...st, currentTime: t };
    stAt = now();
    localSeekUntil = now() + 600;
    throttledSeek(t);
    emit('timeupdate');
  },
  setVolume(v) {
    st = { ...st, volume: v };
    command('volume', v);
    emit('volumechange');
  },
  setMuted(m) {
    st = { ...st, muted: m };
    command('muted', m);
    emit('volumechange');
  },
  on(type, fn) {
    if (!listeners.has(type)) listeners.set(type, new Set());
    listeners.get(type).add(fn);
  },
};

// While dragging the scrubber, send at most ~12 seeks a second (plus the last one).
let seekTimer = 0;
let seekQueued = null;
function throttledSeek(t) {
  seekQueued = t;
  if (seekTimer) return;
  command('seek', seekQueued);
  seekQueued = null;
  seekTimer = setTimeout(() => {
    seekTimer = 0;
    if (seekQueued != null) throttledSeek(seekQueued);
  }, 80);
}

function applyState(msg) {
  const prev = st;
  const duration = msg.duration === -1 ? Infinity : msg.duration;
  const keepLocalTime = now() < localSeekUntil;
  st = {
    paused: msg.paused,
    ended: msg.ended,
    currentTime: keepLocalTime ? media.currentTime : msg.currentTime,
    duration,
    volume: msg.volume,
    muted: msg.muted,
    rate: msg.rate || 1,
    bufferedEnd: msg.bufferedEnd,
  };
  stAt = now();
  if (msg.title) document.title = msg.title;
  if (msg.width && msg.height) setAspect(msg.width, msg.height);
  if (prev.paused !== st.paused || prev.ended !== st.ended) emit('play');
  if (prev.duration !== st.duration) emit('durationchange');
  if (prev.volume !== st.volume || prev.muted !== st.muted) emit('volumechange');
  emit('timeupdate', 'progress');
}

// ---- captions mirrored from the tab ----
let cap = { available: false, enabled: false, lines: [] };
let onCaptions = () => {};
const captions = {
  available: () => cap.available,
  enabled: () => cap.enabled,
  toggle() {
    cap = { ...cap, enabled: !cap.enabled };
    command('captions');
  },
  onChange(cb) {
    onCaptions = cb;
  },
};

// ---- window ----
async function toggleFullscreen() {
  const fs = await appWindow.isFullscreen();
  await appWindow.setFullscreen(!fs);
}

const ui = BP.createPlayerUI({
  doc: document,
  win: window,
  media,
  captions,
  closeTitle: 'Close (pauses video)',
  actions: {
    returnToTab() {
      command('return');
      appWindow.destroy();
    },
    close() {
      command('close');
      appWindow.destroy();
    },
    fullscreen: toggleFullscreen,
    exitFullscreen: () => appWindow.setFullscreen(false),
  },
});

const videoEl = document.createElement('video');
videoEl.muted = true; // sound keeps playing from the tab
videoEl.autoplay = true;
videoEl.playsInline = true;
videoEl.disablePictureInPicture = true;
const waiting = BP.h(document, 'div', { class: 'waiting', text: 'Connecting…' });
ui.stage.append(videoEl, waiting);
videoEl.addEventListener('resize', () => setAspect(videoEl.videoWidth, videoEl.videoHeight));

// Drag the window by the video; double-click for fullscreen.
ui.root.addEventListener('mousedown', (e) => {
  if (e.button !== 0 || e.target.closest('.top, .bottom')) return;
  if (e.detail === 2) toggleFullscreen();
  else appWindow.startDragging();
});

let aspectKey = '';
function setAspect(w, h) {
  const key = `${w}x${h}`;
  if (!w || !h || key === aspectKey) return;
  aspectKey = key;
  invoke('set_aspect', { width: w, height: h }).catch(() => {});
}

// Tell the tab how big we are, so it sends just enough pixels.
let viewTimer = 0;
function reportView() {
  clearTimeout(viewTimer);
  viewTimer = setTimeout(
    () => send({ type: 'view', width: innerWidth, height: innerHeight, dpr: devicePixelRatio || 1 }),
    150,
  );
}
addEventListener('resize', reportView);

// ---- decoding (in a worker) ----
const worker = new Worker('decoder-worker.js');
worker.onmessage = ({ data: msg }) => {
  switch (msg.type) {
    case 'track':
      videoEl.srcObject = new MediaStream([msg.track]);
      videoEl.play().catch(() => {});
      break;
    case 'first-frame':
      waiting.remove();
      send({ type: 'first-frame' });
      break;
    case 'latency':
      send({ type: 'latency', ms: msg.ms });
      break;
    case 'keyframe':
      send({ type: 'keyframe' });
      break;
    case 'stats':
      send({ ...msg, window: `${innerWidth}x${innerHeight}`, size: `${videoEl.videoWidth}x${videoEl.videoHeight}` });
      break;
    case 'error':
      send({ type: 'jserror', message: msg.message });
      break;
  }
};
worker.onerror = (e) => send({ type: 'jserror', message: `worker: ${e.message}` });

async function supportedCodecs() {
  const out = [];
  for (const [name, codec] of [['avc', 'avc1.640028'], ['vp8', 'vp8']]) {
    try {
      if ((await VideoDecoder.isConfigSupported({ codec, codedWidth: 1280, codedHeight: 720 })).supported) out.push(name);
    } catch {}
  }
  return out;
}

// ---- messages from the tab (handled in order) ----
let queue = Promise.resolve();
function onMessage(msg) {
  queue = queue.then(async () => {
    try {
      switch (msg.type) {
        case 'assigned':
          await onAssigned();
          break;
        case 'chunk':
          worker.postMessage(msg);
          break;
        case 'config':
          setAspect(msg.width, msg.height);
          worker.postMessage(msg);
          break;
        case 'state':
          applyState(msg);
          break;
        case 'captions':
          cap = { available: msg.available, enabled: msg.enabled, lines: msg.lines || [] };
          onCaptions(cap.lines);
          break;
      }
    } catch (err) {
      send({ type: 'jserror', message: `${msg.type}: ${err}` });
    }
  });
}

const codecsReady = supportedCodecs();

async function onAssigned() {
  send({
    type: 'ready',
    codecs: await codecsReady,
    view: { width: innerWidth, height: innerHeight, dpr: devicePixelRatio || 1 },
  });
}

(async () => {
  const channel = new Channel();
  channel.onmessage = onMessage;
  await invoke('attach', { channel });
})();
