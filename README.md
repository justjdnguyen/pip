<img src="assets/wordmark.svg" width="960" alt="Better PiP">

# Your video, above it all.

Picture-in-Picture for Chromium browsers, with native floating windows on Windows
and macOS. Keep a video in view while you work, read, or switch tabs.

**[Download the preview](https://github.com/justjdnguyen/pip/releases/tag/v0.1.0-preview.4)**
&nbsp; [Installation guide](docs/INSTALL.md) &nbsp; [Report a bug](https://github.com/justjdnguyen/pip/issues)

## A window that follows you

Hover over a video and click the PiP button. Drag the window where you want it,
resize it without losing its shape, and keep the controls within reach.

- **Make room.** Native video follows the window as you resize. The companion
  supports multiple windows and avoids Document PiP's maximum-size restriction.
- **Line things up.** On Windows and macOS, drag near a monitor edge or another PiP to
  snap into place, or resize toward an edge while keeping the opposite side
  anchored. Keep dragging past the edge to break through the brief hold.
- **Set the pace.** Click the speed button or press `S` to cycle **1×, 1.5×, 2×**.
  Use `<` and `>` for finer steps.
- **Keep control.** Scrub, skip, adjust volume, toggle captions, or go fullscreen.
  Return the video to its tab when you're done.
- **Keep it local.** Video travels from the tab to the companion on your computer.
  No account, analytics, or project-operated server. [Privacy details](PRIVACY.md)

Standard video subtitles and YouTube captions are supported where available.
Audio stays in the source tab and is delayed to match the companion picture.

## Get started

Install the browser extension first, then the companion for native windows.
**No Rust, Node.js, or build tools required.**

### Browser extension

1. Download and extract the **extension ZIP** from the
   [preview release](https://github.com/justjdnguyen/pip/releases/tag/v0.1.0-preview.4)
   into a folder you will keep.
2. Open `chrome://extensions`, turn on **Developer mode**, and click **Load unpacked**.
   Select the folder containing `manifest.json`.
3. Reload your video tabs. Hover over a video and click its PiP button, or press
   `Alt+P` (`Option+P` on a Mac).

In Helium, use `helium://extensions`; in Edge, use `edge://extensions`.
There is no Chrome Web Store listing yet.

### Desktop companion

Choose your companion from the same release:

- **Windows 11, x64:** extract the Windows ZIP and double-click **Install.cmd**.
- **Apple silicon:** extract the macOS arm64 archive and run **bash install.sh**
  in its folder.
- **Intel Mac:** extract the macOS x64 archive and run **bash install.sh**
  in its folder.

Restart your browser. It will launch the companion automatically when needed.
Installation is per user and requires no administrator access. Open each browser
once before installing the Mac companion.

The preview targets **Windows 11** and **macOS 13.3+**. WebView2 is required on Windows;
Windows N editions also need the Media Feature Pack. The companion is not signed
with a trusted publisher certificate or Apple-notarized. Read the
[installation guide](docs/INSTALL.md) for OS approval, updates, and uninstallation.

**Prefer extension only?** Without the companion, Better PiP tries Document PiP,
then the browser's built-in video PiP. Window limits and available controls depend
on the browser. There is no supported Linux companion build yet.

## Keep your hands on the keys

In the page, **Alt+P** / **Option+P** toggles PiP for the main video.
Inside the PiP window:

- **Space / K** plays or pauses. **← / →** skips 5 seconds; **J / L** skips 10.
- **↑ / ↓** changes volume. **M** mutes; **C** toggles captions.
- **S** cycles speed. **< / >** steps between 0.25× and 2×.
- **F** or a double-click toggles fullscreen. **Esc** exits it.
- **0–9**, **Home**, and **End** jump to a position.

## An early preview

The main test setup is Helium with YouTube. Other Chromium browsers can load the
extension, but behavior varies by browser, site, and codec.

- DRM-protected video and some cross-origin sources cannot be captured.
- Browser-only Document PiP permits one window at a time and caps its size.
- Multiple PiPs, mixed-DPI monitors, and Mac sleep/wake need broader real-world testing.
- Native playback uses H.264. Web fallback may use more CPU or feel less smooth.
- Intel Mac builds are tested in CI; playback still needs testing on more real machines.
- Updates are manual: install the new companion, replace/reload the extension,
  and refresh your video tabs.

Found a problem? [Open an issue](https://github.com/justjdnguyen/pip/issues) with your
OS, browser version, video site, and whether you installed the companion. Redact
personal information from logs and screenshots before sharing them.

## Build something better

Bug reports, browser compatibility results, and focused pull requests are welcome.

[Build from source](CONTRIBUTING.md) · [Release process](docs/RELEASING.md) ·
[Implementation notes](HANDOFF.md)

Better PiP is [MIT licensed](LICENSE). Third-party components keep their own
licenses; companion downloads include their notices and source locations.
