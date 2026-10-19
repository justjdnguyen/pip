### What's new in preview.3

- New Better PiP logo across the browser extension and Windows/macOS companion icons.
- Rewritten README with clearer downloads, installation steps, and keyboard shortcuts.
- Documented the Media Feature Pack requirement for Windows N editions.

This release refreshes branding and documentation; playback behavior is unchanged
from preview.2.

### Installation and preview limitations

Early preview of Better PiP: native Picture-in-Picture windows for Windows and macOS,
plus a Chromium browser extension with captions, playback controls, and speed shortcuts.

Install the extension ZIP with Developer mode → Load unpacked. For native windows,
also download the companion archive for your OS. Windows users run Install.cmd;
Mac users run bash install.sh from the extracted folder. No developer tools required.
The extension is not in the Chrome Web Store yet.

Preview companions have no trusted publisher signature and are not Apple-notarized.
Read README.md and docs/INSTALL.md in the repository before installation. Verify
downloads against SHA256SUMS.txt. Protected video, multiple monitors/DPI, simultaneous
PiPs, and Mac sleep/wake have known limitations or need broader testing. Intel Mac
builds are compile-tested in CI and need user testing on real hardware.

Please report reproducible problems with your OS, browser, video site, and whether
the companion was installed. Remove personal information from any logs or screenshots.
Windows N editions also need Microsoft's Media Feature Pack for Media Foundation.

### Third-party licenses

Companion downloads include `THIRD-PARTY-LICENSES.html` with dependency licenses
and links to their source code.
