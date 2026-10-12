# Operating the supragnosis standalone daemon (macOS)

> Linux: see [systemd/README.md](systemd/README.md) - a user unit plus the federation-hub setup.
>
> Container: see [docker/README.md](docker/README.md) - the shape a hub wants. Not this one: the
> viewer and MCP surfaces below are local-only by design, and a container has no local.

Instead of spawning over stdio for each chat, **a single always-on local daemon** holds the db
and exposes MCP streamable-http. Agents (Claude Code, etc.) just connect over http. Because the
daemon is the sole holder of the db, the single-process lock problem also disappears.

- MCP: `http://127.0.0.1:7373/mcp` (loopback-only, bearer token from the 0600 `mcp.token` - the
  local trust surface, Principle 17)
- Viewer: `~/.supragnosis/viz.sock` (HTTP over a unix socket, 0600 owner-only - no TCP port)
- Both stay local. Reaching knowledge from elsewhere goes through a hub: its TLS listener carries
  the federation sync API and, for principals it admits, MCP and the viewer
  (docs/remote-server.md, docs/remote-viewer.md). It binds off loopback only with TLS and at least
  one admitted node or principal (docs/federation.md F10).

## Quick install (recommended)

```sh
bash deploy/install.sh
```

This script: builds the release -> copies the binary to `~/.local/bin/supragnosis` ->
`supragnosis service install --take-over` (generates and loads the LaunchAgent, retiring any other
manager) -> re-registers Claude Code with the http transport.

With Homebrew there is nothing to build: `brew install supragnosis-server`, then
`supragnosis service install` - or Start at Login in the desktop app's tray menu, which runs the
same command.

## Manual install

```sh
# 1) Build + put the binary on a stable path (so it survives cargo clean)
cargo build --release --bin supragnosis
mkdir -p ~/.local/bin ~/.supragnosis/redb ~/.supragnosis/log
cp target/release/supragnosis ~/.local/bin/supragnosis

# 2) Clean up any existing stdio server that is holding the db lock
pkill -f "target/release/supragnosis" || true

# 3) Generate + load the LaunchAgent (auto-start on login + restart if it dies). It names this
#    binary, logs to ~/.supragnosis/log, and carries forward the EnvironmentVariables of a plist it
#    replaces; --env SUPRAGNOSIS_X=... adds more. --take-over retires any other manager first.
~/.local/bin/supragnosis service install --take-over

# 4) Connect Claude Code through the bridge (it relays to the daemon and reads the token itself)
~/.local/bin/supragnosis connect claude-code --replace   # `supragnosis connect` lists every app
```

Now any chat/session attaches to this daemon. The viewer serves HTTP over the unix socket, e.g.
`curl --unix-socket ~/.supragnosis/viz.sock http://viz/api/graph` (the desktop shell is the
graphical client).

## Operations

The daemon has one manager, the canonical LaunchAgent `com.supragnosis.daemon`
(docs/daemon-lifecycle.md). The CLI recognizes it and every other manager the product has ever
installed - a brew services job, a `supragnosis start` pidfile daemon, retired labels - acts on
whichever single one is present, and refuses to guess when there is more than one:

```sh
supragnosis status              # who manages it, whether it answers, which version it runs
supragnosis restart             # restart it (launchctl kickstart -k), or reload the job after a stop
supragnosis stop                # stop it (launchctl bootout; down until restart or the next login)
supragnosis service uninstall   # no longer start at login (a hand-written plist is moved aside)
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
supragnosis service uninstall
claude mcp remove supragnosis -s user
```

## Notes / cautions

- After updating code, just redo `cargo build --release` + `cp target/release/supragnosis ~/.local/bin/` +
  `supragnosis restart` (re-running `install.sh` is simplest). `supragnosis status` says when the
  running daemon is older than the binary on disk.
- The plist is generated per user (no hand-edited paths). Settings that exist only as environment
  variables - `SUPRAGNOSIS_HOST` (recorded in provenance), `_WORKSPACE`, `_EMBED`, `_DATA_DIR` - go
  in with `supragnosis service install --env KEY=VALUE` and are carried forward on reinstall.
- To use real semantic embeddings, build with `--features fastembed` (that build makes `fastembed`
  the default embedder) and reinstall. A store the prebuilt binary wrote has no embedder recorded,
  so it takes the new one; `supragnosis reproject` then gives its existing entities vectors, while
  observations written before the switch stay keyword-only. A store indexed with a different model
  is refused at open, since two models share no vector space - point that one at a new
  `SUPRAGNOSIS_DATA_DIR`.
- Only one daemon should run (single ownership of the db + ports), and only one manager should
  run it - `supragnosis status` reports a conflict, and `service install --take-over` resolves it.
  Do not use stdio registration and http registration at the same time.

## Cutting a release

The version lives in **three** places, and the `release:` commit moves all of them:

- `Cargo.toml` (`[workspace.package] version`)
- `app/Cargo.toml` (same key, its own workspace since the desktop shell was split out)
- `server.json` (`version`, and the tag inside `packages[0].identifier`)

They ship as one number - `supragnosis-vX.Y.Z-*.tar.gz`, `Supragnosis-vX.Y.Z-macos-universal.app.zip`
and `ghcr.io/ashon/supragnosis:X.Y.Z`. The release workflow fails if `server.json` and the image tag
disagree, so a missed bump is a red build rather than a registry entry pointing at nothing.
