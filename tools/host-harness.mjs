// Talks to the companion app directly over its stdin/stdout protocol, no
// browser needed. Opens one window and prints everything the app sends back.
// Usage: node tools/host-harness.mjs [seconds] [binary]
import { spawn } from 'node:child_process';
import { resolve } from 'node:path';

const secs = Number(process.argv[2] || 6);
const bin = process.argv[3] || resolve(import.meta.dirname, '../companion/src-tauri/target/debug/better-pip-host');
const app = spawn(bin, ['chrome-extension://test/'], { stdio: ['pipe', 'pipe', 'inherit'] });
const send = (msg) => {
  const body = Buffer.from(JSON.stringify(msg));
  const len = Buffer.alloc(4);
  len.writeUInt32LE(body.length);
  app.stdin.write(Buffer.concat([len, body]));
};
let buf = Buffer.alloc(0);
app.stdout.on('data', (d) => {
  buf = Buffer.concat([buf, d]);
  while (buf.length >= 4 && buf.length >= 4 + buf.readUInt32LE(0)) {
    const n = buf.readUInt32LE(0);
    console.log(buf.subarray(4, 4 + n).toString().slice(0, 400));
    buf = buf.subarray(4 + n);
  }
});
send({ type: 'open', id: 'h1', width: 480, height: 270, title: 'harness' });
setTimeout(() => app.stdin.end(), secs * 1000);
