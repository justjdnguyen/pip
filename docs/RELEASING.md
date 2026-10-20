# Releases

## Build preview packages

Run `node tools/sync-ui.mjs`, then `node tools/check-release.mjs`.
`node tools/package-release.mjs extension <version>` creates the unpacked-extension ZIP.
Build the companion with `cargo build --release --locked --bin better-pip-host` from
`companion/src-tauri`. Generate dependency notices with:

```sh
cargo install cargo-about --locked --version 0.8.4
cargo about generate --locked --fail --manifest-path companion/src-tauri/Cargo.toml --all-features tools/licenses.hbs -o dist/THIRD-PARTY-LICENSES.html
```

Then run `node tools/package-release.mjs windows-x64 <version>` on Windows, or
`macos-arm64` / `macos-x64` on the Mac device. The package script requires
the license notices and embeds them beside the binary. It copies only an explicit
allowlist of extension files and companion installer files; no repository history,
test captures, or build directories go in an extension archive.

## On-device checks and builds

Checks and release builds run on the Windows PC and `t3-mac`. GitHub Actions
workflows are disabled and removed; do not use hosted runners for this project.
Install gitleaks 8.30.1 and cargo-about 0.8.4 from their official releases/crates.
Set `BP_GITLEAKS_BIN` if gitleaks is not on PATH.

Run `node tools/check-local.mjs` on each device. It checks JavaScript and release
consistency, scans Git history for secrets, runs Rust tests/all-target checks,
and tests the installer in isolation on Windows. An optional Rust target argument
runs the Rust checks for that target (the device must be able to execute it).
Also run the caption layout fixture in a browser and test actual PiP playback.

```sh
# Windows PC
node tools/build-release-local.mjs windows-x64 0.1.0-preview.4
node tools/build-release-local.mjs extension 0.1.0-preview.4

# t3-mac; install both Rust targets first
rustup target add aarch64-apple-darwin x86_64-apple-darwin
node tools/build-release-local.mjs macos-arm64 0.1.0-preview.4
node tools/build-release-local.mjs macos-x64 0.1.0-preview.4
```

The build helper generates fresh dependency notices, packages the specified
architecture, and writes SHA-256 sidecars in `dist/`. Collect the four archives
from the same commit and combine their sidecars into `SHA256SUMS.txt`. Verify
the checksums, then upload all four archives and the checksum file to a draft
GitHub prerelease. Tag version and manifest/Cargo base versions must agree
(for example, `v0.1.0-preview.4` uses base `0.1.0`). GitHub only hosts the artifacts.

Before publishing, test installation/uninstallation, playback, speed controls,
fullscreen, and native failure recovery on actual Windows and Mac machines.
Compilation is not an end-to-end Mac playback test.

Unsigned preview distribution is intentional and must be stated in release notes.
Trusted Windows signing and Apple Developer ID signing/notarization require publisher
credentials and are follow-up work. Never commit signing keys or passwords.

## Chrome Web Store

The initial release uses Load unpacked. A store listing requires a Chrome developer
account, listing assets, privacy disclosures, and Google's review. See
[Google's publication guide](https://developer.chrome.com/docs/webstore/publish).
Use the extension-only ZIP, not a companion archive or the repository source archive.

Before shipping a store build, reserve its item in the developer dashboard and copy
the dashboard's public key into `manifest.json`, following
[Google's stable-ID instructions](https://developer.chrome.com/docs/extensions/reference/manifest/key).
The current development public key does not guarantee the eventual store ID. Update
both native-host installer allowlists and documented extension ID to match the store
item before distributing companions for it. Do not publish a store item that cannot
connect to the release companion.

The listing should explain the optional companion download, video-capture behavior,
all-sites page access, local processing, and the limitations without the companion.
Use `PRIVACY.md` as the public privacy-policy URL after the repository is public.
