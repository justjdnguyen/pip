// Fast release checks: versions, syntax, generated UI, and native-host identity.
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { Script } from 'node:vm';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const read = p => readFileSync(resolve(root, p), 'utf8');
const manifest = JSON.parse(read('manifest.json'));
const config = JSON.parse(read('companion/src-tauri/tauri.conf.json'));
const cargoVersion = read('companion/src-tauri/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1];
assert.equal(manifest.version, config.version, 'Extension and companion versions differ');
assert.equal(manifest.version, cargoVersion, 'Cargo version differs');
const id = createHash('sha256').update(Buffer.from(manifest.key, 'base64')).digest('hex').slice(0, 32)
  .replace(/[0-9a-f]/g, c => String.fromCharCode(97 + parseInt(c, 16)));
for (const f of ['tools/install-host-win.ps1', 'tools/install-host-mac.sh']) {
  assert.ok(read(f).includes(id), `${f} does not allow extension ${id}`);
}
for (const f of ['util.js', 'styles.js', 'player-ui.js']) {
  assert.equal(read(`src/content/${f}`).replaceAll('\r\n', '\n'),
    read(`companion/ui/shared/${f}`).replaceAll('\r\n', '\n'), `Run tools/sync-ui.mjs: ${f}`);
}
function syntax(dir) {
  for (const f of readdirSync(resolve(root, dir), { withFileTypes: true })) {
    const path = `${dir}/${f.name}`;
    if (f.isDirectory()) syntax(path);
    else if (f.name.endsWith('.js')) new Script(read(path), { filename: path });
  }
}
syntax('src'); syntax('companion/ui');
for (const f of ['LICENSE', 'PRIVACY.md', 'README.md', 'docs/INSTALL.md', 'docs/RELEASING.md']) {
  assert.ok(read(f).length > 100, `Missing release document: ${f}`);
}
console.log(`Release checks passed: version ${manifest.version}; extension ${id}`);
