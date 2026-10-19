# Installation and troubleshooting

## What to download

Every release includes an extension ZIP and separate companion archives. Install
the extension in each browser you use. Install the companion once per OS user.
The companion uses local native messaging; it does not open a network server.

The preview extension is loaded through **Developer mode → Load unpacked**. It is
not installed by dragging a ZIP onto a page and is not yet in the Chrome Web Store.
The fixed unpacked extension ID is `fblfebboimckibidkphfchjlmohondgm`.

## Windows

1. Extract the Windows x64 companion ZIP and run `Install.cmd`.
2. Load the extracted extension folder through `chrome://extensions`.
3. Restart the browser, open a video, and click the PiP button.

The installer copies the executable to `%LOCALAPPDATA%\BetterPiP\app` and registers
Chrome, Chromium/Helium, Edge, Brave, and Vivaldi under HKCU. It does not require
administrator access or add a login/startup task. You may remove the extracted
companion archive after installation; keep the unpacked extension folder.

The executable and scripts have no trusted publisher signature in this preview.
Verify that the download came from this project's release and compare its SHA-256
with `SHA256SUMS.txt` if needed (`Get-FileHash <download> -Algorithm SHA256`). Managed
devices may prohibit unsigned apps or developer-mode extensions. Do not disable
organization security policies. Install WebView2 Runtime if it is missing.
The companion also uses Windows Media Foundation. Windows N editions need Microsoft's
[Media Feature Pack](https://support.microsoft.com/en-us/windows/experience/platform-variants/media-feature-pack-for-windows-n).
An error about `mfplat.dll` usually means those OS media components are missing.

To uninstall: close the browser/PiP windows, extract the same companion ZIP if needed,
and run `Uninstall.cmd`. Remove the extension from the browser separately.
Updates use the same installer and stable install location. If Windows reports the
executable is in use, close browsers using Better PiP and retry.

## macOS

1. Use the arm64 archive for Apple silicon or x64 for Intel.
2. Extract the archive, open Terminal in that directory, and run `bash install.sh`.
3. Load the extension unpacked, restart the browser, and open PiP.

The companion goes in `~/Library/Application Support/BetterPiP/bin`; browser manifests
go in supported browser profile directories. No administrator password or startup
agent is required. Run the installer again after adding a new browser.

These previews do not use an Apple Developer ID certificate and are not notarized.
If macOS blocks the binary and you trust the exact release you downloaded, follow
[Apple's instructions](https://support.apple.com/en-us/102445) for reviewing the alert
and approving that particular app in **System Settings → Privacy & Security**.
The installer does not disable Gatekeeper or remove quarantine attributes.
Do not bypass an alert saying software is malicious or damaged.

To uninstall: close browsers/PiP windows and run `bash uninstall.sh` from the archive.
Remove the browser extension separately. Updating uses `bash install.sh` from a newer
archive; replace/reload the extension too.

## If it does not work

- Reload the extension, then reload video tabs. Old tabs retain old content scripts.
- Keep the extension's unpacked folder at the location you loaded it from.
- Confirm the extension ID matches the value above. Custom builds with a different
  extension ID need matching `allowed_origins` in native host registration.
- If you only see the browser's basic PiP controls, the companion may be unavailable,
  the site may block capture, or the video may be protected.
- Test an ordinary non-DRM video before reporting a site-specific issue.
- Report OS, browser version, site, extension/companion version, and reproduction steps.
  Redact titles, captions, and paths in optional logs before sharing.
