// Probe: can a tab capture + encode its playing <video>, and does it keep
// producing frames while the tab is hidden? Usage: node tools/capture-probe.mjs [url]
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const URL_ = process.argv[2] || 'https://www.youtube.com/watch?v=aqz-KE-bpKQ';
const BIN = '/Applications/Helium.app/Contents/MacOS/Helium';
const PORT = 9334;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const browser = spawn(BIN, [`--user-data-dir=${mkdtempSync(join(tmpdir(), 'bp-probe-'))}`, `--remote-debugging-port=${PORT}`,
  '--no-first-run', '--no-default-browser-check', '--autoplay-policy=no-user-gesture-required', URL_], { stdio: 'ignore' });

function connect(wsUrl) {
  const ws = new WebSocket(wsUrl); let id = 0; const pending = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); } };
  const send = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
  return new Promise((r) => (ws.onopen = () => r({ send })));
}
const ev = async (c, expression) => { const r = await c.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); return r.result?.result?.value ?? JSON.stringify(r.result?.exceptionDetails?.exception?.description || r.error); };

try {
  await sleep(9000);
  const targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
  const t = targets.find((x) => x.type === 'page' && x.url.includes('youtube'));
  const c = await connect(t.webSocketDebuggerUrl);
  console.log('support:', await ev(c, `(async () => JSON.stringify({
    mstp: typeof MediaStreamTrackProcessor, enc: typeof VideoEncoder,
    avc: (await VideoEncoder.isConfigSupported({ codec: 'avc1.640028', width: 1920, height: 1080, avc: { format: 'annexb' }, hardwareAcceleration: 'prefer-hardware' })).supported,
    vp8: (await VideoEncoder.isConfigSupported({ codec: 'vp8', width: 1920, height: 1080 })).supported,
  }))()`));
  console.log('start:', await ev(c, `(async () => {
    const v = document.querySelector('video'); v.muted = ${process.env.MUTED === '1'}; v.volume = ${process.env.VOL || 0.01}; await v.play().catch(()=>{});
    const track = v.captureStream().getVideoTracks()[0];
    const reader = new MediaStreamTrackProcessor({ track }).readable.getReader();
    window.__probe = { frames: 0, bytes: 0, chunks: 0, size: '' };
    const enc = new VideoEncoder({ output: (ch) => { __probe.chunks++; __probe.bytes += ch.byteLength; }, error: (e) => __probe.err = String(e) });
    let configured = false;
    (async () => { for (;;) { const { value: f, done } = await reader.read(); if (done) break; __probe.frames++;
      if (!configured) { __probe.size = f.displayWidth + 'x' + f.displayHeight; enc.configure({ codec: 'avc1.640028', width: f.displayWidth, height: f.displayHeight, bitrate: 6e6, latencyMode: 'realtime', avc: { format: 'annexb' } }); configured = true; }
      if (!window.__cv) { __cv = new OffscreenCanvas(f.displayWidth, f.displayHeight); __cx = __cv.getContext('2d'); }
      __cx.drawImage(f, 0, 0); const cf = new VideoFrame(__cv, { timestamp: f.timestamp }); f.close();
      if (enc.encodeQueueSize < 3) enc.encode(cf, { keyFrame: __probe.frames % 120 === 1 }); cf.close(); } })();
    return 'ok ' + v.currentTime.toFixed(1);
  })()`));
  const sample = async (label) => { const a = JSON.parse(await ev(c, 'JSON.stringify(__probe)')); await sleep(5000); const b = JSON.parse(await ev(c, 'JSON.stringify(__probe)'));
    console.log(label, { fps: (b.frames - a.frames) / 5, encodedFps: (b.chunks - a.chunks) / 5, kbps: Math.round(((b.bytes - a.bytes) * 8) / 5 / 1000), size: b.size, err: b.err, hidden: await ev(c, 'document.hidden'), paused: await ev(c, 'document.querySelector("video").paused'), t: await ev(c, 'document.querySelector("video").currentTime') }); };
  await sample('visible tab:');
  // Hide the tab by opening another one in front.
  const nt = await (await fetch(`http://127.0.0.1:${PORT}/json/new?about:blank`, { method: 'PUT' })).json();
  await fetch(`http://127.0.0.1:${PORT}/json/activate/${nt.id}`);
  await sleep(3000);
  await sample('hidden tab (0-8s):');
  await sleep(10000);
  await sample('hidden tab (18s+):');
} finally { browser.kill(); }
