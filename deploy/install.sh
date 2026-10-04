#!/usr/bin/env bash
# supragnosis standalone daemon install (macOS/launchd).
# - Copy the release binary to a stable path (~/.local/bin) (so it survives cargo clean)
# - Generate + load the LaunchAgent (supragnosis service install: auto-start at login, restart if it dies)
# - Re-register Claude Code with the http transport
# Run: bash deploy/install.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN_SRC="$REPO_ROOT/target/release/supragnosis"
BIN_DST="$HOME/.local/bin/supragnosis"
LABEL="com.supragnosis.daemon"
PLIST_DST="$HOME/Library/LaunchAgents/$LABEL.plist"
MCP_URL="http://127.0.0.1:7373/mcp"

echo "[1/5] Release build"
( cd "$REPO_ROOT" && cargo build --release --bin supragnosis )

echo "[2/5] Stop existing daemon (release db lock/binary hold)"
mkdir -p "$HOME/.local/bin" "$HOME/.supragnosis/redb" "$HOME/.supragnosis/log"
# Stop first before replacing - overwriting a file while running breaks the mapping. Stop the launchd-managed
# process with unload, and any leftover with pkill (by install path - the daemon runs from $BIN_DST).
# Other managers (a brew services job, the retired com.ashon.supragnosis label) are retired by
# `service install --take-over` below, which also moves a hand-written plist aside rather than losing it.
launchctl unload "$PLIST_DST" 2>/dev/null || true
pkill -f "$BIN_DST" 2>/dev/null || true

echo "[3/5] Install binary + generate/load the LaunchAgent"
# In-place overwrite (cp over) triggers SIGKILL ('killed: 9') on exec due to a macOS code-signing cache
# mismatch - avoid it by replacing with a new inode (rm then cp).
rm -f "$BIN_DST"
cp "$BIN_SRC" "$BIN_DST"
"$BIN_DST" service install --take-over

echo "[4/5] Health check ($MCP_URL)"
# A GET without initialize returns 405/event, but this only checks whether the port is open.
if curl -s -o /dev/null -m 3 "http://127.0.0.1:7373/mcp" ; then echo "  MCP port responds OK"; else echo "  (may still be starting up - check the logs)"; fi

echo "[5/5] Connect Claude Code through the bridge"
# `connect` registers `supragnosis bridge`, which reads the bearer token from its 0600 file itself,
# so Claude Code's settings hold no copy of it (docs/client-connect.md). --replace switches an
# earlier http entry, the one that did hold a copy, over to the bridge.
"$BIN_DST" connect claude-code --replace || echo "  Claude Code not found - 'supragnosis connect' lists the apps it can connect"

echo ""
echo "Done. Viewer socket: ~/.supragnosis/viz.sock (HTTP over UDS) | Logs: ~/.supragnosis/log/"
echo "Control (label $LABEL):"
echo "  supragnosis status    # server + viewer state"
echo "  supragnosis restart   # restart both (launchctl kickstart -k)"
echo "  supragnosis stop      # stop both (launchctl bootout; down until restart or the next login)"
echo "  supragnosis service uninstall   # no longer start at login"
