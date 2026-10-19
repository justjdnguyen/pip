// Renders the extension icon (rose rounded square + white PiP glyph) to PNGs.
// Usage: node tools/make-icons.mjs
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

// Signed distance to a rounded rectangle (in 64-unit icon space).
function sdRoundRect(px, py, x0, y0, x1, y1, r) {
  const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
  const hx = (x1 - x0) / 2 - r, hy = (y1 - y0) / 2 - r;
  const qx = Math.abs(px - cx) - hx, qy = Math.abs(py - cy) - hy;
  const ox = Math.max(qx, 0), oy = Math.max(qy, 0);
  return Math.hypot(ox, oy) + Math.min(Math.max(qx, qy), 0) - r;
}

function shade(x, y) {
  // returns [r,g,b,a] in 0..1
  if (sdRoundRect(x, y, 0, 0, 64, 64, 14) > 0) return [0, 0, 0, 0];
  const outer = sdRoundRect(x, y, 11, 15, 53, 49, 4);
  const frame = outer <= 0 && outer >= -4; // 4-unit stroke
  const inner = sdRoundRect(x, y, 31, 31, 49, 45, 2) <= 0;
  if (frame || inner) return [1, 1, 1, 1];
  return [0xe1 / 255, 0x1d / 255, 0x48 / 255, 1];
}

function crc32(buf) {
  let c, crc = 0xffffffff;
  for (const b of buf) {
    c = (crc ^ b) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

function png(size) {
  const SS = 4;
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let py = 0; py < size; py++) {
    raw[py * (size * 4 + 1)] = 0;
    for (let px = 0; px < size; px++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < SS; sy++) for (let sx = 0; sx < SS; sx++) {
        const x = ((px + (sx + 0.5) / SS) / size) * 64;
        const y = ((py + (sy + 0.5) / SS) / size) * 64;
        const [cr, cg, cb, ca] = shade(x, y);
        r += cr * ca; g += cg * ca; b += cb * ca; a += ca;
      }
      const n = SS * SS, o = py * (size * 4 + 1) + 1 + px * 4;
      raw[o] = a ? Math.round((r / a) * 255) : 0;
      raw[o + 1] = a ? Math.round((g / a) * 255) : 0;
      raw[o + 2] = a ? Math.round((b / a) * 255) : 0;
      raw[o + 3] = Math.round((a / n) * 255);
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw)), chunk('IEND', Buffer.alloc(0)),
  ]);
}

import { mkdirSync, rmSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

// Extension icons.
for (const s of [16, 32, 48, 128]) writeFileSync(new URL(`../assets/icon-${s}.png`, import.meta.url), png(s));

// Companion app icons.
const appIcons = new URL('../companion/src-tauri/icons/', import.meta.url);
mkdirSync(appIcons, { recursive: true });
writeFileSync(new URL('32x32.png', appIcons), png(32));
writeFileSync(new URL('128x128.png', appIcons), png(128));
writeFileSync(new URL('128x128@2x.png', appIcons), png(256));
writeFileSync(new URL('icon.png', appIcons), png(512));
if (process.platform === 'darwin') {
  const set = new URL('icon.iconset/', appIcons);
  mkdirSync(set, { recursive: true });
  for (const s of [16, 32, 128, 256, 512]) {
    writeFileSync(new URL(`icon_${s}x${s}.png`, set), png(s));
    writeFileSync(new URL(`icon_${s}x${s}@2x.png`, set), png(s * 2));
  }
  execFileSync('iconutil', ['-c', 'icns', set.pathname, '-o', new URL('icon.icns', appIcons).pathname]);
  rmSync(set, { recursive: true });
}
console.log('icons written');
