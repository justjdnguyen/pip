# Contributing

Bug reports, browser compatibility results, and focused pull requests are welcome.
Include OS/browser versions, reproduction steps, the video site, and whether native
companion or browser-only PiP was used. Redact personal information from logs and images.

## Build from source

Checks and builds run on the Windows PC or `t3-mac`, not GitHub Actions. See
`docs/RELEASING.md` for the local check suite and release build helpers.

Install Node.js and stable Rust. Windows also needs Visual Studio C++ build tools,
the Windows SDK, and WebView2 Runtime. macOS needs Xcode command-line tools.

```sh
node tools/sync-ui.mjs
cargo build --release --locked --manifest-path companion/src-tauri/Cargo.toml --bin better-pip-host
```

Register the host using `tools/install-host-win.ps1` on Windows or
`bash tools/install-host-mac.sh` on macOS. These development helpers register the
build output in place. Load the extension unpacked from the repository root.

`companion/ui/shared/` contains generated copies of `src/content/` files.
Edit the source, run `node tools/sync-ui.mjs`, and commit the copies too.

```sh
node tools/check-release.mjs
cargo test --locked --manifest-path companion/src-tauri/Cargo.toml --bin better-pip-host
cargo check --locked --manifest-path companion/src-tauri/Cargo.toml --all-targets
```

On Windows, `cargo run --manifest-path companion/src-tauri/Cargo.toml --bin mf_decode_test`
also generates, decodes, and compares H.264 frames on your GPU.
`BP_WEB_VIDEO=1` forces web playback. Test actual resizing, fullscreen, and audio
synchronization on the affected OS; unit tests cannot establish those behaviors.

## Code and dependencies

Keep transport/pacing platform-independent where practical. Platform APIs belong in
`companion/src-tauri/src/video/`. Avoid new extension runtime dependencies. Include
relevant checks and limitations in PR descriptions. Contributions use the MIT license.

## Security

Use GitHub's private vulnerability reporting feature if enabled. Do not post credentials
or exploitable details in a public issue. If private reporting is unavailable, open an
issue asking for a private contact without disclosing the vulnerability.
