// Build and package on the device that runs the target OS. No hosted builds.
// Usage: node tools/build-release-local.mjs <platform> <version>
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const [platform, version] = process.argv.slice(2);
const targets = { 'windows-x64': 'x86_64-pc-windows-msvc', 'macos-arm64': 'aarch64-apple-darwin', 'macos-x64': 'x86_64-apple-darwin' };
if (!version || !/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(version)) throw Error('Supply a release version');
if (platform !== 'extension' && !targets[platform]) throw Error('Unknown platform');
if (platform === 'windows-x64' && process.platform !== 'win32') throw Error('Build Windows on Windows');
if (platform.startsWith('macos') && process.platform !== 'darwin') throw Error('Build Mac packages on t3-mac');
const base = JSON.parse(readFileSync(resolve(root, 'manifest.json'), 'utf8')).version;
if (version.split('-')[0] !== base) throw Error('Release version differs from manifest');
const run = (command, args) => execFileSync(command, args, {
  cwd: root, stdio: 'inherit', env: { ...process.env, MACOSX_DEPLOYMENT_TARGET: '13.3' },
});
run(process.execPath, ['tools/check-release.mjs']);
if (platform === 'extension') {
  run(process.execPath, ['tools/package-release.mjs', platform, version]);
} else {
  const target = targets[platform];
  // Reuse the native Windows build directory; explicit Mac targets allow both
  // architectures to coexist on t3-mac without packaging the wrong binary.
  const targetArgs = platform === 'windows-x64' ? [] : ['--target', target];
  run('cargo', ['build', '--release', '--locked', '--manifest-path', 'companion/src-tauri/Cargo.toml', '--bin', 'better-pip-host', ...targetArgs]);
  mkdirSync(resolve(root, 'dist'), { recursive: true });
  run('cargo', ['about', 'generate', '--locked', '--fail', '--manifest-path', 'companion/src-tauri/Cargo.toml', '--all-features', 'tools/licenses.hbs', '-o', 'dist/THIRD-PARTY-LICENSES.html']);
  const binary = resolve(root, 'companion/src-tauri/target', ...(targetArgs.length ? [target] : []), 'release', platform === 'windows-x64' ? 'better-pip-host.exe' : 'better-pip-host');
  run(process.execPath, ['tools/package-release.mjs', platform, version, binary]);
}
