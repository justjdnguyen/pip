### What's new in preview.4

- Monitor-edge and PiP-to-PiP snapping on Windows and macOS, with a brief hold
  and continued-drag release.
- Resize snapping keeps the opposite edge anchored and the video in proportion.
  Wider edge and corner targets make resizing easier.
- YouTube captions continue updating in background tabs. PiP excludes clipped
  caption scrollback and updates when captions scroll or disappear.

Mac snapping was exercised live on an Intel Retina Mac with two native PiPs:
visible screen edges, hold and breakthrough, PiP-to-PiP contact, and anchored
aspect-preserving resize. Multiple monitors and mixed display scaling remain
untested. Caption tests and the browser layout fixture pass on Mac; full
background-tab YouTube timing still needs a live extension retest. Update both
the extension and companion, then refresh your video tabs.

### Installation and preview limitations

Early preview of Better PiP: native Picture-in-Picture windows for Windows and macOS,
plus a Chromium browser extension with captions, playback controls, and speed shortcuts.

Install the extension ZIP with Developer mode → Load unpacked. For native windows,
also download the companion archive for your OS. Windows users run Install.cmd;
Mac users run bash install.sh from the extracted folder. No developer tools required.
The extension is not in the Chrome Web Store yet.

Preview companions have no trusted publisher signature and are not Apple-notarized.
Read README.md and docs/INSTALL.md in the repository before installation. Verify
downloads against SHA256SUMS.txt. Protected video, multiple monitors/DPI, more
than two simultaneous PiPs, and Mac sleep/wake need broader testing. Intel Mac
checks and playback were run on real hardware; Apple Silicon remains untested.

Please report reproducible problems with your OS, browser, video site, and whether
the companion was installed. Remove personal information from any logs or screenshots.
Windows N editions also need Microsoft's Media Feature Pack for Media Foundation.

### Third-party licenses

Companion downloads include `THIRD-PARTY-LICENSES.html` with dependency licenses
and links to their source code.
