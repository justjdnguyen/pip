# Better PiP — handoff notes

Where the project stands and what's next, for whoever (human or agent) picks it up.
Written 2026-10-19 when moving development from the Mac to the Windows PC.

## What it is

Firefox-style Picture-in-Picture for Chromium browsers (target: Helium, also Chrome/Edge/Brave).
Two parts:

1. **Extension** (`manifest.json`, `src/`): hover button on videos, in-page placeholder, PiP controls.
   Fixed extension ID `fblfebboimckibidkphfchjlmohondgm` (from the `key` in the manifest).
2. **Companion app** (`companion/`, Tauri v2 / Rust): borderless, always-on-top PiP windows with no size
   limit and any number at once — things Chrome's own Document PiP can't do. If the app isn't
   installed, the extension falls back to Document PiP, then to native video PiP.

### How a PiP works with the app

```
tab: <video>.captureStream() → MediaStreamTrackProcessor → OffscreenCanvas (scale to window size)
     → VideoEncoder (H.264 Annex B, prefer-hardware, realtime) → base64 chunks
  → background.js (native messaging port, 4-byte length + JSON)
  → app (Rust, src-tauri/src/main.rs) → per-window Channel
  → companion/ui/player.js → decoder-worker.js (VideoDecoder + paced queue → VideoTrackGenerator)
  → <video> element in the PiP window
```

- Sound stays in the tab. For lip sync the tab routes the video's audio through a Web Audio DelayNode
  set to the measured picture latency (`audioDelay` in `src/content/app-session.js`; latency measured
  in `decoder-worker.js` → `latency` message).
- Controls are shared code (`src/content/player-ui.js`), copied into `companion/ui/shared/` by
  `node tools/sync-ui.mjs` (run after editing the shared files, before building the app).
- App keeps one hidden, preloaded "spare" window so a PiP opens in ~0.2s. Hovering a video starts
  the app and warms the tab's capture + encoder (`warmUp` in app-session.js).
- Debug: `BP_DEBUG_LOG=/path/file` makes the app log all non-video traffic + stats every 2s.

### File map

| Path | What |
|---|---|
| `src/background.js` | Toolbar/shortcut trigger; native host connection + relay; idle disconnect (60 min) |
| `src/content/app-session.js` | Companion-app mode: capture, encode, send, state/captions sync, lip-sync audio delay, hover warm-up |
| `src/content/pip-session.js` | Opens PiP (app → Document PiP → native), placeholder, "extension reloaded" toast |
| `src/content/player-ui.js` | The PiP controls UI (shared with the app) |
| `src/content/adapters.js` | Site specifics (YouTube captions/fullscreen/placeholder slot) |
| `src/content/toggle.js` | Hover button |
| `companion/src-tauri/src/main.rs` | App: native messaging I/O, window management, spare window, macOS aspect lock |
| `companion/ui/player.js`, `decoder-worker.js` | PiP window page: remote media mirror, decoding, display |
| `tools/install-host-mac.sh` | Registers the app with browsers on macOS |
| `tools/startup-timing.mjs` | Opens/closes PiP on YouTube N times via CDP, prints startup timings from the app log |
| `tools/e2e.mjs` | Older end-to-end smoke test (screenshots) |
| `tools/*.swift`, `capture-probe.mjs` | macOS-only measuring tools |

## Current state (all measured on the Mac, Helium, YouTube)

- Opening: ~0.35s first time in a tab after a short hover, ~0.2s after that.
- 60 fps, 0 dropped frames; picture ~75 ms behind the tab; lip sync confirmed good by the user.
- Audio uses a 'playback'-latency AudioContext and slow/rare delay changes (fixed crackling).
- Window background is black (so resize gaps aren't white).

### Known issue being worked on: resizing isn't smooth

The video is drawn by a web page, which redraws a frame or two after the window edge moves, so
there are gaps while dragging. Agreed fix (not started yet): **draw video natively under a
transparent web page that only holds the controls**, on both platforms.

## Next: native video, with Windows/macOS parity

Same structure on both platforms; only "decode" and "show" differ.

1. **Shared Rust core** (new `companion/src-tauri/src/video/`):
   - `config` / `chunk` messages for a window go to a native sink instead of the page.
   - Annex B parsing, the smoothing buffer (port of the pacing in `decoder-worker.js`: clock follows the
     fastest frame, adaptive 15–150 ms delay), keyframe requests (`{type:'keyframe', id}`), lip-sync
     latency reports (`{type:'latency', id, ms}`; skip first 30 frames, EMA 0.05, every 500 ms).
   - Page asks `invoke('video_mode')` → `'native'` (hide its `<video>`, transparent background,
     report `codecs: ['avc']`, skip the worker) or `'web'` (current path as fallback).
   - Native sink sends `{type:'first-frame'}` to the page so it hides "Connecting…".
2. **Windows** (do first, on the Windows PC):
   - **Spike first (riskiest bit):** a DirectComposition target on the top-level HWND
     (`topmost = false`, so it sits under child windows), visual with a composition swap chain, and the
     WebView2 child set transparent (`.transparent(true)` in Tauri). Confirm the web page's transparent
     areas show the swap chain, and that controls draw on top.
   - **Spike done (2026-10-19), it works:** `companion/src-tauri/src/bin/dcomp_spike.rs` + `companion/ui/spike.html`
     (`cargo run --bin dcomp_spike`, `BP_SPIKE_SECONDS=n` to auto-quit). Window built with `.transparent(true)`
     + `.no_redirection_bitmap(true)`; DComp target (`topmost=false`) → root visual → black 16×16 backdrop
     surface stretched to the window (letterbox bars; without it they're see-through) + a fixed 1280×720
     flip-model composition swap chain scaled/centred by `SetTransform2` + `Commit()` in the `WM_SIZE`
     subclass handler. Verified by screenshots: the page's transparent areas show the swap chain, the
     controls/gradient/40%-alpha panel blend on top, letterboxing is correct, `WM_SIZING` aspect lock
     works. Scripted corner drag: no gaps; in 1 of 5 mid-drag captures the video was one frame *ahead* of
     the window edge (slight crop), worth a look by hand-dragging (maybe apply the transform in
     `WM_WINDOWPOSCHANGING` or use `IDCompositionDevice::WaitForCommitCompletion`/`DwmFlush`).
     Gotchas: Tauri examples crash with 0xC0000139 (no comctl32 v6 manifest), so spikes go in `src/bin/`;
     data: URLs need the `webview-data-url` feature, so test pages go in `ui/`.
   - Decode: Media Foundation H.264 decoder MFT with a D3D11 device manager, low-latency mode, NV12 out.
   - Show: ID3D11VideoProcessor blit NV12 → BGRA swap chain sized to the video; on `WM_SIZE` only update
     the DComp visual's scale transform + `Commit()` (synchronous with DWM → smooth resize).
   - Window subclass (`SetWindowSubclass`): `WM_SIZING` for aspect lock (macOS has this already via
     `setContentAspectRatio`), `WM_SIZE` for the transform.
   - Show without stealing focus (`SW_SHOWNOACTIVATE`), `skip_taskbar(true)`.
   - Fallback web path on Windows: WebView2 (Chromium) may lack `VideoTrackGenerator` in workers →
     add a fallback (post VideoFrames to the page and draw to a canvas, or MediaStreamTrackGenerator).
   - `tools/install-host-win.ps1`: write the host manifest JSON and registry keys under HKCU for
     Chrome (`Software\Google\Chrome\NativeMessagingHosts\com.betterpip.host`), Edge
     (`Software\Microsoft\Edge\...`), Chromium (`Software\Chromium\...`), Brave
     (`Software\BraveSoftware\Brave-Browser\...`) and Helium (**check** which key Helium reads).
3. **macOS** (later, on the Mac): `AVSampleBufferDisplayLayer` in a layer-backed NSView under the
   WKWebView (autoresizing), fed compressed samples (Annex B → AVCC, format description from SPS/PPS;
   skipped frames get `DoNotDisplay`, shown ones `DisplayImmediately`); transparent webview needs
   `macOSPrivateApi: true` + tauri feature `macos-private-api`.

Parity checklist: smooth resize, shape kept, no size limit, multi-monitor, several PiPs, bottom-right
placement, no title bar, ~0.2–0.4 s open, 60 fps, lip sync, same controls/shortcuts/fullscreen,
fallback to the web path if native fails.

## Building and running on Windows

1. Install: Rust (rustup, MSVC toolchain), Visual Studio Build Tools (C++ workload + Windows SDK),
   Node.js. WebView2 runtime ships with Windows 11. (All present on the Windows PC as of 2026-10-19:
   Rust 1.92 MSVC, VS 2022 + Build Tools, SDK 10.0.26100, Node 25, WebView2 155.)
2. `node tools/sync-ui.mjs`, then `cd companion/src-tauri && cargo build --release`.
3. Register the host with the browser (install script still to be written; see above).
4. Load the extension unpacked from the repo root (`chrome://extensions` → Developer mode → Load unpacked).
5. After reloading the extension, reload video tabs (old tabs show "Better PiP was updated…").

Notes: `tools/startup-timing.mjs` and `tools/e2e.mjs` default to the macOS Helium path; set
`BIN=C:\path\to\browser.exe`. The `.swift` tools and `install-host-mac.sh` are macOS-only.
