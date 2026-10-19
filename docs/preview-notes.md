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

### Dependency source

Bundled third-party notices list the dependency versions and their licenses. Unmodified
sources are available from the corresponding version pages on crates.io. In particular,
the MPL-2.0 components included in the dependency notices are available under MPL-2.0:
[dtoa-short 0.3.5](https://crates.io/crates/dtoa-short/0.3.5),
[cssparser-macros 0.7.1](https://crates.io/crates/cssparser-macros/0.7.1),
[cssparser 0.37.0](https://crates.io/crates/cssparser/0.37.0),
[option-ext 0.2.0](https://crates.io/crates/option-ext/0.2.0), and
[selectors 0.38.0](https://crates.io/crates/selectors/0.38.0).
