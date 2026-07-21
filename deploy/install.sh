#!/usr/bin/env bash
# supragnosis standalone daemon install (macOS/launchd).
# - Copy the release binary to a stable path (~/.local/bin) (so it survives cargo clean)
# - Install + load the LaunchAgent plist (auto-start at login, restart if it dies)
# - Re-register Claude Code with the http transport
# Run: bash deploy/install.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_SRC="$REPO_ROOT/target/release/supragnosis"
BIN_DST="$HOME/.local/bin/supragnosis"
PLIST_SRC="$REPO_ROOT/deploy/launchd/com.ashon.supragnosis.plist"
PLIST_DST="$HOME/Library/LaunchAgents/com.ashon.supragnosis.plist"
MCP_URL="http://127.0.0.1:7373/mcp"

echo "[1/5] Release build"
( cd "$REPO_ROOT" && cargo build --release --bin supragnosis )

echo "[2/5] Stop existing daemon (release db lock/binary hold)"
mkdir -p "$HOME/.local/bin" "$HOME/.supragnosis/db" "$HOME/.supragnosis/log"
# Stop first before replacing - overwriting a file while running breaks the mapping. Stop the launchd-managed
# process with unload, and any leftover with pkill (by install path - the daemon runs from $BIN_DST).
launchctl unload "$PLIST_DST" 2>/dev/null || true
pkill -f "$BIN_DST" 2>/dev/null || true

echo "[3/5] Install binary + load LaunchAgent"
# In-place overwrite (cp over) triggers SIGKILL ('killed: 9') on exec due to a macOS code-signing cache
# mismatch - avoid it by replacing with a new inode (rm then cp).
rm -f "$BIN_DST"
cp "$BIN_SRC" "$BIN_DST"
cp "$PLIST_SRC" "$PLIST_DST"
launchctl load "$PLIST_DST"
sleep 1

echo "[4/5] Health check ($MCP_URL)"
# A GET without initialize returns 405/event, but this only checks whether the port is open.
if curl -s -o /dev/null -m 3 "http://127.0.0.1:7373/mcp" ; then echo "  MCP port responds OK"; else echo "  (may still be starting up - check the logs)"; fi

echo "[5/5] Register Claude Code with the http transport"
claude mcp remove supragnosis -s user 2>/dev/null || true
claude mcp add --transport http supragnosis "$MCP_URL" --scope user

echo ""
echo "Done. Viewer: http://127.0.0.1:7374 | Logs: ~/.supragnosis/log/"
echo "Stop:  launchctl unload $PLIST_DST"
echo "Restart: launchctl unload $PLIST_DST && launchctl load $PLIST_DST"
