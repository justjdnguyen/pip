'use strict';

// Companion app mode. The video stays playing in the page (so sound comes from
// the tab as usual); we capture its picture, compress it with the hardware
// encoder at the PiP window's size, and stream it to the Better PiP desktop
// app, which shows it in its own window. The app sends control commands
// (play, seek, volume…) back here.
//
//   content (this file) ⇄ background.js ⇄ native messaging ⇄ app
//
// (WebRTC would be simpler, but privacy-focused browsers like Helium block the
// local connections it needs.)

BP.AppPip = (() => {
  const sessions = new Map(); // video -> session

  const supported = () =>
    typeof HTMLVideoElement.prototype.captureStream === 'function' &&
    typeof MediaStreamTrackProcessor === 'function' &&
    typeof VideoEncoder === 'function';

  async function appAvailable() {
    try {
      const res = await chrome.runtime.sendMessage({ type: 'app-status' });
      return !!res?.available;
    } catch {
      return false;
    }
  }

  // Encode at the size the PiP window shows (never larger than the source).
  function targetSize(srcW, srcH, view) {
    const fit = Math.min(1, (view.width * view.dpr) / srcW, (view.height * view.dpr) / srcH);
    return { w: Math.max(2, Math.round(srcW * fit) & ~1), h: Math.max(2, Math.round(srcH * fit) & ~1) };
  }

  // What the app can decode. It tells us on 'ready'; until then assume the
  // usual answer so the encoder can start while the window is being set up.
  const DEFAULT_CODECS = ['avc', 'vp8'];

  async function pickConfig(codecs, w, h, fps) {
    // Generous bitrate: this never leaves the computer, and starved encoders
    // are what make the picture go blurry during fast motion.
    const bitrate = BP.clamp(w * h * fps * 0.12, 2e6, 25e6);
    const base = { width: w, height: h, bitrate, framerate: fps, latencyMode: 'realtime' };
    const candidates = [];
    if (codecs.includes('avc')) {
      const avc = { ...base, codec: w * h > 1920 * 1088 ? 'avc1.640033' : 'avc1.640028', avc: { format: 'annexb' } };
      candidates.push({ ...avc, hardwareAcceleration: 'prefer-hardware' }, avc);
    }
    if (codecs.includes('vp8')) candidates.push({ ...base, codec: 'vp8' });
    for (const config of candidates) {
      try {
        if ((await VideoEncoder.isConfigSupported(config)).supported) return config;
      } catch {}
    }
    return null;
  }
  // Lip sync. The sound keeps playing from the tab, but the picture reaches
  // the PiP window a little later (capture, encode, transfer, decode). We route
  // the video's sound through a delay of the same length. The routing can't be
  // undone, so after PiP closes it stays in place with a delay of zero.
  const audioGraphs = new WeakMap(); // video -> Promise<{ setDelay } | null>
  function audioDelay(video) {
    if (!audioGraphs.has(video)) audioGraphs.set(video, createAudioDelay(video));
    return audioGraphs.get(video);
  }
  async function createAudioDelay(video) {
    let ctx;
    try {
      // 'playback' uses bigger audio buffers than 'interactive'. Small buffers
      // can run dry while the page is busy, which sounds like crackling.
      ctx = new AudioContext({ latencyHint: 'playback' });
      // Only reroute if the context can actually play, or the video would go silent.
      await Promise.race([ctx.resume(), new Promise((r) => setTimeout(r, 1000))]);
      if (ctx.state !== 'running') throw new Error(`audio context ${ctx.state}`);
      // Throws if the site already routes this video through Web Audio.
      const source = ctx.createMediaElementSource(video);
      const delay = ctx.createDelay(1);
      const fade = ctx.createGain();
      source.connect(delay).connect(fade).connect(ctx.destination);
      video.addEventListener('play', () => ctx.resume().catch(() => {}));
      // This route adds its own output latency on top of the tab's usual
      // ~26ms; that already delays the sound, so count it. (outputLatency is
      // only known once audio has played for a moment, so check each time.)
      const extra = () => Math.max(0, ctx.baseLatency + (ctx.outputLatency || 0) - 0.026);
      let current = 0;
      return {
        setDelay(seconds) {
          const target = seconds > 0 ? BP.clamp(seconds - extra(), 0, 0.9) : 0;
          const now = ctx.currentTime;
          const param = delay.delayTime;
          if (current === 0 || target === 0) {
            // Big jump (PiP opening or closing): fade out, jump, fade back in.
            if (target === current) return;
            fade.gain.cancelScheduledValues(now);
            fade.gain.setValueAtTime(fade.gain.value, now);
            fade.gain.linearRampToValueAtTime(0, now + 0.03);
            param.cancelScheduledValues(now);
            param.setValueAtTime(target, now + 0.03);
            fade.gain.linearRampToValueAtTime(1, now + 0.06);
          } else {
            // Small drift: change slowly (any change briefly alters the pitch),
            // and only when it's large enough to notice.
            if (Math.abs(target - current) < 0.02) return;
            param.cancelScheduledValues(now);
            param.setValueAtTime(param.value, now);
            param.linearRampToValueAtTime(target, now + 2);
          }
          current = target;
        },
      };
    } catch (err) {
      console.info('[Better PiP] Can’t delay this video’s sound for lip sync:', err);
      ctx?.close().catch(() => {});
      return null;
    }
  }

  const codecFamily = (codec) => (codec.startsWith('avc') ? 'avc' : codec);

  async function open(video, size) {
    if (BP.settings.engine === 'chrome' || !supported()) return false;
    // DRM-protected video (Netflix etc.) can't be captured.
    if (video.mediaKeys) return false;
    if (!(await appAvailable())) return false;
    let stream;
    try {
      stream = video.captureStream();
    } catch (err) {
      // e.g. cross-origin video without CORS.
      console.info('[Better PiP] Can’t capture this video, using Chrome PiP:', err);
      return false;
    }
    sessions.set(video, createSession(video, stream, size));
    return true;
  }

  function createSession(video, stream, size) {
    const ac = new AbortController();
    const on = (target, type, fn) => target.addEventListener(type, fn, { signal: ac.signal });
    const adapter = BP.adapters.find(video);
    const ctx = adapter.capture(video);
    const port = chrome.runtime.connect({ name: 'bp-app' });
    let audio = null;
    audioDelay(video).then((a) => (audio = closed ? null : a));
    let closed = false;

    const placeholder = BP.mountPlaceholder({
      video,
      slot: adapter.placeholderSlot(video, ctx),
      keepVideo: true,
      onActivate: () => end(),
    });
    BP.toggle?.refresh();

    const post = (msg) => {
      if (closed) return;
      try {
        port.postMessage(msg);
      } catch {
        end();
      }
    };

    post({
      type: 'open',
      width: size.width,
      height: size.height,
      videoWidth: video.videoWidth,
      videoHeight: video.videoHeight,
      title: document.title,
      // For startup timing in the app's debug log.
      clickedAt: Date.now(),
    });

    // ---- picture: capture → scale → encode → send ----
    let codecs = DEFAULT_CODECS;
    let encoderFamily = '';
    let view = { width: size.width, height: size.height, dpr: devicePixelRatio || 1 };
    let encoder = null;
    let encoderKey = '';
    let configuring = false;
    let canvas = null;
    let g = null;
    let forceKey = true;
    let lastKeyTs = -Infinity;
    let reader = null;
    const encodeStart = new Map(); // frame timestamp -> when we handed it to the encoder

    // Pipeline stats, reported every 2s (visible in the app's debug log).
    const stats = { frames: 0, encoded: 0, dropped: 0, bytes: 0, encodeMs: 0 };

    async function configure(w, h, fps) {
      configuring = true;
      try {
        const config = await pickConfig(codecs, w, h, fps);
        if (!config || closed) return;
        try {
          encoder?.close();
        } catch {}
        encoder = new VideoEncoder({
          output: sendChunk,
          error: (err) => {
            console.warn('[Better PiP] Encoder error:', err);
            encoder = null;
            encoderKey = '';
          },
        });
        encoder.configure(config);
        encoderKey = `${w}x${h}`;
        encoderFamily = codecFamily(config.codec);
        forceKey = true;
        canvas = new OffscreenCanvas(w, h);
        g = canvas.getContext('2d', { alpha: false, desynchronized: true });
        post({ type: 'config', codec: config.codec, width: w, height: h, hw: config.hardwareAcceleration || 'auto' });
      } finally {
        configuring = false;
      }
    }

    function sendChunk(chunk) {
      const bytes = new Uint8Array(chunk.byteLength);
      chunk.copyTo(bytes);
      const started = encodeStart.get(chunk.timestamp);
      // When this frame was showing in the tab (wall clock), so the app can
      // tell how far behind the tab's sound its picture is.
      const shownAt = started ? Date.now() - (performance.now() - started) : 0;
      if (started) {
        stats.encodeMs += performance.now() - started;
        encodeStart.delete(chunk.timestamp);
      }
      stats.encoded++;
      stats.bytes += chunk.byteLength;
      post({ type: 'chunk', key: chunk.type === 'key', ts: chunk.timestamp, cap: shownAt, sent: Date.now(), data: toBase64(bytes) });
    }

    function encodeFrame(frame, fps) {
      const srcW = frame.displayWidth;
      const srcH = frame.displayHeight;
      if (!srcW || !srcH) return;
      stats.frames++;
      const { w, h } = targetSize(srcW, srcH, view);
      if (!encoder || encoderKey !== `${w}x${h}`) {
        if (!configuring) configure(w, h, fps);
        return;
      }
      // Drop frames rather than fall behind.
      if (encoder.encodeQueueSize > 3) {
        stats.dropped++;
        return;
      }
      // Scale through a canvas. This also normalises frame formats the
      // hardware encoder rejects when fed straight from captureStream().
      g.drawImage(frame, 0, 0, w, h);
      const copy = new VideoFrame(canvas, { timestamp: frame.timestamp });
      // Keyframes are big, so the frames right after one get fewer bits and
      // look softer. The link to the app doesn't lose data, so they're only
      // needed on request (new window, decoder trouble), plus a rare safety one.
      const key = forceKey || frame.timestamp - lastKeyTs > 10e6;
      if (key) {
        forceKey = false;
        lastKeyTs = frame.timestamp;
      }
      if (encodeStart.size > 120) encodeStart.clear();
      encodeStart.set(frame.timestamp, performance.now());
      try {
        encoder.encode(copy, { keyFrame: key });
      } finally {
        copy.close();
      }
    }

    async function pump(track) {
      reader?.cancel().catch(() => {});
      const fps = Math.round(track.getSettings().frameRate) || 60;
      const r = new MediaStreamTrackProcessor({ track }).readable.getReader();
      reader = r;
      while (!closed && reader === r) {
        let res;
        try {
          res = await r.read();
        } catch {
          break;
        }
        if (res.done) break;
        try {
          encodeFrame(res.value, fps);
        } finally {
          res.value.close();
        }
      }
    }

    function startCapture() {
      const track = stream.getVideoTracks().find((t) => t.readyState === 'live');
      if (track) pump(track);
    }
    // A new source (e.g. YouTube moving to the next video) adds a new track.
    on(stream, 'addtrack', (e) => {
      if (e.track.kind === 'video') pump(e.track);
    });

    function sendStats() {
      post({
        type: 'cstats',
        ...stats,
        encodeMsAvg: +(stats.encodeMs / Math.max(1, stats.encoded)).toFixed(1),
        size: encoderKey,
      });
      Object.keys(stats).forEach((k) => (stats[k] = 0));
    }

    // ---- state + captions → app ----
    const captions = adapter.captions(video, ctx);
    let captionLines = [];
    const sendCaptions = () =>
      post({ type: 'captions', available: captions.available(), enabled: captions.enabled(), lines: captionLines });
    captions.onChange((lines) => {
      captionLines = lines;
      sendCaptions();
    });

    const sendState = () =>
      post({
        type: 'state',
        paused: video.paused,
        ended: video.ended,
        currentTime: video.currentTime,
        // JSON has no Infinity; -1 means live.
        duration: video.duration === Infinity ? -1 : video.duration || 0,
        volume: video.volume,
        muted: video.muted,
        rate: video.playbackRate,
        bufferedEnd: BP.bufferedEnd(video),
        width: video.videoWidth,
        height: video.videoHeight,
        title: document.title,
      });
    for (const type of ['play', 'pause', 'ended', 'seeked', 'durationchange', 'loadedmetadata', 'emptied', 'volumechange', 'ratechange', 'progress']) {
      on(video, type, sendState);
    }

    const stateTimer = setInterval(() => {
      sendState();
      sendCaptions();
      sendStats();
    }, 2000);

    // ---- app → us ----
    // Handle messages strictly in order (call setup steps depend on each other).
    let queue = Promise.resolve();
    port.onMessage.addListener((msg) => {
      queue = queue.then(() => handleMessage(msg));
    });
    async function handleMessage(msg) {
      try {
        switch (msg.type) {
          case 'ready':
            codecs = msg.codecs || [];
            // Start over if we guessed a codec the app can't decode.
            if (encoderFamily && !codecs.includes(encoderFamily)) encoderKey = '';
            if (msg.view) view = msg.view;
            sendState();
            sendCaptions();
            startCapture();
            break;
          case 'latency':
            // How far the picture in the app trails this tab; hold the sound back to match.
            audio?.setDelay(msg.ms / 1000);
            break;
          case 'keyframe':
            forceKey = true;
            break;
          case 'view':
            // The window was resized; the next frame reconfigures if needed.
            view = { width: msg.width, height: msg.height, dpr: msg.dpr || 1 };
            break;
          case 'cmd':
            command(msg.cmd, msg.value);
            break;
          case 'closed':
            end({ notify: false });
            break;
          case 'app-error':
            console.warn('[Better PiP] App error:', msg.error);
            end({ notify: false });
            break;
        }
      } catch (err) {
        console.warn('[Better PiP]', msg.type, err);
      }
    }
    port.onDisconnect.addListener(() => end({ notify: false }));

    function command(cmd, value) {
      switch (cmd) {
        case 'play':
          video.play().catch(() => {});
          break;
        case 'pause':
          video.pause();
          break;
        case 'seek':
          if (Number.isFinite(value)) video.currentTime = value;
          break;
        case 'volume':
          if (Number.isFinite(value)) video.volume = BP.clamp(value, 0, 1);
          break;
        case 'muted':
          video.muted = !!value;
          break;
        case 'rate':
          if (Number.isFinite(value) && value > 0) video.playbackRate = BP.clamp(value, 0.0625, 16);
          break;
        case 'captions':
          if (captions.available()) captions.toggle();
          setTimeout(sendCaptions, 200);
          break;
        case 'return':
          end({ focus: true });
          break;
        case 'close':
          video.pause();
          end();
          break;
      }
      sendState();
    }

    on(window, 'pagehide', () => end());

    function end({ notify = true, focus = false } = {}) {
      if (closed) return;
      closed = true;
      ac.abort();
      audio?.setDelay(0);
      clearInterval(stateTimer);
      reader?.cancel().catch(() => {});
      try {
        encoder?.close();
      } catch {}
      stream.getTracks().forEach((t) => t.stop());
      captions.destroy();
      placeholder.remove();
      sessions.delete(video);
      if (notify) {
        try {
          port.disconnect();
        } catch {}
      }
      if (focus) chrome.runtime.sendMessage({ type: 'focus-tab' }).catch(() => {});
      BP.toggle?.refresh();
    }

    return { end };
  }

  function toBase64(bytes) {
    if (bytes.toBase64) return bytes.toBase64();
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  }

  // Called when the user hovers a video: get everything ready so the click
  // opens quickly. Starts the app (via the background), and runs one frame of
  // this video through capture and the hardware encoder. The first capture and
  // encoder in a tab take ~0.5–1s to start; after that they're quick.
  let lastWarm = 0;
  const warmedVideos = new WeakSet();
  function warmUp(video) {
    if (BP.settings.engine === 'chrome' || !supported()) return;
    if (Date.now() - lastWarm > 30000) {
      lastWarm = Date.now();
      appAvailable();
    }
    if (video && !warmedVideos.has(video) && !video.mediaKeys && !video.paused && !sessions.has(video)) {
      warmedVideos.add(video);
      warmPipeline(video).catch(() => {});
    }
  }

  async function warmPipeline(video) {
    let stream;
    try {
      stream = video.captureStream();
    } catch {
      return; // can't capture this video; open() will fall back
    }
    const stop = () => stream.getTracks().forEach((t) => t.stop());
    const timeout = (ms) => new Promise((_, reject) => setTimeout(() => reject(new Error('timeout')), ms));
    let encoder = null;
    try {
      const track =
        stream.getVideoTracks()[0] ||
        (await Promise.race([
          new Promise((resolve) => stream.addEventListener('addtrack', (e) => e.track.kind === 'video' && resolve(e.track))),
          timeout(3000),
        ]));
      const reader = new MediaStreamTrackProcessor({ track }).readable.getReader();
      const { value: frame } = await Promise.race([reader.read(), timeout(3000)]);
      reader.cancel().catch(() => {});
      if (!frame) return;
      try {
        const view = { width: BP.settings.width, height: BP.settings.width, dpr: devicePixelRatio || 1 };
        const { w, h } = targetSize(frame.displayWidth, frame.displayHeight, view);
        const config = await pickConfig(DEFAULT_CODECS, w, h, 60);
        if (!config) return;
        const canvas = new OffscreenCanvas(w, h);
        canvas.getContext('2d', { alpha: false }).drawImage(frame, 0, 0, w, h);
        encoder = new VideoEncoder({ output() {}, error() {} });
        encoder.configure(config);
        const copy = new VideoFrame(canvas, { timestamp: 0 });
        encoder.encode(copy, { keyFrame: true });
        copy.close();
        await Promise.race([encoder.flush(), timeout(3000)]);
      } finally {
        frame.close();
      }
    } finally {
      try {
        encoder?.close();
      } catch {}
      stop();
    }
  }

  return {
    open,
    warmUp,
    has: (video) => sessions.has(video),
    count: () => sessions.size,
    close: (video, opts) => sessions.get(video)?.end(opts),
    closeAll: () => [...sessions.values()].forEach((s) => s.end()),
  };
})();
