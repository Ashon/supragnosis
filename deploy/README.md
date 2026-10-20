# Operating the supragnosis standalone daemon (macOS)

> Linux: see [systemd/README.md](systemd/README.md) - a user unit plus the federation-hub setup.
>
> Container: see [docker/README.md](docker/README.md) - the shape a hub wants. Not this one: the
> viewer and MCP surfaces below are local-only by design, and a container has no local.

Instead of spawning over stdio for each chat, **a single always-on local daemon** holds the db
and exposes MCP streamable-http. Agents (Claude Code, etc.) reach it through `supragnosis bridge`,
a stdio relay every client can run that reads the token from its file, so no client configuration
holds a copy (docs/client-connect.md). Because the daemon is the sole holder of the db, the
single-process lock problem also disappears.

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
manager) -> connects Claude Code through the bridge.

With Homebrew there is nothing to build: `brew install supragnosis-server`, then
`supragnosis service install` - or Start at Login on the desktop app's Settings page, which runs
the same command.

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
supragnosis service uninstall              # no longer started at login
supragnosis connect                        # lists the AI apps and how each is connected
supragnosis connect claude-code --remove   # once per connected app
rm ~/.local/bin/supragnosis                # or brew uninstall supragnosis-server
```

`~/.supragnosis` holds the knowledge itself - the store, the node's identity key, the logs - and
nothing above touches it. Remove it only if the knowledge should go too.

## Notes / cautions

- After updating code, just redo `cargo build --release` + `cp target/release/supragnosis ~/.local/bin/` +
  `supragnosis restart` (re-running `install.sh` is simplest). `supragnosis status` says when the
  running daemon is older than the binary on disk.
- The plist is generated per user (no hand-edited paths). Settings that exist only as environment
  variables - `SUPRAGNOSIS_HOST` (recorded in provenance), `_WORKSPACE`, `_EMBED`, `_DATA_DIR` - go
  in with `supragnosis service install --env KEY=VALUE` and are carried forward on reinstall.
- Semantic retrieval needs the embedding model, which `service install` fetches once into
  `~/.supragnosis/models` (224 MB; `supragnosis embed fetch` by hand, `SUPRAGNOSIS_MODEL_DIR` to
  put it elsewhere). The daemon never downloads: without the files it is keyword only and `status`
  says so. Loaded, the model holds about 1 GB; `SUPRAGNOSIS_EMBED=none` keeps the daemon small
  and keyword only, and `SUPRAGNOSIS_EMBED_MODEL=bge-small-en` is the English-only model at half
  the size ([docs/semantic-retrieval.md](../docs/semantic-retrieval.md)).
- What was written before the model has no vectors, and the daemon gives it them by itself: once
  the model's files are there it embeds every observation and entity that has none, once, and
  `supragnosis status` shows the progress (`semantic on (...) - embedding what was written before
  the model: 40 of 120`). No `reproject` is needed for it. A store indexed with a different model
  is refused at open, since two models share no vector space: with the daemon stopped,
  `supragnosis embed reset` removes its vectors and the next start embeds it with the model the
  daemon runs ([docs/semantic-retrieval.md](../docs/semantic-retrieval.md) Section 13).
- Only one daemon should run (single ownership of the db + ports), and only one manager should
  run it - `supragnosis status` reports a conflict, and `service install --take-over` resolves it.
  Register each AI app through `supragnosis connect`, which writes the bridge. An app that runs
  plain `supragnosis` over stdio opens the store itself, and beside the daemon it fails on the lock.

## Cutting a release

The version lives in **three** places, and the `release:` commit moves all of them:

- `Cargo.toml` (`[workspace.package] version`)
- `app/Cargo.toml` (same key, its own workspace since the desktop shell was split out)
- `server.json` (`version`, and the tag inside `packages[0].identifier`)

They ship as one number - `supragnosis-vX.Y.Z-*.tar.gz`, `Supragnosis-vX.Y.Z-macos-arm64.app.zip`
and `ghcr.io/ashon/supragnosis:X.Y.Z`. The release workflow fails if `server.json` and the image tag
disagree, so a missed bump is a red build rather than a registry entry pointing at nothing.
