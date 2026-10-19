<img src="assets/icon.svg" width="64" height="64" alt="">

# Better PiP

Firefox-style Picture-in-Picture for Chromium browsers (Chrome, Helium, Brave, Arc, Edge…).

- **Hover button** on the right edge of any video ("Watch in Picture-in-Picture")
- **Placeholder** in the page while the video is popped out; click it to bring the video back
- **PiP window controls:** scrubber with hover time, play/pause, skip ±5s, volume + mute,
  captions, fullscreen, back to tab (top left), close (top right)
- **Captions** for standard `<track>` subtitles, plus YouTube's own captions
- Controls scale with the window size and auto-hide

## Install (unpacked)

1. Open `chrome://extensions` (Helium: `helium://extensions`) and turn on **Developer mode**.
2. Click **Load unpacked** and pick this folder.
3. Reload any tabs that were already open.

## Shortcuts

| Where | Key | Action |
| --- | --- | --- |
| Page | `Alt+P` (`⌥P`) | Toggle PiP for the main video |
| PiP | `Space` / `K` | Play / pause |
| PiP | `←` `→` | Skip 5s |
| PiP | `J` `L` | Skip 10s |
| PiP | `↑` `↓` | Volume |
| PiP | `M` / `C` / `F` | Mute / captions / fullscreen |
| PiP | `0`–`9`, `Home`, `End` | Jump to position |

Double-click the PiP video to go fullscreen.

## How it works

The extension uses Chrome's [Document Picture-in-Picture API](https://developer.chrome.com/docs/web-platform/document-picture-in-picture)
to open an always-on-top window, then moves the page's real `<video>` element into it
(so it keeps playing, with no re-buffering) and draws the controls around it.
Site-specific behaviour (captions, fullscreen, layout) lives in `src/content/adapters.js`,
similar to Firefox's PiP "site wrappers".

## Known limits (Chromium, not this extension)

- **One PiP window at a time.** Chromium closes the existing PiP window when another opens.
- **Max size ≈ 80% of one display.** The window can't be stretched across monitors.
- Videos inside **cross-origin iframes** (e.g. YouTube embeds on other sites) can't be moved,
  so they fall back to Chrome's built-in PiP with its basic controls.

## Development

- `node tools/make-icons.mjs` regenerates the PNG icons.
- `node tools/e2e.mjs` loads the extension in Helium, opens `test/index.html`, clicks through
  PiP and writes screenshots to `test/out/`. Set `PAGE=<url>` to test a real site.
