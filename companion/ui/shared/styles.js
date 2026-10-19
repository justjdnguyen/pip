'use strict';

// CSS for the PiP window.
BP.PIP_CSS = `
:root { color-scheme: dark; }
* { box-sizing: border-box; }
html, body {
  margin: 0; height: 100%; overflow: hidden; background: #000;
  font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  color: #fbfbfe; user-select: none; -webkit-user-select: none;
}
.pip {
  position: fixed; inset: 0;
  --btn: clamp(30px, 6.5vmin, 52px);
  --icon: clamp(18px, 3.8vmin, 30px);
  --pad: clamp(8px, 2vmin, 20px);
}
.pip:not(.active) { cursor: none; }

.stage { position: absolute; inset: 0; background: #000; }
.stage video {
  position: absolute !important; inset: 0 !important; left: 0 !important; top: 0 !important;
  width: 100% !important; height: 100% !important;
  max-width: none !important; max-height: none !important;
  margin: 0 !important; padding: 0 !important; transform: none !important;
  object-fit: contain !important; background: #000; opacity: 1 !important; visibility: visible !important;
}

.shade {
  position: absolute; inset: 0; pointer-events: none;
  background: rgba(0, 0, 0, .42); opacity: 0; transition: opacity .2s;
}
.top, .bottom { position: absolute; left: 0; right: 0; opacity: 0; transition: opacity .2s; }
.pip.active .shade, .pip.active .top, .pip.active .bottom { opacity: 1; }
.pip:not(.active) .top, .pip:not(.active) .bottom { pointer-events: none; }

.top { top: 0; display: flex; justify-content: space-between; padding: calc(var(--pad) * .6); }
.bottom { bottom: 0; padding: 0 var(--pad) calc(var(--pad) * .8); }

.row { display: grid; grid-template-columns: 1fr auto 1fr; align-items: center; gap: 8px; }
.time { font-size: clamp(12px, 2.4vmin, 18px); font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; }
.live-badge { display: none; font-size: 11px; font-weight: 700; letter-spacing: .05em; padding: 2px 6px; border-radius: 3px; background: #e11d48; }
.pip.live .live-badge { display: inline-block; }
.pip.live .time { display: none; }
.transport { display: flex; align-items: center; gap: clamp(4px, 3vmin, 28px); }
.right { display: flex; align-items: center; justify-content: flex-end; gap: clamp(2px, 1vmin, 8px); }

button {
  all: unset; display: inline-grid; place-items: center; flex: none;
  width: var(--btn); height: var(--btn); border-radius: calc(var(--btn) * .24);
  cursor: pointer; color: #fff; transition: background .12s;
}
button:hover { background: rgba(255, 255, 255, .16); }
button:active { background: rgba(255, 255, 255, .26); }
button:focus-visible { outline: 2px solid #fff; outline-offset: -2px; }
button svg { width: var(--icon); height: var(--icon); fill: currentColor; }
button.play svg { width: calc(var(--icon) * 1.35); height: calc(var(--icon) * 1.35); }
button[disabled] { opacity: .35; cursor: default; background: none; }
.top button { background: rgba(0, 0, 0, .35); }
.top button:hover { background: rgba(255, 255, 255, .2); }

.slider { position: relative; display: flex; align-items: center; cursor: pointer; touch-action: none; }
.slider .track { position: relative; width: 100%; height: 4px; border-radius: 2px; background: rgba(255, 255, 255, .28); transition: height .1s; }
.slider .fill, .slider .buffered { position: absolute; left: 0; top: 0; bottom: 0; border-radius: inherit; }
.slider .fill { background: #fff; }
.slider .buffered { background: rgba(255, 255, 255, .35); }
.slider .knob {
  position: absolute; top: 50%; width: 14px; height: 14px; margin: -7px 0 0 -7px;
  border-radius: 50%; background: #fff; transform: scale(0); transition: transform .1s;
  box-shadow: 0 1px 3px rgba(0,0,0,.5);
}
.slider:hover .knob, .slider.dragging .knob { transform: scale(1); }
.slider:hover .track, .slider.dragging .track { height: 6px; }

.vol { width: clamp(56px, 13vmin, 120px); height: var(--btn); }
.scrub { height: 20px; margin-top: 2px; }
.pip.live .scrub { visibility: hidden; }
.tip {
  position: absolute; bottom: 22px; transform: translateX(-50%);
  background: rgba(20, 20, 22, .9); padding: 3px 7px; border-radius: 4px;
  font-size: 12px; font-variant-numeric: tabular-nums; white-space: nowrap;
  opacity: 0; pointer-events: none;
}
.scrub:hover .tip, .scrub.dragging .tip { opacity: 1; }

.captions {
  position: absolute; left: 4%; right: 4%; bottom: 5%;
  display: flex; flex-direction: column; align-items: center; gap: .12em;
  pointer-events: none; text-align: center; transition: bottom .2s;
  font-size: clamp(14px, 5vmin, 72px); line-height: 1.3;
}
.pip.active .captions { bottom: calc(var(--btn) + var(--pad) + 30px); }
.captions span {
  background: rgba(8, 8, 8, .78); color: #fff; padding: .04em .35em;
  border-radius: 3px; white-space: pre-wrap; box-decoration-break: clone; -webkit-box-decoration-break: clone;
}

.osd {
  position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
  background: rgba(20, 20, 22, .8); padding: 8px 14px; border-radius: 8px;
  font-size: clamp(13px, 3vmin, 22px); font-variant-numeric: tabular-nums;
  opacity: 0; transition: opacity .2s; pointer-events: none;
}
.osd.show { opacity: 1; transition: none; }

@media (max-width: 440px) { .vol { display: none; } }
@media (max-width: 320px) { .time, .live-badge { display: none !important; } .back5, .fwd5 { display: none; } }
@media (max-height: 150px) { .top { display: none; } }
`;

// CSS for the in-page hover toggle (inside a closed shadow root).
BP.TOGGLE_CSS = `
:host { all: initial; }
.btn {
  position: fixed; z-index: 2147483647; display: grid; place-items: center;
  width: 36px; height: 36px; padding: 0; border: 0; border-radius: 9px;
  background: rgba(12, 12, 13, .72); color: #fff; cursor: pointer;
  box-shadow: 0 2px 10px rgba(0, 0, 0, .35);
  transform: translate(-100%, -50%);
  opacity: 0; pointer-events: none; transition: opacity .15s, background .1s;
}
.btn.visible { opacity: .9; pointer-events: auto; }
.btn.visible:hover { opacity: 1; background: rgba(48, 48, 52, .9); }
.btn svg { width: 20px; height: 20px; fill: currentColor; }
.toast {
  position: fixed; z-index: 2147483647; left: 50%; bottom: 32px; transform: translateX(-50%);
  max-width: 90vw; padding: 10px 16px; border-radius: 8px;
  background: rgba(20, 20, 22, .95); color: #fff;
  font: 13px/1.4 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  box-shadow: 0 4px 16px rgba(0, 0, 0, .4);
  opacity: 0; transition: opacity .2s; pointer-events: none;
}
.toast.show { opacity: 1; }
`;

// CSS for the placeholder left in the page while the video is popped out.
BP.PLACEHOLDER_CSS = `
:host { all: initial; }
.ph {
  position: absolute; inset: 0; display: flex; flex-direction: column;
  align-items: center; justify-content: center; gap: 14px;
  background: #0c0c0d; color: #fbfbfe; cursor: pointer; text-align: center;
  font: 14px/1.4 system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
  container-type: size;
}
.ph svg { width: clamp(32px, 14cqh, 88px); height: auto; fill: currentColor; }
.ph p { margin: 0; padding: 0 16px; }
.ph small { opacity: .6; font-size: 12px; }
.ph:hover small { opacity: .9; }
`;
