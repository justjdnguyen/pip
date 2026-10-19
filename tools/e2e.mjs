// Smoke test: loads the extension in a Chromium browser, hovers a video,
// clicks the PiP toggle, and screenshots the page and the PiP window.
// Usage: node tools/e2e.mjs [/path/to/browser/binary]
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import http from 'node:http';
import { readFile } from 'node:fs/promises';

const ROOT = resolve(import.meta.dirname, '..');
const OUT = join(ROOT, 'test', 'out');
mkdirSync(OUT, { recursive: true });
const BIN = process.argv[2] || '/Applications/Helium.app/Contents/MacOS/Helium';
const PORT = 9333, HTTP = 8765;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const server = http.createServer(async (req, res) => {
  try {
    const p = join(ROOT, decodeURIComponent(new URL(req.url, 'http://x').pathname));
    const body = await readFile(p);
    const type = p.endsWith('.html') ? 'text/html' : p.endsWith('.vtt') ? 'text/vtt' : 'application/octet-stream';
    res.writeHead(200, { 'content-type': type, 'access-control-allow-origin': '*' }).end(body);
  } catch { res.writeHead(404).end(); }
}).listen(HTTP);

const profile = mkdtempSync(join(tmpdir(), 'bp-profile-'));
// APP=1: register the companion app (debug build) with this test profile.
if (process.env.APP) {
  mkdirSync(join(profile, 'NativeMessagingHosts'), { recursive: true });
  writeFileSync(join(profile, 'NativeMessagingHosts', 'com.betterpip.host.json'), JSON.stringify({
    name: 'com.betterpip.host', description: 'test', type: 'stdio',
    path: join(ROOT, `companion/src-tauri/target/${process.env.DEBUG_APP ? 'debug' : 'release'}/better-pip-host`),
    allowed_origins: ['chrome-extension://fblfebboimckibidkphfchjlmohondgm/'],
  }));
}
const browser = spawn(BIN, [
  `--user-data-dir=${profile}`, `--load-extension=${ROOT}`, `--disable-extensions-except=${ROOT}`,
  `--remote-debugging-port=${PORT}`, '--no-first-run', '--no-default-browser-check',
  '--autoplay-policy=no-user-gesture-required', ...(process.env.BIG ? ['--window-size=2200,1400'] : []),
  process.env.PAGE || `http://localhost:${HTTP}/test/index.html${process.env.SRC ? '?src=' + encodeURIComponent(process.env.SRC) : ''}`,
], { stdio: 'ignore', env: { ...process.env, BP_DEBUG_LOG: process.env.APP ? join(OUT, 'app.log') : '', ...(process.env.APP ? { BP_DEBUG_SHOW_CONTROLS: '1' } : {}) } });

async function targets() {
  for (let i = 0; i < 50; i++) {
    try { return await (await fetch(`http://127.0.0.1:${PORT}/json`)).json(); } catch { await sleep(200); }
  }
  throw new Error('no devtools');
}
function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let id = 0; const pending = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); setTimeout(() => pending.has(i) && (pending.delete(i), r({ error: 'timeout' })), 5000); try { ws.send(JSON.stringify({ id: i, method, params })); } catch (e) { r({ error: String(e) }); } });
  return new Promise((r) => (ws.onopen = () => r({ send, ws })));
}
const evaluate = async (c, expr) => {
  const r = await c.send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.error || r.result?.exceptionDetails) return 'ERROR ' + JSON.stringify(r.error || r.result.exceptionDetails.exception?.description);
  return r.result?.result?.value;
};
const shot = async (c, name) => {
  const r = await c.send('Page.captureScreenshot', { format: 'png' });
  if (r.result) writeFileSync(join(OUT, name), Buffer.from(r.result.data, 'base64'));
  else console.log('screenshot failed', name, JSON.stringify(r.error));
};
const mouse = (c, type, x, y) => c.send('Input.dispatchMouseEvent', { type, x, y, button: type === 'mouseMoved' ? 'none' : 'left', clickCount: 1 });

try {
  await sleep(process.env.PAGE ? 8000 : 2500);
  // Wait for the extension's background worker so the content script is in place.
  for (let i = 0; i < 20 && !(await targets()).some((t) => t.type === 'service_worker' && t.url.startsWith('chrome-extension://fblfeb')); i++) await sleep(500);
  let page = (await targets()).find((t) => t.type === 'page' && t.url.includes(process.env.PAGE ? new URL(process.env.PAGE).hostname : '/test/index.html'));
  const SEL = process.env.PAGE ? 'video' : '#v';
  const c = await connect(page.webSocketDebuggerUrl);
  await c.send('Page.enable');
  await c.send('Runtime.enable');
  c.ws.addEventListener('message', (m) => { const d = JSON.parse(m.data); if (d.method === 'Runtime.consoleAPICalled' && /PiP/.test(JSON.stringify(d.params.args))) console.log('console:', d.params.args.map(a => a.value ?? a.description).join(' ')); if (d.method === 'Runtime.exceptionThrown') console.log('exception:', d.params.exceptionDetails.exception?.description?.slice(0, 200)); });
  await c.send('Page.bringToFront');
  console.log('ready:', await evaluate(c, `new Promise(r => { const v = document.querySelector('${SEL}'); const go = () => { v.play().then(() => r('playing ' + v.duration.toFixed(1) + 's'), e => r('play failed ' + e)); }; v.readyState > 2 ? go() : v.addEventListener('canplay', go, { once: true }); setTimeout(() => r('timeout readyState=' + v.readyState), 8000); })`));
  const rect = await evaluate(c, `JSON.stringify((document.querySelector('${SEL}').closest('.html5-video-player') || document.querySelector('${SEL}')).getBoundingClientRect())`);
  const r = JSON.parse(rect);
  await mouse(c, 'mouseMoved', r.x + r.width / 2, r.y + r.height / 2);
  await sleep(Number(process.env.HOVER_MS || 400));
  await shot(c, '1-hover-toggle.png');

  // Click the toggle: anchored 12px inside the right edge, centred vertically.
  const tx = r.x + r.width - 12 - 18, ty = r.y + r.height / 2;
  await mouse(c, 'mouseMoved', tx, ty);
  await sleep(200);
  console.log('under cursor:', await evaluate(c, `document.elementFromPoint(${tx}, ${ty})?.tagName`));
  await mouse(c, 'mousePressed', tx, ty);
  await mouse(c, 'mouseReleased', tx, ty);
  await sleep(1500);
  await shot(c, '2-page-placeholder.png');
  console.log('page state:', await evaluate(c, `JSON.stringify({ placeholder: !!document.querySelector('better-pip-placeholder'), videosInPage: document.querySelectorAll('video').length, pip: !!window.documentPictureInPicture?.window })`));

  const all = await targets();
  console.log('targets:', all.map((t) => `${t.type} ${t.url}`).join(' | '));
  const pipT = all.find((t) => t.type === 'page' && t.id !== page.id);
  if (pipT) {
    const p = await connect(pipT.webSocketDebuggerUrl);
    await p.send('Page.enable');
    await sleep(800);
    await shot(p, '3-pip-idle.png');
    if (process.env.RESIZE_EXPERIMENT) {
      const sz = () => evaluate(p, `JSON.stringify([innerWidth, innerHeight])`);
      // Stand-in for the user dragging the window edge wider.
      await p.send('Runtime.evaluate', { expression: 'resizeTo(700, outerHeight)', userGesture: true });
      await sleep(500);
      console.log('after drag:', await sz());
      await mouse(p, 'mousePressed', 200, 150);
      await mouse(p, 'mouseReleased', 200, 150);
      await sleep(500);
      console.log('after click (snapped):', await sz());
    }
    const size = JSON.parse(await evaluate(p, `JSON.stringify({ w: innerWidth, h: innerHeight })`));
    await mouse(p, 'mouseMoved', size.w / 2, size.h / 2);
    await sleep(300);
    await mouse(p, 'mouseMoved', size.w / 2 + 5, size.h / 2);
    await sleep(300);
    await shot(p, '4-pip-controls.png');
    console.log('pip state:', await evaluate(p, `JSON.stringify({ size: [innerWidth, innerHeight], video: !!document.querySelector('video'), paused: document.querySelector('video')?.paused, captionsBtnDisabled: document.querySelector('.cc')?.disabled, time: document.querySelector('.time')?.textContent })`));
    // Turn on captions via the CC button.
    const cc = JSON.parse(await evaluate(p, `JSON.stringify(document.querySelector('.cc').getBoundingClientRect())`));
    await mouse(p, 'mousePressed', cc.x + cc.width / 2, cc.y + cc.height / 2);
    await mouse(p, 'mouseReleased', cc.x + cc.width / 2, cc.y + cc.height / 2);
    await evaluate(p, `(() => { const v = document.querySelector('video'); if (!location.href.startsWith('http')) {} v.currentTime = ${process.env.PAGE ? 'v.currentTime' : 0.5}; })()`);
    await sleep(800);
    await shot(p, '5-pip-captions.png');
    console.log('captions:', await evaluate(p, `document.querySelector('.captions').textContent`));
    // Back to tab.
    const back = JSON.parse(await evaluate(p, `JSON.stringify(document.querySelector('.return').getBoundingClientRect())`));
    await mouse(p, 'mouseMoved', back.x + back.width / 2, back.y + back.height / 2);
    await mouse(p, 'mousePressed', back.x + back.width / 2, back.y + back.height / 2);
    await mouse(p, 'mouseReleased', back.x + back.width / 2, back.y + back.height / 2);
    await sleep(800);
    console.log('hit test:', await evaluate(p, `document.elementFromPoint(${back.x + back.width / 2}, ${back.y + back.height / 2})?.outerHTML.slice(0, 80)`), JSON.stringify(back));
    if (process.env.JSCLICK) await evaluate(p, `document.querySelector('.return').click()`);
    await sleep(500);
    console.log('pip after click:', await evaluate(p, `JSON.stringify({ active: document.querySelector('.pip')?.className, video: !!document.querySelector('video') })`));
    console.log('page after click:', await evaluate(c, `JSON.stringify({ videos: [...document.querySelectorAll('video')].map(v => v.id), ph: !!document.querySelector('better-pip-placeholder'), pipWin: !!documentPictureInPicture.window })`));
    console.log('after return:', await evaluate(c, `JSON.stringify({ placeholder: !!document.querySelector('better-pip-placeholder'), videoBack: document.querySelector('.player video') === document.getElementById('v'), paused: document.getElementById('v').paused, track: document.getElementById('v').textTracks[0].mode })`));
    await shot(c, '6-page-after-return.png');
  } else {
    console.log('No PiP window target found.');
  }
} finally {
  // HOLD=seconds keeps the browser (and any app windows) open for inspection.
  if (process.env.HOLD) await sleep(Number(process.env.HOLD) * 1000);
  browser.kill();
  server.close();
}
