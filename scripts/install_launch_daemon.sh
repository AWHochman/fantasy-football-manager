#!/bin/bash
set -euo pipefail

LABEL="com.austinhochman.fantasy-football-manager"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
USER_NAME="$(id -un)"
USER_HOME="$HOME"
PLIST_PATH="/Library/LaunchDaemons/$LABEL.plist"
LOG_DIR="$USER_HOME/Library/Logs/fantasy-football-manager"
TEMP_PLIST="$(mktemp)"

cleanup() {
    rm -f "$TEMP_PLIST"
}
trap cleanup EXIT

if [[ ! -f "$PROJECT_ROOT/.env" ]]; then
    echo "Missing $PROJECT_ROOT/.env. Configure it before installing the daemon." >&2
    exit 1
fi

mkdir -p "$LOG_DIR"
chmod 600 "$PROJECT_ROOT/.env"

cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml"

cat >"$TEMP_PLIST" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$PROJECT_ROOT/target/release/monitor</string>
  </array>
  <key>WorkingDirectory</key>
  <string>$PROJECT_ROOT</string>
  <key>UserName</key>
  <string>$USER_NAME</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key>
    <string>$USER_HOME</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>StartInterval</key>
  <integer>1800</integer>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>$LOG_DIR/monitor.log</string>
  <key>StandardErrorPath</key>
  <string>$LOG_DIR/monitor.error.log</string>
</dict>
</plist>
PLIST

plutil -lint "$TEMP_PLIST"
sudo install -o root -g wheel -m 644 "$TEMP_PLIST" "$PLIST_PATH"
sudo launchctl bootout "system/$LABEL" 2>/dev/null || true
sudo launchctl bootstrap system "$PLIST_PATH"
sudo launchctl kickstart -k "system/$LABEL"

echo "Installed $LABEL. It runs immediately and then every 30 minutes."
echo "Logs: $LOG_DIR/monitor.log and $LOG_DIR/monitor.error.log"
