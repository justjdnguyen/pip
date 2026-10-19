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
`macos-arm64` / `macos-x64` on the matching Mac runner. The package script requires
the license notices and embeds them beside the binary. It copies only an explicit
allowlist of extension files and companion installer files; no repository history,
test captures, or build directories go in an extension archive.

## GitHub automation

The release workflow builds Windows x64, Apple silicon, and Intel Mac companions,
the extension ZIP, dependency notices, and SHA-256 checksums. Tag pushes matching
`v*` create a **draft prerelease**; inspect all assets and CI results before publishing.
The workflow can also be dispatched for an existing tag. Tag version and manifest/
Cargo base versions must agree (for example, `v0.1.0-preview.1` uses base `0.1.0`).

Before publishing, test installation/uninstallation, playback, speed controls,
fullscreen, and native failure recovery on actual Windows and Mac machines.
CI compilation is not an end-to-end Mac playback test.

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
