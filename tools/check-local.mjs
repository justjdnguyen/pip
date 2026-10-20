// Run the check suite on the actual Windows or Mac device, without hosted CI.
// Usage: node tools/check-local.mjs [Rust target triple]
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const target = process.argv[2];
const run = (command, args) => execFileSync(command, args, { cwd: root, stdio: 'inherit' });
run(process.execPath, ['tools/check-release.mjs']);
run(process.execPath, ['--test', 'tools/test-youtube-captions.mjs']);
// Fail if the scanner is unavailable rather than silently omitting the scan.
run(process.env.BP_GITLEAKS_BIN || 'gitleaks', ['git', '--redact', '--no-banner', '--log-opts=--all']);
const cargo = ['--locked', '--manifest-path', 'companion/src-tauri/Cargo.toml', ...(target ? ['--target', target] : [])];
run('cargo', ['test', ...cargo, '--bin', 'better-pip-host']);
run('cargo', ['check', ...cargo, '--all-targets']);
if (process.platform === 'win32') {
  run('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', resolve(root, 'tools/test-installer-win.ps1')]);
}
console.log(`Device checks passed: ${process.platform}/${process.arch}${target ? ` (${target})` : ''}`);
