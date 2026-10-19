// One geometric mark for SVG, extension PNGs, and native ICO/ICNS containers.
// Usage: node tools/make-icons.mjs
import { writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';

const colours = { ground: '#14251f', frame: '#f4f6ef', picture: '#69e0af' };
const rgb = hex => hex.slice(1).match(/../g).map(c => parseInt(c, 16) / 255);
const ground = rgb(colours.ground), frameColour = rgb(colours.frame), picture = rgb(colours.picture);
const glyph = `<defs><mask id="cut"><rect width="64" height="64" fill="white"/><rect x="28" y="29" width="36" height="35" fill="black"/></mask></defs>
  <rect x="10" y="14" width="42" height="30" rx="5" fill="none" stroke="${colours.frame}" stroke-width="4" mask="url(#cut)"/>
  <rect x="30" y="32" width="26" height="18" rx="3" fill="${colours.picture}"/>`;
const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" role="img" aria-label="Better PiP">
  <rect width="64" height="64" rx="14" fill="${colours.ground}"/>
  ${glyph}
</svg>\n`;
writeFileSync(new URL('../assets/icon.svg', import.meta.url), svg);
writeFileSync(new URL('../assets/wordmark.svg', import.meta.url), `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 960 224" role="img" aria-label="Better PiP">
  <rect width="960" height="224" fill="${colours.ground}"/>
  <svg x="48" y="48" width="128" height="128" viewBox="0 0 64 64">${glyph.replaceAll('id="cut"', 'id="wordmark-cut"').replaceAll('url(#cut)', 'url(#wordmark-cut)')}</svg>
  <text x="210" y="145" fill="${colours.frame}" font-family="Arial, Helvetica, sans-serif" font-size="92" font-weight="700" letter-spacing="-4">Better PiP</text>
</svg>\n`);

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
  const frame = sdRoundRect(x, y, 8, 12, 54, 46, 7) <= 0 &&
    sdRoundRect(x, y, 12, 16, 50, 42, 3) >= 0 && !(x >= 28 && y >= 29);
  const floating = sdRoundRect(x, y, 30, 32, 56, 50, 3) <= 0;
  if (floating) return [...picture, 1];
  if (frame) return [...frameColour, 1];
  return [...ground, 1];
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

import { mkdirSync } from 'node:fs';

// Extension icons.
for (const s of [16, 32, 48, 128]) writeFileSync(new URL(`../assets/icon-${s}.png`, import.meta.url), png(s));

// Companion app icons.
const appIcons = new URL('../companion/src-tauri/icons/', import.meta.url);
mkdirSync(appIcons, { recursive: true });
writeFileSync(new URL('32x32.png', appIcons), png(32));
writeFileSync(new URL('128x128.png', appIcons), png(128));
writeFileSync(new URL('128x128@2x.png', appIcons), png(256));
writeFileSync(new URL('icon.png', appIcons), png(512));
// Modern Windows and macOS both accept PNG-backed icon entries. Generate both
// containers on every OS so a branding update cannot leave one platform stale.
const icoSizes = [16, 32, 48, 256];
const icoImages = icoSizes.map(png);
const ico = Buffer.alloc(6 + 16 * icoSizes.length);
ico.writeUInt16LE(1, 2); ico.writeUInt16LE(icoSizes.length, 4);
let offset = ico.length;
icoSizes.forEach((s, i) => {
  const p = 6 + i * 16;
  ico[p] = ico[p + 1] = s === 256 ? 0 : s;
  ico.writeUInt16LE(1, p + 4); ico.writeUInt16LE(32, p + 6);
  ico.writeUInt32LE(icoImages[i].length, p + 8); ico.writeUInt32LE(offset, p + 12);
  offset += icoImages[i].length;
});
writeFileSync(new URL('icon.ico', appIcons), Buffer.concat([ico, ...icoImages]));
const icnsEntries = [[16, 'icp4'], [32, 'icp5'], [64, 'icp6'], [128, 'ic07'],
  [256, 'ic08'], [512, 'ic09'], [1024, 'ic10']].map(([size, type]) => {
  const data = png(size), header = Buffer.alloc(8);
  header.write(type); header.writeUInt32BE(data.length + 8, 4);
  return Buffer.concat([header, data]);
});
const icnsHeader = Buffer.alloc(8);
icnsHeader.write('icns'); icnsHeader.writeUInt32BE(8 + icnsEntries.reduce((n, b) => n + b.length, 0), 4);
writeFileSync(new URL('icon.icns', appIcons), Buffer.concat([icnsHeader, ...icnsEntries]));
console.log('SVG, extension PNGs, and Windows/macOS app icons written');
