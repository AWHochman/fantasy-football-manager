#!/bin/bash
set -euo pipefail

LABEL="com.austinhochman.fantasy-football-manager"
PLIST_PATH="/Library/LaunchDaemons/$LABEL.plist"

sudo launchctl bootout "system/$LABEL" 2>/dev/null || true
sudo rm -f "$PLIST_PATH"

echo "Uninstalled $LABEL. Local logs and alert state were left in place."
