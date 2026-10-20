// Regression coverage for the media-driven background caption clock.
// Run: node --test tools/test-youtube-captions.mjs
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import test from 'node:test';

const read = (name) => readFileSync(new URL(`../src/content/${name}`, import.meta.url), 'utf8');

function setup() {
  const document = new EventTarget();
  document.hidden = false;
  const video = new EventTarget();
  const frames = new Map();
  const errors = [];
  let nextId = 0;
  const window = {
    requestAnimationFrame(callback) {
      if (typeof callback !== 'function') throw new TypeError('Invalid callback');
      frames.set(++nextId, callback);
      return nextId;
    },
    cancelAnimationFrame: (id) => frames.delete(Number(id)),
    reportError: (err) => errors.push(err),
  };
  let observer;
  let text = '';
  let enabled = true;
  let windows = null;
  const button = { getAttribute: (name) => name === 'aria-pressed' ? String(enabled) : null };
  const player = {
    querySelector: () => button,
    querySelectorAll: () => windows || [{ querySelectorAll: () => [{ textContent: text }] }],
  };
  const BP = {};
  const context = {
    window, document, Event, performance: { now: () => 1234 }, BP,
    getComputedStyle: (element) => ({ display: 'block', visibility: 'visible', overflowY: 'visible', ...element.style }),
    MutationObserver: class {
      constructor(cb) { observer = cb; }
      observe() {}
      disconnect() { observer = () => {}; }
    },
  };
  runInNewContext(read('youtube-caption-clock.js'), context);
  runInNewContext(read('adapters.js'), context);
  const captions = BP.adapters.list[0].captions(video, { player });
  const lines = [];
  captions.onChange((value) => lines.push(Array.from(value)));
  function renderCaption() {
    text = `Caption at ${video.currentTime}`;
    observer();
    window.requestAnimationFrame(renderCaption);
  }
  return {
    window, document, video, frames, errors, captions, lines, renderCaption,
    tick: () => video.dispatchEvent(new Event('timeupdate')),
    setEnabled: (value) => { enabled = value; },
    setWindows: (value) => { windows = value; observer(); },
    paint() {
      for (const [id, callback] of [...frames]) {
        if (!frames.delete(id)) continue;
        callback(1234);
      }
    },
  };
}

test('captions advance while hidden and return to the native clock in sync', () => {
  const s = setup();
  s.window.requestAnimationFrame(s.renderCaption);
  s.video.currentTime = 1;
  s.paint();
  assert.deepEqual(s.lines.at(-1), ['Caption at 1']);
  s.document.hidden = true;
  for (const time of [2, 3, 50]) {
    s.video.currentTime = time;
    s.tick();
    assert.deepEqual(s.lines.at(-1), [`Caption at ${time}`]);
    assert.equal(s.frames.size, 1, 'no duplicate native callbacks after a background tick');
  }
  s.document.hidden = false;
  s.video.currentTime = 51;
  s.tick();
  assert.deepEqual(s.lines.at(-1), ['Caption at 50']);
  s.paint();
  assert.deepEqual(s.lines.at(-1), ['Caption at 51']);
});

test('no background work when captions are off or the session is destroyed', () => {
  const s = setup();
  s.document.hidden = true;
  let calls = 0;
  s.window.requestAnimationFrame(() => calls++);
  s.setEnabled(false);
  s.tick();
  assert.equal(calls, 0);
  s.setEnabled(true);
  s.captions.destroy();
  s.tick();
  s.video.dispatchEvent(new Event('seeked'));
  s.document.dispatchEvent(new Event('visibilitychange'));
  assert.equal(calls, 0);
  s.document.hidden = false;
  s.paint();
  assert.equal(calls, 1, 'closing PiP preserves queued native work');
});

test('cancellation, shared timestamps, callback errors and next-frame scheduling', () => {
  const s = setup();
  s.document.hidden = true;
  const calls = [];
  let cancelled;
  s.window.requestAnimationFrame(function (time) {
    assert.equal(this, s.window);
    calls.push(time);
    s.window.cancelAnimationFrame(String(cancelled));
    s.window.requestAnimationFrame((time) => calls.push(time));
    throw new Error('renderer failed');
  });
  cancelled = s.window.requestAnimationFrame(() => assert.fail('cancelled callback ran'));
  s.window.requestAnimationFrame((time) => calls.push(time));
  s.tick();
  assert.deepEqual(calls, [1234, 1234]);
  assert.equal(s.errors.length, 1);
  s.tick();
  assert.deepEqual(calls, [1234, 1234, 1234]);
  assert.equal(s.frames.size, 0);
});

test('a hidden-tab seek advances captions without waiting for a timer', () => {
  const s = setup();
  s.document.hidden = true;
  s.window.requestAnimationFrame(s.renderCaption);
  s.video.currentTime = 120;
  s.video.dispatchEvent(new Event('seeked'));
  assert.deepEqual(s.lines.at(-1), ['Caption at 120']);
});

test('roll-up history is excluded and style-only expiry clears the caption', () => {
  const s = setup();
  const line = (textContent, top) => ({
    textContent,
    getBoundingClientRect: () => ({ top, bottom: top + 20, width: 300, height: 20 }),
  });
  const win = {
    style: { overflowY: 'hidden' },
    getBoundingClientRect: () => ({ top: 100, bottom: 140, width: 300, height: 40 }),
    querySelectorAll: () => [line('expired first line', 40), line('expired second line', 60),
      line('expired third line', 80), line('current first line', 100), line('current second line', 120)],
  };
  s.setWindows([win]);
  assert.deepEqual(s.lines.at(-1), ['current first line', 'current second line']);
  win.style.display = 'none';
  s.tick();
  assert.deepEqual(s.lines.at(-1), [], 'clear text when YouTube hides the window without removing it');
});
