#!/bin/bash
set -euo pipefail
PACKAGE_DIR="$(cd "$(dirname "$0")" && pwd)"
bash "$PACKAGE_DIR/tools/install-host-mac.sh" --uninstall
rm -f "$HOME/Library/Application Support/BetterPiP/bin/better-pip-host"
for notice in LICENSE THIRD-PARTY-LICENSES.html PRIVACY.md; do
  rm -f "$HOME/Library/Application Support/BetterPiP/bin/$notice"
done
echo 'Companion uninstalled. Remove the browser extension separately.'
