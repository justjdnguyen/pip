#!/bin/bash
# Registers the Better PiP companion app with every Chromium browser installed
# for this user, so the extension can launch it. Safe to run again.
#
# Usage: tools/install-host-mac.sh [path/to/app/binary]
#   Defaults to the .app bundle if built, else the release binary, else the debug binary.
#   tools/install-host-mac.sh --uninstall   removes the registrations.
set -euo pipefail

HOST_NAME="com.betterpip.host"
EXTENSION_ID="fblfebboimckibidkphfchjlmohondgm"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SUPPORT="$HOME/Library/Application Support"

# Each browser's data folder (the registration goes in <folder>/NativeMessagingHosts).
BROWSERS=(
  "net.imput.helium"
  "Google/Chrome"
  "Google/Chrome Beta"
  "Google/Chrome Canary"
  "Chromium"
  "BraveSoftware/Brave-Browser"
  "Microsoft Edge"
  "Vivaldi"
  "Arc/User Data"
)

if [[ "${1:-}" == "--uninstall" ]]; then
  for b in "${BROWSERS[@]}"; do
    f="$SUPPORT/$b/NativeMessagingHosts/$HOST_NAME.json"
    [[ -f "$f" ]] && rm "$f" && echo "removed: $b"
  done
  exit 0
fi

BIN="${1:-}"
if [[ -z "$BIN" ]]; then
  APP="$ROOT/companion/src-tauri/target/release/bundle/macos/Better PiP.app/Contents/MacOS/better-pip-host"
  RELEASE="$ROOT/companion/src-tauri/target/release/better-pip-host"
  DEBUG="$ROOT/companion/src-tauri/target/debug/better-pip-host"
  if [[ -x "$APP" ]]; then BIN="$APP"; elif [[ -x "$RELEASE" ]]; then BIN="$RELEASE"; elif [[ -x "$DEBUG" ]]; then BIN="$DEBUG"; else
    echo "Build the app first: (cd companion/src-tauri && cargo build)" >&2; exit 1
  fi
fi
BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"

MANIFEST=$(cat <<JSON
{
  "name": "$HOST_NAME",
  "description": "Better PiP companion app",
  "path": "$BIN",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://$EXTENSION_ID/"]
}
JSON
)

installed=0
for b in "${BROWSERS[@]}"; do
  dir="$SUPPORT/$b"
  [[ -d "$dir" ]] || continue
  mkdir -p "$dir/NativeMessagingHosts"
  printf '%s\n' "$MANIFEST" > "$dir/NativeMessagingHosts/$HOST_NAME.json"
  echo "registered: $b"
  installed=$((installed + 1))
done
echo "App: $BIN"
[[ $installed -gt 0 ]] || echo "No Chromium browsers found." >&2
