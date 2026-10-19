# Privacy

Better PiP has no analytics, advertising, accounts, or project-operated backend.
The extension and companion do not upload video, captions, browsing history, or
playback data to the developer. Websites and browsers retain their own network
behavior and privacy policies.

## Data handled locally

The extension detects video elements and adds a hover button. In companion mode,
captured video frames, playback state, captions, the page title, and window/latency
messages travel to the local companion through native messaging. Audio stays in
the source tab, optionally routed through a local Web Audio delay for synchronization.
Settings such as PiP width are stored in the browser's local extension storage.

Hovering a playing video can warm the capture/encoder pipeline and start the companion
so opening PiP is faster. This is local and does not upload frames.

## Permissions

- Page access on all sites: find videos, show the toggle, capture video where supported,
  and operate the selected video. Content scripts also run in embedded frames.
- `storage`: remember local preferences.
- `nativeMessaging`: communicate with the installed companion on the same computer.

No browser history or microphone permission is requested. Browser-managed updates,
OS publisher checks, and the source website are outside this policy.

## Debugging and reports

Debug logs are opt-in through `BP_DEBUG_LOG`. They can include page titles, captions,
playback state, local paths, and timing information; the app logger omits video chunks.
Development tools can create screenshots and recordings. Inspect and redact these
before sharing them in an issue.

The project receives only information you deliberately submit, such as GitHub issues
and pull requests. GitHub handles that information under its own privacy policy.
