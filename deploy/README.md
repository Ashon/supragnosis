# Operating the supragnosis standalone daemon (macOS)

Instead of spawning over stdio for each chat, **a single always-on local daemon** holds the db
and exposes MCP streamable-http. Agents (Claude Code, etc.) just connect over http. Because the
daemon is the sole holder of the db, the Cozo single-process lock problem also disappears.

- MCP: `http://127.0.0.1:7373/mcp` (loopback-only, no auth = local trust surface, Principle 17)
- Viewer: `~/.supragnosis/viz.sock` (HTTP over a unix socket, 0600 owner-only - no TCP port)
- All local. Non-local exposure / auth / TLS is not supported yet (later).

## Quick install (recommended)

```sh
bash deploy/install.sh
```

This script: builds the release -> copies the binary to `~/.local/bin/supragnosis` ->
installs and loads the LaunchAgent -> re-registers Claude Code with the http transport.

## Manual install

```sh
# 1) Build + put the binary on a stable path (so it survives cargo clean)
cargo build --release --bin supragnosis
mkdir -p ~/.local/bin ~/.supragnosis/db ~/.supragnosis/log
cp target/release/supragnosis ~/.local/bin/supragnosis

# 2) Clean up any existing stdio server that is holding the db lock
pkill -f "target/release/supragnosis" || true

# 3) Install + load the LaunchAgent (auto-start on login + restart if it dies)
cp deploy/launchd/com.supragnosis.daemon.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.supragnosis.daemon.plist

# 4) Register Claude Code with the http transport (no more spawning per chat)
claude mcp remove supragnosis -s user 2>/dev/null || true
claude mcp add supragnosis --transport http http://127.0.0.1:7373/mcp --scope user
```

Now any chat/session attaches to this daemon. The viewer serves HTTP over the unix socket, e.g.
`curl --unix-socket ~/.supragnosis/viz.sock http://viz/api/graph` (the desktop shell is the
graphical client).

## Operations

The daemon uses the canonical launchd label `com.supragnosis.daemon`, so the `supragnosis`
CLI drives it directly (it detects the launchd job and delegates to launchctl). This restarts
the MCP server **and** the viewer in one command:

```sh
supragnosis status    # server + viewer state (self-managed or launchd)
supragnosis restart   # restart both (launchctl kickstart -k under the hood)
supragnosis stop      # stop both (launchctl bootout; stays down until reloaded)
```

Underlying launchctl (equivalent to the above), plus logs:

```sh
# status / logs
launchctl list | grep supragnosis
tail -f ~/.supragnosis/log/supragnosis.err.log

# stop / restart (raw)
launchctl bootout   gui/$(id -u)/com.supragnosis.daemon
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.supragnosis.daemon.plist
launchctl kickstart -k gui/$(id -u)/com.supragnosis.daemon   # restart in place

# full removal
launchctl bootout gui/$(id -u)/com.supragnosis.daemon 2>/dev/null || true
rm ~/Library/LaunchAgents/com.supragnosis.daemon.plist
claude mcp remove supragnosis -s user
```

## Notes / cautions

- After updating code, just redo `cargo build --release` + `cp target/release/supragnosis ~/.local/bin/` +
  a daemon restart (above) (re-running `install.sh` is simplest).
- The paths in the plist are absolute paths based on the user `ashon.lee`. Other users should adjust
  the `/Users/...` paths and `Label` inside the plist, and the paths in `deploy/install.sh`.
- The embedder is `hashing` (zero downloads, deterministic). To use real semantic embeddings, build
  with `--features fastembed` and switch the plist to `SUPRAGNOSIS_EMBED=fastembed` + a **new `SUPRAGNOSIS_DATA_DIR`**
  (the existing db is indexed with hashing-256, so swapping the embedder is rejected).
- Only one daemon should run (single ownership of the db + ports). Do not use stdio registration and http registration at the same time.
