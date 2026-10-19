// Build an allowlisted preview archive. Usage: node tools/package-release.mjs <platform> <version>
import { cpSync, mkdirSync, readFileSync, writeFileSync, existsSync, mkdtempSync, chmodSync, readdirSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const [platform, version = '0.1.0-preview.1'] = process.argv.slice(2);
if (!['extension', 'windows-x64', 'macos-arm64', 'macos-x64'].includes(platform)) throw Error('Unknown package platform');
if (!/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(version)) throw Error('Invalid release version');
const base = JSON.parse(readFileSync(join(root, 'manifest.json'))).version;
if (version.split('-')[0] !== base) throw Error('Release version differs from manifest');
if (platform === 'windows-x64' && process.platform !== 'win32') throw Error('Build Windows packages on Windows');
if (platform.startsWith('macos') && process.platform !== 'darwin') throw Error('Build Mac packages on macOS');
execFileSync(process.execPath, [join(root, 'tools/check-release.mjs')], { stdio: 'inherit' });
const dist = join(root, 'dist'); mkdirSync(dist, { recursive: true });
const stageRoot = mkdtempSync(join(dist, '.stage-'));
const name = `better-pip-${platform}-${version}`;
const stage = join(stageRoot, name); mkdirSync(stage);
const copy = (src, dest = src) => {
  const target = join(stage, dest); mkdirSync(dirname(target), { recursive: true });
  cpSync(join(root, src), target, { recursive: true });
};
if (platform === 'extension') {
  copy('manifest.json'); copy('src'); copy('assets'); copy('LICENSE');
} else {
  const exe = platform === 'windows-x64' ? 'better-pip-host.exe' : 'better-pip-host';
  copy(`companion/src-tauri/target/release/${exe}`, exe);
  const notices = 'dist/THIRD-PARTY-LICENSES.html';
  if (!existsSync(join(root, notices))) throw Error('Generate third-party license notices first (docs/RELEASING.md)');
  copy(notices, 'THIRD-PARTY-LICENSES.html'); copy('LICENSE'); copy('docs/INSTALL.md', 'INSTALL.md'); copy('PRIVACY.md');
  if (platform === 'windows-x64') {
    for (const f of readdirSync(join(root, 'packaging/windows'))) copy(`packaging/windows/${f}`, f);
    copy('tools/install-host-win.ps1');
  } else {
    for (const f of ['install.sh', 'uninstall.sh']) { copy(`packaging/macos/${f}`, f); chmodSync(join(stage, f), 0o755); }
    copy('tools/install-host-mac.sh'); chmodSync(join(stage, exe), 0o755);
  }
}
const archive = join(dist, `${name}.${platform.startsWith('macos') ? 'tar.gz' : 'zip'}`);
if (existsSync(archive)) throw Error(`Archive already exists: ${archive}`);
if (platform.startsWith('macos')) execFileSync('tar', ['-czf', archive, '-C', stageRoot, name]);
else if (process.platform === 'win32') {
  // Fixed PowerShell code; paths are passed via environment, never interpolated into shell text.
  execFileSync('powershell.exe', ['-NoProfile', '-Command',
    'Compress-Archive -Path (Join-Path $env:BP_PACKAGE_STAGE "*") -DestinationPath $env:BP_PACKAGE_ARCHIVE'],
    { env: { ...process.env, BP_PACKAGE_STAGE: stage, BP_PACKAGE_ARCHIVE: archive } });
} else execFileSync('zip', ['-qr', archive, '.'], { cwd: stage });
const hash = createHash('sha256').update(readFileSync(archive)).digest('hex');
writeFileSync(`${archive}.sha256`, `${hash}  ${name}.${platform.startsWith('macos') ? 'tar.gz' : 'zip'}\n`);
console.log(archive);
