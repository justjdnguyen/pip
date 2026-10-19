'use strict';

// The Firefox-style PiP controls. Shared by the Chrome PiP window (where
// `media` wraps the real <video>) and the companion app (where `media` relays
// commands back to the tab).
//
// media: {
//   paused, ended, currentTime, duration, volume, muted, rate, seeking, bufferedEnd
//   play(), pause(), seek(t), setVolume(v), setMuted(m), setRate(r)
//   on(type, fn)  – 'play' 'pause' 'ended' 'timeupdate' 'durationchange'
//                   'loadedmetadata' 'emptied' 'progress' 'volumechange' 'seeked' 'ratechange'
// }
// captions: { available(), enabled(), toggle(), onChange(cb(lines)) }
// actions:  { returnToTab(), close(), fullscreen() }

BP.SKIP_SECONDS = 5;
// Speeds the speed button cycles through, and the finer steps for < and >.
BP.QUICK_RATES = [1, 1.5, 2];
BP.RATE_STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

// Wrap a real <video> element as `media`.
BP.localMedia = (video, signal) => ({
  get paused() { return video.paused; },
  get ended() { return video.ended; },
  get currentTime() { return video.currentTime; },
  get duration() { return video.duration; },
  get volume() { return video.volume; },
  get muted() { return video.muted; },
  get rate() { return video.playbackRate; },
  get seeking() { return video.seeking; },
  get bufferedEnd() { return BP.bufferedEnd(video); },
  play: () => video.play().catch(() => {}),
  pause: () => video.pause(),
  seek: (t) => { video.currentTime = t; },
  setVolume: (v) => { video.volume = v; },
  setMuted: (m) => { video.muted = m; },
  setRate: (r) => { video.playbackRate = r; },
  on: (type, fn) => video.addEventListener(type, fn, { signal }),
});

// End of the buffered range that contains the playhead.
BP.bufferedEnd = (video) => {
  const b = video.buffered;
  let end = 0;
  for (let i = 0; i < b.length; i++) {
    if (b.start(i) <= video.currentTime + 0.5) end = Math.max(end, b.end(i));
  }
  return end;
};

BP.createPlayerUI = ({ doc, win, media, captions, actions, closeTitle = 'Close (pauses video)' }) => {
  const SKIP = BP.SKIP_SECONDS;
  const HIDE_DELAY_MS = 2500;
  const ac = new AbortController();
  const on = (target, type, fn, opts = {}) => target.addEventListener(type, fn, { ...opts, signal: ac.signal });
  const h = (tag, props, children) => BP.h(doc, tag, props, children);
  const icon = (name) => BP.icon(doc, name);
  const button = (cls, iconName, title, onClick) => {
    const b = h('button', { class: cls, title, 'aria-label': title }, [icon(iconName)]);
    on(b, 'click', (e) => {
      e.stopPropagation();
      onClick();
    });
    return b;
  };
  const setIcon = (btn, name) => btn.replaceChildren(icon(name));

  // ---- DOM ----
  const stage = h('div', { class: 'stage' });
  const shade = h('div', { class: 'shade' });
  const captionsEl = h('div', { class: 'captions', 'aria-live': 'polite' });
  const osd = h('div', { class: 'osd' });

  const returnBtn = button('return', 'returnToTab', 'Back to tab', () => actions.returnToTab());
  const closeBtn = button('close', 'close', closeTitle, () => actions.close());
  const top = h('div', { class: 'top' }, [returnBtn, closeBtn]);

  const timeEl = h('div', { class: 'time', text: '0:00 / 0:00' });
  const liveBadge = h('div', { class: 'live-badge', text: 'LIVE' });
  const backBtn = button('back5', 'back5', `Back ${SKIP}s (←)`, () => seekBy(-SKIP));
  const playBtn = button('play', 'play', 'Play (Space)', togglePlay);
  const fwdBtn = button('fwd5', 'fwd5', `Forward ${SKIP}s (→)`, () => seekBy(SKIP));
  const muteBtn = button('mute', 'volumeHigh', 'Mute (M)', toggleMute);
  const volFill = h('div', { class: 'fill' });
  const volKnob = h('div', { class: 'knob' });
  const vol = h('div', { class: 'slider vol', role: 'slider', 'aria-label': 'Volume' }, [
    h('div', { class: 'track' }, [volFill, volKnob]),
  ]);
  const rateBtn = h('button', { class: 'rate', title: 'Speed (S)', 'aria-label': 'Playback speed' });
  on(rateBtn, 'click', (e) => {
    e.stopPropagation();
    cycleRate();
  });
  const ccBtn = button('cc', 'ccOff', 'Captions (C)', toggleCaptions);
  const fsBtn = button('fs', 'fullscreen', 'Fullscreen (F)', () => actions.fullscreen());

  const buffered = h('div', { class: 'buffered' });
  const played = h('div', { class: 'fill' });
  const scrubKnob = h('div', { class: 'knob' });
  const tip = h('div', { class: 'tip' });
  const scrub = h('div', { class: 'slider scrub', role: 'slider', 'aria-label': 'Seek' }, [
    h('div', { class: 'track' }, [buffered, played, scrubKnob]),
    tip,
  ]);

  const bottom = h('div', { class: 'bottom' }, [
    h('div', { class: 'row' }, [
      h('div', {}, [timeEl, liveBadge]),
      h('div', { class: 'transport' }, [backBtn, playBtn, fwdBtn]),
      h('div', { class: 'right' }, [muteBtn, vol, rateBtn, ccBtn, fsBtn]),
    ]),
    scrub,
  ]);

  const root = h('div', { class: 'pip' }, [stage, shade, captionsEl, osd, top, bottom]);
  doc.body.append(root);

  // ---- captions ----
  captions.onChange((lines) => {
    captionsEl.replaceChildren(...lines.map((l) => h('span', { text: l })));
    renderCaptionsButton();
  });

  // ---- rendering ----
  let scrubbing = false;
  let scrubPreview = null;
  const duration = () => media.duration;
  const isLive = () => duration() === Infinity;

  function renderPlay() {
    const paused = media.paused || media.ended;
    setIcon(playBtn, paused ? 'play' : 'pause');
    playBtn.title = paused ? 'Play (Space)' : 'Pause (Space)';
    playBtn.setAttribute('aria-label', playBtn.title);
  }

  function renderTime() {
    root.classList.toggle('live', isLive());
    const d = duration();
    const t = scrubPreview ?? media.currentTime;
    timeEl.textContent = `${BP.formatTime(t)} / ${BP.formatTime(d)}`;
    const pct = Number.isFinite(d) && d > 0 ? (t / d) * 100 : 0;
    played.style.width = `${pct}%`;
    scrubKnob.style.left = `${pct}%`;
    scrub.setAttribute('aria-valuenow', Math.round(t));
  }

  function renderBuffered() {
    const d = duration();
    buffered.style.width = Number.isFinite(d) && d > 0 ? `${(media.bufferedEnd / d) * 100}%` : '0';
  }

  function renderVolume() {
    const level = media.muted ? 0 : media.volume;
    setIcon(muteBtn, level === 0 ? 'volumeOff' : level < 0.5 ? 'volumeLow' : 'volumeHigh');
    muteBtn.title = media.muted ? 'Unmute (M)' : 'Mute (M)';
    volFill.style.width = `${level * 100}%`;
    volKnob.style.left = `${level * 100}%`;
    vol.setAttribute('aria-valuenow', Math.round(level * 100));
  }

  function renderCaptionsButton() {
    const available = captions.available();
    const enabled = available && captions.enabled();
    ccBtn.disabled = !available;
    setIcon(ccBtn, enabled ? 'ccOn' : 'ccOff');
    ccBtn.title = available ? (enabled ? 'Turn off captions (C)' : 'Turn on captions (C)') : 'No captions available';
  }

  const formatRate = (r) => `${+r.toFixed(2)}×`;
  function renderRate() {
    const r = media.rate || 1;
    rateBtn.textContent = formatRate(r);
    rateBtn.classList.toggle('changed', r !== 1);
    rateBtn.title = `Speed ${formatRate(r)} (S; < and > for finer steps)`;
  }

  function renderAll() {
    renderRate();
    renderPlay();
    renderTime();
    renderBuffered();
    renderVolume();
    renderCaptionsButton();
  }

  media.on('play', renderPlay);
  media.on('pause', renderPlay);
  media.on('ended', renderPlay);
  media.on('timeupdate', renderTime);
  media.on('durationchange', () => (renderTime(), renderBuffered()));
  media.on('loadedmetadata', renderAll);
  media.on('emptied', renderAll);
  media.on('progress', renderBuffered);
  media.on('volumechange', renderVolume);
  media.on('seeked', onSeeked);
  media.on('ratechange', renderRate);

  // Smooth scrubber while visible and playing (timeupdate is only ~4/sec).
  const tick = () => {
    if (ac.signal.aborted) return;
    if (root.classList.contains('active') && !media.paused) renderTime();
    win.requestAnimationFrame(tick);
  };
  win.requestAnimationFrame(tick);

  // ---- actions ----
  function togglePlay() {
    if (media.paused || media.ended) media.play();
    else media.pause();
  }

  let pendingSeek = null;
  function seekTo(t) {
    const d = duration();
    if (!Number.isFinite(d)) return;
    pendingSeek = BP.clamp(t, 0, d);
    if (!media.seeking) applySeek();
  }
  function applySeek() {
    if (pendingSeek == null) return;
    const t = pendingSeek;
    pendingSeek = null;
    media.seek(t);
  }
  function onSeeked() {
    applySeek();
    renderTime();
  }
  function seekBy(delta) {
    if (!Number.isFinite(duration())) return;
    seekTo((pendingSeek ?? media.currentTime) + delta);
    flash(`${delta > 0 ? '+' : '−'}${Math.abs(delta)}s`);
  }

  function setVolume(v) {
    v = BP.clamp(v, 0, 1);
    media.setVolume(v);
    media.setMuted(v === 0);
  }
  function toggleMute() {
    if (media.muted || media.volume === 0) {
      if (media.volume === 0) media.setVolume(0.5);
      media.setMuted(false);
    } else {
      media.setMuted(true);
    }
  }

  function toggleCaptions() {
    if (!captions.available()) return;
    captions.toggle();
    // Some sites (YouTube) update asynchronously; re-render shortly after.
    renderCaptionsButton();
    setTimeout(renderCaptionsButton, 150);
  }

  function setRate(r) {
    media.setRate(r);
    renderRate();
    flash(`Speed ${formatRate(r)}`);
  }
  // Button / S: 1× → 1.5× → 2× → 1×. From any other speed, go to the next quick
  // speed above it (or back to 1×).
  function cycleRate() {
    const r = media.rate || 1;
    setRate(BP.QUICK_RATES.find((q) => q > r + 0.01) ?? BP.QUICK_RATES[0]);
  }
  // < and >: one finer step down or up.
  function stepRate(dir) {
    const r = media.rate || 1;
    const steps = BP.RATE_STEPS;
    const next = dir > 0 ? steps.find((s) => s > r + 0.01) : [...steps].reverse().find((s) => s < r - 0.01);
    if (next) setRate(next);
  }

  let osdTimer = 0;
  function flash(text) {
    osd.textContent = text;
    osd.classList.add('show');
    clearTimeout(osdTimer);
    osdTimer = setTimeout(() => osd.classList.remove('show'), 600);
  }

  // ---- sliders (pointer drag) ----
  function fraction(el, clientX) {
    const r = el.querySelector('.track').getBoundingClientRect();
    return BP.clamp((clientX - r.left) / r.width, 0, 1);
  }

  function bindSlider(el, { onMove, onStart, onEnd, onHover }) {
    on(el, 'pointerdown', (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      el.setPointerCapture(e.pointerId);
      el.classList.add('dragging');
      onStart?.(fraction(el, e.clientX));
      onMove(fraction(el, e.clientX));
    });
    on(el, 'pointermove', (e) => {
      onHover?.(fraction(el, e.clientX));
      if (el.hasPointerCapture(e.pointerId)) onMove(fraction(el, e.clientX));
    });
    const end = (e) => {
      if (!el.classList.contains('dragging')) return;
      el.classList.remove('dragging');
      onEnd?.(fraction(el, e.clientX));
    };
    on(el, 'pointerup', end);
    on(el, 'pointercancel', end);
  }

  bindSlider(scrub, {
    onStart() {
      scrubbing = true;
    },
    onMove(f) {
      const d = duration();
      if (!Number.isFinite(d)) return;
      scrubPreview = f * d;
      renderTime();
      seekTo(scrubPreview);
    },
    onEnd(f) {
      const d = duration();
      scrubbing = false;
      scrubPreview = null;
      if (Number.isFinite(d)) seekTo(f * d);
      renderTime();
    },
    onHover(f) {
      const d = duration();
      if (!Number.isFinite(d)) return;
      tip.textContent = BP.formatTime(f * d);
      const w = scrub.getBoundingClientRect().width;
      const tipW = tip.getBoundingClientRect().width;
      tip.style.left = `${BP.clamp(f * w, tipW / 2, w - tipW / 2)}px`;
    },
  });

  bindSlider(vol, { onMove: (f) => setVolume(f) });
  on(vol, 'wheel', (e) => {
    e.preventDefault();
    setVolume((media.muted ? 0 : media.volume) - Math.sign(e.deltaY) * 0.05);
  });

  // ---- auto-hiding controls ----
  let hideTimer = 0;
  let overControls = false;
  function showControls() {
    root.classList.add('active');
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (!scrubbing && !overControls) root.classList.remove('active');
    }, HIDE_DELAY_MS);
  }
  on(doc, 'pointermove', (e) => {
    overControls = !!e.target.closest?.('.top, .bottom');
    showControls();
  });
  on(doc, 'pointerdown', showControls);
  on(doc.documentElement, 'pointerleave', () => {
    if (!scrubbing) {
      overControls = false;
      root.classList.remove('active');
    }
  });

  // ---- keyboard ----
  on(doc, 'keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const d = duration();
    let handled = true;
    switch (e.key) {
      case ' ':
      case 'k':
      case 'K':
        togglePlay();
        break;
      case 'ArrowLeft':
        seekBy(-SKIP);
        break;
      case 'ArrowRight':
        seekBy(SKIP);
        break;
      case 'j':
      case 'J':
        seekBy(-10);
        break;
      case 'l':
      case 'L':
        seekBy(10);
        break;
      case 'ArrowUp':
        setVolume((media.muted ? 0 : media.volume) + 0.05);
        flash(`Volume ${Math.round(media.volume * 100)}%`);
        break;
      case 'ArrowDown':
        setVolume((media.muted ? 0 : media.volume) - 0.05);
        flash(`Volume ${Math.round(media.volume * 100)}%`);
        break;
      case 'm':
      case 'M':
        toggleMute();
        break;
      case 'c':
      case 'C':
        toggleCaptions();
        break;
      case 'f':
      case 'F':
        actions.fullscreen();
        break;
      case 's':
      case 'S':
        cycleRate();
        break;
      case '>':
      case '.':
        if (e.key === '.' && !e.shiftKey) {
          handled = false;
          break;
        }
        stepRate(1);
        break;
      case '<':
        stepRate(-1);
        break;
      case 'Escape':
        if (actions.exitFullscreen) actions.exitFullscreen();
        else handled = false;
        break;
      case 'Home':
        seekTo(0);
        break;
      case 'End':
        if (Number.isFinite(d)) seekTo(d);
        break;
      default:
        if (/^[0-9]$/.test(e.key) && Number.isFinite(d)) seekTo((Number(e.key) / 10) * d);
        else handled = false;
    }
    if (handled) {
      e.preventDefault();
      showControls();
    }
  });

  renderAll();
  showControls();

  return {
    root,
    stage,
    render: renderAll,
    destroy() {
      ac.abort();
      clearTimeout(hideTimer);
      clearTimeout(osdTimer);
    },
  };
};
