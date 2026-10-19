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
     **Done (2026-10-19, branch `windows-native-video`):** `src/video/win_decoder.rs` (`Decoder::new/decode/flush`,
     `create_device` with multithread protection on). Tested by `cargo run --bin mf_decode_test`: encodes a moving
     checkerboard with the MF H.264 encoder (no test files needed), decodes it, checks pixels. Results at 1280×720,
     854×480, 1920×1080: every picture comes out on the same `decode` call it went in (0 frames of delay), decode
     call ~0.1 ms median (the first call ~20 ms: decoder setup on the first keyframe), output is a 7-slice NV12
     texture array with `D3D11_BIND_DECODER`, luma matches within 2/255, and the visible size comes from the
     display aperture (854 inside an 864-wide texture, 1080 inside 1088). Frames hold a decoder surface while
     alive, so present and drop them promptly.
   - **Show + app wiring done (2026-10-19, branch `windows-native-video`):** `video/win_sink.rs` (decoder →
     `ID3D11VideoProcessor` NV12→BGRA with crop, BT.709 limited → full RGB → composition swap chain sized to the
     picture, `Present(0)`), `video/win_window.rs` (subclass: `WM_SIZING` aspect lock for all PiP windows,
     `WM_SIZE` DComp transform + `Commit`, black backdrop), `video/core.rs` (shared pacer, a port of
     decoder-worker.js; unit-tested), `main.rs` (`video_mode` command, chunks/config routed to the window's video
     thread, keyframe/latency to the tab, `first-frame` and `video-mode` fallback to the page; Windows windows are
     `.transparent(true).no_redirection_bitmap(true).skip_taskbar(true)`, shown with `SW_SHOWNOACTIVATE`),
     `ui/player.js` (asks `video_mode`; native = transparent page, reports `codecs: ['avc']`, no worker; switches
     to the worker on `video-mode: web`). `BP_WEB_VIDEO=1` forces the web path.
     `cargo run --bin native_video_spike` plays a generated clip through the real sink.
     **Verified** in Helium + YouTube with `tools/startup-timing.mjs` (`BIN=%LOCALAPPDATA%\imput\Helium\Application\chrome.exe`):
     first frame ~0.3 s after Alt+P, native path used, latency reports 55–61 ms; screenshot + scripted corner
     drag of the real PiP window: correct picture, no gaps. Drags need the cursor ~2 px *outside* the visible
     edge (invisible resize border).
     **Not yet verified:** the web fallback on WebView2 (`BP_WEB_VIDEO=1` runs failed to start the test page —
     environment flake while the PC was in use, not diagnosed), color accuracy vs the tab (BT.709 assumed),
     multi-monitor/DPI changes, several PiPs at once, fullscreen.
   - `tools/install-host-win.ps1` done: manifest in `%LOCALAPPDATA%\BetterPiP`, HKCU keys for Chrome, Chromium
     (**Helium reads `Software\Chromium`**, checked in its chrome.dll), Edge, Brave, Vivaldi. `-Uninstall` removes them.
   - Shared contract (`src/video/mod.rs`): `StreamConfig`, `Frame { data, key, ts_us, display }`, trait
     `VideoSink { configure, submit, flush }` + per-OS `create_sink(&WebviewWindow)`. The Mac thread
     (branch `macos-native-video`) implements it in `video/macos.rs`.
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
3. **macOS**: `AVSampleBufferDisplayLayer` in a layer-backed NSView under the WKWebView (autoresizing),
   fed compressed samples (Annex B → AVCC, format description from SPS/PPS; skipped frames get
   `DoNotDisplay`, shown ones `DisplayImmediately`).
   - **Spike done (2026-10-19), it works:** `companion/src-tauri/src/bin/avsb_spike.rs` + `companion/ui/spike.html`
     (`cargo run --bin avsb_spike`, `BP_SPIKE_SECONDS=n` to auto-quit, `BP_SPIKE_SWITCH=n` to alternate 16:9/4:3
     streams every n s, `BP_SPIKE_DUMP=1` to print the view tree). Unlike the Windows spike it uses the real
     sink (`video::create_sink` → `src/video/macos.rs`, with `video/mod.rs` pulled in by `#[path]`), fed by a VideoToolbox encoder making a test
     pattern, converted to Annex B as the extension sends it. Layout: tao's NSWindow → `WryWebViewParent`
     (content view) → [our layer-hosting NSView whose layer *is* the AVSampleBufferDisplayLayer
     (`videoGravity = resizeAspect`, black background), added `positioned: NSWindowBelow relativeTo: nil`,
     autoresizing width+height] + `WryWebView`. Verified by screenshot: the page's transparent areas show
     the video, the label / 40%-red panel / control gradient blend on top. Decode-only frames
     (`DoNotDisplay`, 1 in 30 in the spike) and stream reconfigure (new format description on the next
     keyframe) work with no layer errors.
     4:3 streams are pillarboxed correctly in the 16:9 window. Resize: `tools/drag-resize.swift`
     (scripted corner drag + screen recording; finds the video's green border in each frame and looks for
     black window background past it) found **no gaps** in 0.6 s, 0.3 s and 0.15 s out-and-back drags of
     +400×225 pt; mid-drag frames show the video flush with the window edge while the page's controls lag
     by about a frame (expected: only they are web-drawn).
   - **In the app (2026-10-19, Helium/YouTube, release build):** every window, the hidden spare included,
     gets a native sink (`BP_DEBUG_LOG` shows `native {"started":true}`). First frame 0.16–0.21 s after
     Alt+P with a warm tab (0.5–1.8 s for the first PiP in a tab, mostly waiting for the tab's encoder
     config); latency reports 56–79 ms (web path in the same setup: ~106 ms). Mid-drag frames of the real
     window: video always fills the window. `BP_WEB_VIDEO=1` still gives the web path (checked).
     The drag tool's gap count only means something with the spike's green border; real video (green
     grass!) confuses it, so check real-app drags by looking at the frames.
   - Gotchas:
     - **No private API needed.** wry sets the WKWebView's `drawsBackground = false` whenever the window
       has a `background_color` (main.rs already sets black), so the page is transparent without
       `macOSPrivateApi` / `transparent(true)`. tauri.conf.json is unchanged.
     - `screencapture -l <window>` leaves out the WKWebView's content (it's drawn out of process), so it
       shows only the video. Capture a screen region instead (`screencapture -R x,y,w,h`).
     - The layer's `enqueueSampleBuffer`/`flush`/`status` are deprecated since macOS 14 (use
       `sampleBufferRenderer`), but that needs 14.0 and we support 13.3; they still work on 15.
     - Layer-hosting order matters: `setLayer:` before `setWantsLayer: YES`, then AppKit keeps the layer's
       frame equal to the view bounds (no implicit animation, no extra code on resize).
   - Error semantics: `submit` returns the shared `video::NeedKeyframe` error when it can only restart at
     a keyframe (after `flush`/`configure`, or the layer's `requiresFlushToResumeDecoding`, e.g. after the
     decoder was taken away); a layer `status == failed` is a real error. **Shared change (Windows,
     please pick up):** `core.rs` now falls back to the web path after `MAX_SINK_ERRORS` (3) sink errors
     in a row with no frame shown in between; `NeedKeyframe` errors don't count. Test:
     `falls_back_after_repeated_sink_errors`.
   - Left for macOS: sleep/wake and long hidden periods (`requiresFlushToResumeDecoding` path is written but
     untested); several PiPs at once; multi-monitor / different scale factors; hand-drag check
     by the user; maybe `sampleBufferRenderer` when the minimum macOS goes to 14.
   - **Rounded corners (2026-10-19)**, to match Windows 11: `round_corners` in main.rs makes the NSWindow
     non-opaque with a clear background and clips the content view (video view + web view) to an 8 pt
     continuous-curve rounded rect with a black layer background; the shadow follows (AppKit derives it
     from the window's alpha; `invalidateShadow` after changes). Checked at rest and mid live-resize (held
     drag): no square flashes, no slivers. Square in fullscreen. Opening size is exact on macOS (480×270
     asked → 480×270 content), so the non-Windows `set_inner_size` helpers are fine as they are.
   - **Fullscreen fixed on macOS (was broken):** native fullscreen can't work for our windows, which are
     "fullscreen auxiliary" (so they float over other apps' fullscreen Spaces); tao's attempt silently
     failed, left a title bar on the window and dropped it to the normal level. Now `player.js` calls the
     app's `fullscreen` command (`on: null` toggles), which on macOS uses simple fullscreen (covers the
     screen in place), raises the window above the menu bar/Dock while fullscreen, restores the level after,
     and gives the WKWebView first-responder back (tao's style-mask change took it, so keys stopped
     working). **Shared change:** on Windows/other the command does exactly what `player.js` did before
     (`set_fullscreen(!is_fullscreen)`), but check F / double-click / Esc there.
   - Speed control checked on macOS: button 1→1.5→2×, S, `>`/`<` all change the tab's `playbackRate`
     and the label follows; latency reports at 2× (72–82 ms) match 1×, so the audio delay is unaffected.

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
