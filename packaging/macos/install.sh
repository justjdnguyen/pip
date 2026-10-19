#!/bin/bash
set -euo pipefail
PACKAGE_DIR="$(cd "$(dirname "$0")" && pwd)"
INSTALL_DIR="$HOME/Library/Application Support/BetterPiP/bin"
SOURCE="$PACKAGE_DIR/better-pip-host"
[[ -f "$SOURCE" ]] || { echo 'Extract the complete companion archive first.' >&2; exit 1; }
mkdir -p "$INSTALL_DIR"
# A new file plus rename also supports updating while an older process is running.
TEMP_BIN="$(mktemp "$INSTALL_DIR/.better-pip-host.XXXXXX")"
trap 'rm -f "$TEMP_BIN"' EXIT
cp "$SOURCE" "$TEMP_BIN"
chmod 755 "$TEMP_BIN"
for notice in LICENSE THIRD-PARTY-LICENSES.html PRIVACY.md; do
  cp "$PACKAGE_DIR/$notice" "$INSTALL_DIR/$notice"
done
mv -f "$TEMP_BIN" "$INSTALL_DIR/better-pip-host"
bash "$PACKAGE_DIR/tools/install-host-mac.sh" "$INSTALL_DIR/better-pip-host"
echo 'Installed. Restart your browser and load the Better PiP extension.'
