// Measures PiP startup: opens and closes PiP on a page several times (Alt+P)
// and prints, from the app's debug log, how long each step took.
// Usage: node tools/startup-timing.mjs [url] [runs]
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, mkdirSync, readFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const OUT = join(ROOT, 'test', 'out');
mkdirSync(OUT, { recursive: true });
const LOG = join(OUT, 'timing.log');
rmSync(LOG, { force: true });
const URL_ = process.argv[2] || 'https://www.youtube.com/watch?v=aqz-KE-bpKQ';
const RUNS = Number(process.argv[3] || 3);
const BIN = process.env.BIN || '/Applications/Helium.app/Contents/MacOS/Helium';
const PORT = 9334;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const profile = mkdtempSync(join(tmpdir(), 'bp-timing-'));
mkdirSync(join(profile, 'NativeMessagingHosts'), { recursive: true });
writeFileSync(join(profile, 'NativeMessagingHosts', 'com.betterpip.host.json'), JSON.stringify({
  name: 'com.betterpip.host', description: 'test', type: 'stdio',
  path: join(ROOT, 'companion/src-tauri/target/release/better-pip-host'),
  allowed_origins: ['chrome-extension://fblfebboimckibidkphfchjlmohondgm/'],
}));
const browser = spawn(BIN, [
  `--user-data-dir=${profile}`, `--load-extension=${ROOT}`, `--disable-extensions-except=${ROOT}`,
  `--remote-debugging-port=${PORT}`, '--no-first-run', '--no-default-browser-check',
  '--autoplay-policy=no-user-gesture-required', URL_,
], { stdio: 'ignore', env: { ...process.env, BP_DEBUG_LOG: LOG } });

async function targets() {
  for (let i = 0; i < 50; i++) {
    try { return await (await fetch(`http://127.0.0.1:${PORT}/json`)).json(); } catch { await sleep(200); }
  }
  throw new Error('no devtools');
}
function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  let id = 0; const pending = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.method === 'Runtime.consoleAPICalled' && /Better PiP/.test(JSON.stringify(d.params.args))) console.log('   console:', d.params.args.map((a) => a.value ?? a.description).join(' ')); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  return new Promise((r) => (ws.onopen = () => r({ send })));
}
const evaluate = async (c, expression) => (await c.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;
const altP = async (c) => {
  for (const type of ['keyDown', 'keyUp']) await c.send('Input.dispatchKeyEvent', { type, code: 'KeyP', key: 'π', modifiers: 1, windowsVirtualKeyCode: 80 });
};

function readLog() {
  if (!existsSync(LOG)) return [];
  return readFileSync(LOG, 'utf8').trim().split('\n').map((line) => {
    const [t, dir, ...rest] = line.split(' ');
    let msg; try { msg = JSON.parse(rest.join(' ')); } catch { msg = rest.join(' '); }
    return { t: Number(t), dir, msg };
  });
}

try {
  let page;
  for (let i = 0; i < 40 && !page; i++) {
    page = (await targets()).find((t) => t.type === 'page' && t.url.includes(new URL(URL_).hostname));
    if (!page) await sleep(500);
  }
  if (!page) throw new Error('page not found: ' + (await targets()).map((t) => t.url).join(' '));
  const c = await connect(page.webSocketDebuggerUrl);
  await c.send('Runtime.enable');
  await c.send('Page.bringToFront');
  await sleep(4000);
  console.log('video:', await evaluate(c, `new Promise(r => { const v = document.querySelector('video'); const go = () => v.play().then(() => r('playing'), e => r('play failed ' + e)); v && v.readyState > 2 ? go() : v?.addEventListener('canplay', go, { once: true }); setTimeout(() => r('timeout'), 10000); })`));
  const rect = JSON.parse(await evaluate(c, `JSON.stringify(document.querySelector('video').getBoundingClientRect())`));
  // Hover like a user would (this also starts the app in the background).
  const hoverAt = Date.now();
  for (let i = 0; i < 5; i++) {
    await c.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: rect.x + rect.width / 2 + i * 10, y: rect.y + rect.height / 2 });
    await sleep(50);
  }
  await sleep(Number(process.env.HOVER_MS ?? 1000));
  const hello = readLog().find((e) => e.msg.type === 'hello');
  console.log('app started', hello ? `${hello.t - hoverAt}ms after hover` : 'not yet');

  for (let run = 1; run <= RUNS; run++) {
    const before = readLog().length;
    const pressed = Date.now();
    await altP(c);
    await sleep(Number(process.env.RUN_MS || 5000));
    const log = readLog().slice(before);
    const find = (pred) => log.find((e) => pred(e.msg, e.dir));
    const open = find((m, d) => d === '<' && m.type === 'open');
    const ready = find((m, d) => d === '>' && m.type === 'ready');
    const config = find((m, d) => d === '<' && m.type === 'config');
    const first = find((m, d) => d === '>' && m.type === 'first-frame');
    const rel = (e) => (e ? `${Math.round(e.t - pressed)}ms` : '—');
    console.log(`run ${run}: open received ${rel(open)}, window ready ${rel(ready)}, encoder config ${rel(config)}, first frame shown ${rel(first)}`);
    await altP(c); // close
    await sleep(2000);
  }
} finally {
  browser.kill();
  await sleep(500);
  rmSync(profile, { recursive: true, force: true });
}
