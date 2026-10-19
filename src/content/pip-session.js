'use strict';

// Chrome PiP mode: opens a Document Picture-in-Picture window, moves the
// page's real <video> into it, and draws the shared controls around it. When
// the window closes, the video goes back exactly where it was.
// BP.Pip.open() also decides whether to use the companion app instead.

// The "This video is playing in Picture-in-Picture mode" box left in the page.
//   slot:      adapter-provided { parent, before, zIndex } to cover (e.g. YouTube's player)
//   keepVideo: the video stays in the page (companion app mode), so cover it
//              instead of taking its place
BP.mountPlaceholder = ({ video, slot, keepVideo, onActivate }) => {
  const host = document.createElement('better-pip-placeholder');
  const root = host.attachShadow({ mode: 'closed' });
  BP.adoptCss(root, BP.PLACEHOLDER_CSS);
  root.append(
    BP.h(document, 'div', { class: 'ph', role: 'button', title: 'Return video to this tab' }, [
      BP.icon(document, 'pip'),
      BP.h(document, 'p', { text: 'This video is playing in Picture-in-Picture mode.' }),
      BP.h(document, 'small', { text: 'Click to bring it back' }),
    ]),
  );
  for (const type of ['pointerdown', 'mousedown', 'mouseup', 'dblclick']) {
    host.addEventListener(type, (e) => e.stopPropagation());
  }
  host.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    onActivate();
  });

  const fill = { position: 'absolute', inset: '0', width: '100%', height: '100%', display: 'block' };
  const cleanups = [];

  if (slot) {
    Object.assign(host.style, fill, { zIndex: slot.zIndex ?? '' });
    slot.parent.insertBefore(host, slot.before);
  } else if (keepVideo) {
    // Cover the video where it is, tracking its size and position.
    const z = parseInt(getComputedStyle(video).zIndex, 10);
    Object.assign(host.style, { position: 'absolute', display: 'block', zIndex: Number.isFinite(z) ? z + 1 : '' });
    video.after(host);
    const sync = () =>
      Object.assign(host.style, {
        left: `${video.offsetLeft}px`,
        top: `${video.offsetTop}px`,
        width: `${video.offsetWidth}px`,
        height: `${video.offsetHeight}px`,
      });
    sync();
    const ro = new ResizeObserver(sync);
    ro.observe(video);
    addEventListener('resize', sync);
    cleanups.push(() => ro.disconnect(), () => removeEventListener('resize', sync));
  } else {
    // Take the video's place.
    const geometry = { left: video.offsetLeft, top: video.offsetTop, width: video.offsetWidth, height: video.offsetHeight };
    const pos = getComputedStyle(video).position;
    if (pos === 'absolute' || pos === 'fixed') {
      Object.assign(host.style, fill);
    } else {
      const rect = video.getBoundingClientRect();
      Object.assign(host.style, {
        position: 'relative',
        display: 'block',
        width: `${rect.width}px`,
        height: `${rect.height}px`,
        maxWidth: '100%',
      });
    }
    video.before(host);
    // Call after the video has moved out: if filling the parent collapsed the
    // placeholder (parent has no size of its own), pin it where the video was.
    host.afterVideoMoved = () => {
      if (host.getBoundingClientRect().height < 8 && geometry.height >= 8) {
        Object.assign(host.style, {
          left: `${geometry.left}px`,
          top: `${geometry.top}px`,
          width: `${geometry.width}px`,
          height: `${geometry.height}px`,
          inset: 'auto',
        });
      }
    };
  }

  return {
    el: host,
    replacesVideo: !slot && !keepVideo,
    afterVideoMoved: () => host.afterVideoMoved?.(),
    remove() {
      cleanups.forEach((f) => f());
      host.remove();
    },
  };
};

BP.Pip = (() => {
  let session = null;

  const canUseDocumentPip = () => 'documentPictureInPicture' in window && window.top === window;

  function initialSize(video) {
    const r = video.getBoundingClientRect();
    const aspect =
      video.videoWidth && video.videoHeight
        ? video.videoWidth / video.videoHeight
        : r.width && r.height
          ? r.width / r.height
          : 16 / 9;
    const width = Math.round(BP.clamp(BP.settings.width, 240, screen.availWidth));
    return { width, height: Math.round(width / aspect) };
  }

  async function openNative(video) {
    try {
      video.disablePictureInPicture = false;
      await video.requestPictureInPicture();
      return true;
    } catch (err) {
      console.warn('[Better PiP] Could not open Picture-in-Picture:', err);
      BP.toast?.('Couldn’t open Picture-in-Picture here. Try the button on the video, or press Alt+P.');
      return false;
    }
  }

  async function open(video) {
    if (!video) return false;
    if (BP.AppPip?.has(video)) {
      BP.AppPip.close(video, { focus: false });
      return true;
    }
    if (session) {
      const same = session.video === video;
      session.close({ pause: false });
      if (same) return true;
    }

    // After the extension is reloaded or updated, scripts already in open tabs
    // are cut off from it (and from the app) until the page reloads.
    if (!chrome.runtime?.id) {
      BP.toast?.('Better PiP was updated. Reload this page to use it.');
      return true;
    }

    // Prefer the companion app (unlimited windows, any size, no title bar).
    if (BP.AppPip && (await BP.AppPip.open(video, initialSize(video)))) return true;

    if (document.pictureInPictureElement) {
      await document.exitPictureInPicture().catch(() => {});
    }
    if (!canUseDocumentPip()) return openNative(video);

    let win;
    try {
      win = await window.documentPictureInPicture.requestWindow({
        ...initialSize(video),
        // We draw our own "back to tab" button; this removes Chrome's duplicate from the title bar.
        disallowReturnToOpener: true,
      });
    } catch (err) {
      console.warn('[Better PiP] Document PiP failed, using native PiP:', err);
      return openNative(video);
    }
    session = createSession(video, win);
    return true;
  }

  function createSession(video, win) {
    const doc = win.document;
    const ac = new AbortController();
    const on = (target, type, fn, opts = {}) => target.addEventListener(type, fn, { ...opts, signal: ac.signal });

    const adapter = BP.adapters.find(video);
    const ctx = adapter.capture(video);
    const origParent = video.parentNode;
    const origNext = video.nextSibling;
    const origControls = video.controls;

    const s = { video, close, closed: false };

    const placeholder = BP.mountPlaceholder({
      video,
      slot: adapter.placeholderSlot(video, ctx),
      onActivate: () => close({ pause: false, focus: true }),
    });

    doc.title = document.title || 'Picture-in-Picture';
    BP.adoptCss(doc, BP.PIP_CSS, win);

    const captions = adapter.captions(video, ctx);
    const ui = BP.createPlayerUI({
      doc,
      win,
      media: BP.localMedia(video, ac.signal),
      captions,
      actions: {
        returnToTab: () => close({ pause: false, focus: true }),
        close: () => close({ pause: true }),
        fullscreen: goFullscreen,
      },
    });

    // Move the real video element. Doing it in one step keeps it playing.
    video.controls = false;
    ui.stage.append(video);
    placeholder.afterVideoMoved();

    on(ui.stage, 'dblclick', goFullscreen);

    function goFullscreen() {
      // Put the video back first, then ask the page to go fullscreen while we
      // still have the click's user activation.
      close({ pause: false, focus: true });
      Promise.resolve()
        .then(() => adapter.fullscreen(video, ctx))
        .catch((err) => console.warn('[Better PiP] Fullscreen was blocked:', err));
    }

    // ---- keep the window the same shape as the video ----
    // Chrome only lets the window resize itself right after a click or key
    // press inside it (one resize per press), so after the user drags the
    // window edge we snap it back to the video's shape on their next press.
    const videoAspect = () => (video.videoWidth && video.videoHeight ? video.videoWidth / video.videoHeight : 0);
    // Size the last time the window matched the video's shape.
    let lastSize = { w: win.innerWidth, h: win.innerHeight };
    function snapToAspect() {
      const a = videoAspect();
      if (!a) return;
      const w0 = win.innerWidth;
      const h0 = win.innerHeight;
      if (Math.abs(w0 / a - h0) <= 2) return;
      // Keep whichever side the user changed more.
      const keepWidth = Math.abs(w0 - lastSize.w) / lastSize.w >= Math.abs(h0 - lastSize.h) / lastSize.h;
      let w = keepWidth ? w0 : Math.round(h0 * a);
      let hgt = keepWidth ? Math.round(w0 / a) : h0;
      const frameW = win.outerWidth - win.innerWidth;
      const frameH = win.outerHeight - win.innerHeight;
      const maxW = win.screen.availWidth - frameW;
      const maxH = win.screen.availHeight - frameH;
      if (w > maxW) (w = maxW), (hgt = Math.round(w / a));
      if (hgt > maxH) (hgt = maxH), (w = Math.round(hgt * a));
      try {
        win.resizeTo(w + frameW, hgt + frameH);
        lastSize = { w, h: hgt };
      } catch {}
    }
    // Not on the fullscreen button: fullscreen needs that same press.
    const isFullscreenPress = (e) => !!e.target.closest?.('.fs') || e.key === 'f' || e.key === 'F';
    on(doc, 'pointerdown', (e) => isFullscreenPress(e) || snapToAspect(), { capture: true });
    on(doc, 'keydown', (e) => isFullscreenPress(e) || snapToAspect(), { capture: true });

    // ---- remember the window size ----
    let resizeTimer = 0;
    on(win, 'resize', () => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => BP.settings.saveWidth(win.innerWidth), 400);
    });

    // ---- teardown ----
    function restore() {
      if (s.closed) return;
      s.closed = true;
      ac.abort();
      ui.destroy();
      clearTimeout(resizeTimer);
      captions.destroy();
      video.controls = origControls;

      if (placeholder.el.isConnected && placeholder.replacesVideo) {
        placeholder.el.replaceWith(video);
      } else if (origParent.isConnected) {
        origParent.insertBefore(video, origNext?.parentNode === origParent ? origNext : null);
      } else {
        // The page rebuilt its player while we were away; nothing to put it back into.
        video.pause();
      }
      placeholder.remove();
      if (session === s) session = null;
      BP.toggle?.refresh();
    }

    function close({ pause = false, focus = false } = {}) {
      if (pause) video.pause();
      restore();
      try {
        win.close();
      } catch {}
      if (focus) chrome.runtime.sendMessage({ type: 'focus-tab' }).catch(() => {});
    }

    // Window closed by the browser (its own close button, or another PiP
    // window replacing this one).
    win.addEventListener('pagehide', restore, { once: true });
    // If the page itself goes away, the window closes with it.
    on(window, 'pagehide', () => close());

    return s;
  }

  // Pick the video the user most likely means: playing first, then biggest on screen.
  function bestVideo() {
    let best = null;
    let bestScore = 0;
    for (const v of document.querySelectorAll('video')) {
      const r = v.getBoundingClientRect();
      const w = Math.min(r.right, innerWidth) - Math.max(r.left, 0);
      const hgt = Math.min(r.bottom, innerHeight) - Math.max(r.top, 0);
      const visible = Math.max(0, w) * Math.max(0, hgt);
      if (!visible && !(r.width * r.height)) continue;
      const playing = !v.paused && !v.ended && v.readyState > 2;
      const score = visible + r.width * r.height * 0.1 + (playing ? 1e8 : 0) + (v.currentTime > 0 ? 1e7 : 0);
      if (score > bestScore) {
        best = v;
        bestScore = score;
      }
    }
    return best;
  }

  // Toggle for keyboard shortcut / toolbar button. Returns whether anything happened.
  function toggleBest() {
    if (session) {
      session.close({ pause: false, focus: true });
      return true;
    }
    if (BP.AppPip?.count()) {
      BP.AppPip.closeAll();
      return true;
    }
    if (document.pictureInPictureElement) {
      document.exitPictureInPicture().catch(() => {});
      return true;
    }
    const v = bestVideo();
    if (!v) return false;
    open(v);
    return true;
  }

  return {
    open,
    toggleBest,
    isOpen: () => !!session,
  };
})();
