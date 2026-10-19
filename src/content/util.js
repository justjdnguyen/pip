'use strict';

// Shared namespace for all content scripts (they run in the same isolated world).
var BP = globalThis.BP || (globalThis.BP = {});

const SVG_NS = 'http://www.w3.org/2000/svg';

// 24x24 icons. Strings are path data; objects are text labels.
// Built with createElementNS (never innerHTML) so pages enforcing
// Trusted Types, like YouTube, can't block them.
BP.ICONS = {
  play: ['M8 5v14l11-7z'],
  pause: ['M6 19h4V5H6v14zm8-14v14h4V5h-4z'],
  close: ['M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z'],
  back5: [
    'M12 5V1L7 6l5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6H4c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z',
    { text: '5', x: 12, y: 16.2 },
  ],
  fwd5: [
    { d: 'M12 5V1L7 6l5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6H4c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z', mirror: true },
    { text: '5', x: 12, y: 16.2 },
  ],
  volumeHigh: ['M3 9v6h4l5 5V4L7 9H3zm13.5 3c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z'],
  volumeLow: ['M18.5 12c0-1.77-1.02-3.29-2.5-4.03v8.05c1.48-.73 2.5-2.25 2.5-4.02zM5 9v6h4l5 5V4L9 9H5z'],
  volumeOff: ['M16.5 12c0-1.77-1.02-3.29-2.5-4.03v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51C20.63 14.91 21 13.5 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3 3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06c1.38-.31 2.63-.95 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4 9.91 6.09 12 8.18V4z'],
  ccOn: ['M19 4H5c-1.11 0-2 .9-2 2v12c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm-8 7H9.5v-.5h-2v3h2V13H11v1c0 .55-.45 1-1 1H7c-.55 0-1-.45-1-1v-4c0-.55.45-1 1-1h3c.55 0 1 .45 1 1v1zm7 0h-1.5v-.5h-2v3h2V13H18v1c0 .55-.45 1-1 1h-3c-.55 0-1-.45-1-1v-4c0-.55.45-1 1-1h3c.55 0 1 .45 1 1v1z'],
  ccOff: ['M19 4H5c-1.11 0-2 .9-2 2v12c0 1.1.89 2 2 2h14c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 14H5V6h14v12zM7 15h3c.55 0 1-.45 1-1v-1H9.5v.5h-2v-3h2v.5H11v-1c0-.55-.45-1-1-1H7c-.55 0-1 .45-1 1v4c0 .55.45 1 1 1zm7 0h3c.55 0 1-.45 1-1v-1h-1.5v.5h-2v-3h2v.5H18v-1c0-.55-.45-1-1-1h-3c-.55 0-1 .45-1 1v4c0 .55.45 1 1 1z'],
  fullscreen: ['M7 14H5v5h5v-2H7v-3zm-2-4h2V7h3V5H5v5zm12 7h-3v2h5v-5h-2v3zM14 5v2h3v3h2V5h-5z'],
  returnToTab: [
    'M19 4H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h4v-2H5V6h14v12h-4v2h4a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2z',
    'M12 10l-4 4h3v6h2v-6h3z',
  ],
  pip: [
    'M19 4H5a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2zm0 14H5V6h14z',
    'M12 11h6v6h-6z',
  ],
};

BP.icon = (doc, name) => {
  const svg = doc.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  for (const part of BP.ICONS[name]) {
    if (typeof part === 'object' && part.text) {
      const t = doc.createElementNS(SVG_NS, 'text');
      t.setAttribute('x', part.x);
      t.setAttribute('y', part.y);
      t.setAttribute('text-anchor', 'middle');
      t.setAttribute('font-size', '7');
      t.setAttribute('font-weight', '700');
      t.setAttribute('font-family', 'system-ui, sans-serif');
      t.textContent = part.text;
      svg.append(t);
      continue;
    }
    const p = doc.createElementNS(SVG_NS, 'path');
    p.setAttribute('d', typeof part === 'string' ? part : part.d);
    if (part.mirror) p.setAttribute('transform', 'matrix(-1 0 0 1 24 0)');
    svg.append(p);
  }
  return svg;
};

// Tiny element builder: h(doc, 'div', { class: 'x', text: 'hi', title: 't' }, children)
BP.h = (doc, tag, props = {}, children = []) => {
  const el = doc.createElement(tag);
  for (const [k, v] of Object.entries(props)) {
    if (v == null) continue;
    if (k === 'class') el.className = v;
    else if (k === 'text') el.textContent = v;
    else el.setAttribute(k, v);
  }
  el.append(...children);
  return el;
};

BP.formatTime = (sec) => {
  if (!Number.isFinite(sec) || sec < 0) sec = 0;
  sec = Math.floor(sec);
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = String(sec % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
};

BP.clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

// Attach CSS to a document or shadow root. Constructable stylesheets aren't
// blocked by a page's style-src CSP; <style> is the fallback.
BP.adoptCss = (target, css, realm = window) => {
  try {
    const sheet = new realm.CSSStyleSheet();
    sheet.replaceSync(css);
    target.adoptedStyleSheets = [...target.adoptedStyleSheets, sheet];
  } catch {
    const doc = target.ownerDocument || target;
    const style = doc.createElement('style');
    style.textContent = css;
    (target.head || target).append(style);
  }
};

BP.settings = {
  width: 480,
  // 'auto': use the companion app when it's installed, else Chrome's PiP window.
  // 'chrome': always use Chrome's PiP window.
  engine: 'auto',
  async load() {
    try {
      const { pipWidth, engine } = await chrome.storage.local.get(['pipWidth', 'engine']);
      if (Number.isFinite(pipWidth)) this.width = pipWidth;
      if (engine === 'auto' || engine === 'chrome') this.engine = engine;
    } catch {}
  },
  saveWidth(w) {
    this.width = w;
    chrome.storage.local.set({ pipWidth: w }).catch(() => {});
  },
};
