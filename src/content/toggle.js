'use strict';

// The Firefox-style "Watch in Picture-in-Picture" button that appears on the
// right edge of a video when you hover it.

BP.createToggle = () => {
  const MIN_W = 180;
  const MIN_H = 100;

  const host = document.createElement('better-pip-toggle');
  const shadow = host.attachShadow({ mode: 'closed' });
  BP.adoptCss(shadow, BP.TOGGLE_CSS);

  const btn = BP.h(
    document,
    'button',
    { class: 'btn', title: 'Watch in Picture-in-Picture (Alt+P)', 'aria-label': 'Watch in Picture-in-Picture' },
    [BP.icon(document, 'pip')],
  );
  const toast = BP.h(document, 'div', { class: 'toast', role: 'status' });
  shadow.append(btn, toast);

  let target = null;
  let pointer = null;
  let frame = 0;

  const rectOf = (v) => BP.adapters.find(v).rect(v);

  const eligible = (v) => {
    const r = rectOf(v);
    if (r.width < MIN_W || r.height < MIN_H) return null;
    // Skip GIF-style clips (short, muted, looping), like Firefox does.
    if (v.loop && v.muted && Number.isFinite(v.duration) && v.duration < 45) return null;
    return r;
  };

  const videoAt = (x, y) => {
    let found = null;
    let area = 0;
    for (const v of document.querySelectorAll('video')) {
      const r = eligible(v);
      if (!r) continue;
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom && r.width * r.height > area) {
        found = v;
        area = r.width * r.height;
      }
    }
    return found;
  };

  const mountPoint = () => {
    const fs = document.fullscreenElement;
    return fs && fs.tagName !== 'VIDEO' ? fs : document.documentElement;
  };

  function place() {
    if (!target) return;
    const r = rectOf(target);
    const right = Math.min(r.right, innerWidth) - 12;
    const midY = (Math.max(r.top, 0) + Math.min(r.bottom, innerHeight)) / 2;
    btn.style.left = `${right}px`;
    btn.style.top = `${midY}px`;
  }

  function show(v) {
    // Start the companion app now, so it's ready by the time they click.
    BP.AppPip?.warmUp(v);
    const parent = mountPoint();
    if (host.parentNode !== parent) parent.append(host);
    target = v;
    place();
    btn.classList.add('visible');
  }

  function hide() {
    target = null;
    btn.classList.remove('visible');
  }

  function update() {
    frame = 0;
    if (!pointer) return hide();
    // Don't offer it while the native or our own PiP is already showing this one.
    const v = videoAt(pointer.x, pointer.y);
    if (!v || document.pictureInPictureElement === v || BP.AppPip?.has(v)) return hide();
    show(v);
  }

  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(update);
  };

  document.addEventListener(
    'pointermove',
    (e) => {
      if (e.pointerType === 'touch') return;
      pointer = { x: e.clientX, y: e.clientY };
      schedule();
    },
    { capture: true, passive: true },
  );
  document.documentElement.addEventListener('pointerleave', () => {
    pointer = null;
    schedule();
  });
  addEventListener('scroll', () => target && place(), { capture: true, passive: true });
  addEventListener('resize', () => target && place(), { passive: true });
  document.addEventListener('fullscreenchange', () => {
    hide();
    schedule();
  });

  // Keep the page from reacting to clicks on our button (e.g. pausing the video).
  for (const type of ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'dblclick']) {
    btn.addEventListener(type, (e) => e.stopPropagation());
  }
  btn.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    const v = target;
    hide();
    if (v) BP.Pip.open(v);
  });

  let toastTimer = 0;
  BP.toast = (text) => {
    const parent = mountPoint();
    if (host.parentNode !== parent) parent.append(host);
    toast.textContent = text;
    toast.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toast.classList.remove('show'), 4000);
  };

  return { refresh: schedule, hide };
};
