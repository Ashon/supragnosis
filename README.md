# supragnosis

**Portable memory for AI agents - with provenance.** An embedded, file-based Rust
server: agents shed observations as a by-product of work, the knowledge moves between
your machines, tools and team, and every claim arrives carrying who said it, on what
basis, and how far to trust it. What accumulates across multiple **hosts** and
**workspaces** is an **ontology (a concept/relation graph)** you query and explore
over **MCP** - and humans govern what becomes canon.

> `supragnosis` = *supra* (above/beyond) + *gnosis* (knowing) - knowledge above knowledge (meta-knowledge).

- Language/runtime: **Rust** (`rmcp` 0.16 official MCP SDK, `tokio`)
- Store: **embedded, file-based**. `cozo`/RocksDB (default) unifies relational + graph + vector
  (HNSW); `redb` (opt-in) is a pure-Rust B-tree with no C++ toolchain. Both implement one
  `KnowledgeStore` port and are held to it by a single conformance suite.
- Status: **M4 Phase 4 federation + M3a/M3b resolution**. Semantic + keyword hybrid recall (M2),
  the **proposal gate (M3.5, both slices)** - review carries a computed belief diff of what a verdict
  would overturn and which references would rewire, and the fold enforces blocking checks so a merge
  that cannot commit does not - hub-and-spoke log replication with ed25519-signed
  events over TLS (M4 Phases 0-4), **belief resolution (M3a)** - a replaceable tier-weighted policy
  computes the current belief, contested beliefs surface for mediation, claim_promotion/demotion
  verdicts commit - and **identity resolution (M3b)** - aliases accumulate and forward, the
  conservative merge band proposes entity-merge candidates from name-embedding similarity (the gate
  commits), the resolution write path makes an incremental write equal a fresh replay, and T-Box
  definition conflicts surface contested. **Deferred**: canon effects for `tbox_change`/`recall` and
  multi-principal governance (federation remainder), induced type candidates (naming a type is
  probabilistic -> M5), and bitemporal time-travel queries (M3c, blocked on explicit negation). Per-milestone detail and the honest
  record of what is deferred: [`docs/architecture.md`](docs/architecture.md) Sections 12/14.
- Docs: architecture -> [`docs/architecture.md`](docs/architecture.md), design principles ->
  [`docs/principles.md`](docs/principles.md), proposal workflow ->
  [`docs/proposal-workflow.md`](docs/proposal-workflow.md), federation ->
  [`docs/federation.md`](docs/federation.md), belief resolution (M3a) ->
  [`docs/resolution.md`](docs/resolution.md), identity resolution (M3b spec) ->
  [`docs/resolution-identity.md`](docs/resolution-identity.md)

## Install (prebuilt binary)

### Homebrew (macOS / Linux)
```bash
brew tap ashon/tap
brew install supragnosis                # desktop app (macOS, signed/notarized) - pulls the server with it
brew install supragnosis-server         # server/CLI only (macOS / Linux)
brew services start supragnosis-server  # always-on daemon (MCP :7373 + viewer socket)
```
- `supragnosis` is the desktop-app cask and depends on the `supragnosis-server` formula - the app
  attaches to the server binary on PATH (no bundled sidecar). The installed binary is named
  `supragnosis` either way; only the brew tokens differ.
- Upgrading: `brew upgrade` swaps the binaries and relaunches the app, but does not restart a
  running daemon - follow it with `brew services restart supragnosis-server` (brew prints the same
  reminder in the formula caveats).
- **Dev channel**: `brew install --HEAD supragnosis-server` builds current `main` from source
  (rust pulled as a build dep; refresh with `brew upgrade --fetch-HEAD supragnosis-server`). The
  viewer UI is embedded in the server binary, so the stable desktop app renders the dev viewer.
  For a dev app shell too: `brew install --cask supragnosis-dev` (the rolling signed `dev`
  pre-release; refresh with `brew reinstall supragnosis-dev`).
  Swap procedure and cautions: [`deploy/homebrew/`](deploy/homebrew/).
- Tap templates and the per-release update procedure: [`deploy/homebrew/`](deploy/homebrew/).

### Install script
```bash
# Detect platform -> install the latest release binary to ~/.local/bin (with checksum verification)
curl -fsSL https://supragnosis.dev/install.sh | sh
```
- Or download the platform tar.gz directly from [Releases](https://github.com/Ashon/supragnosis/releases), extract it, and put `supragnosis` on your PATH.
- Supported platforms: macOS (arm64/x86_64), Linux (x86_64/aarch64). For other platforms, build from source below.
- The prebuilt binary is **keyword + hashing search**. For local ONNX **semantic search**, build from source with `--features fastembed`.
- On a `v*` tag push, GitHub Actions (`.github/workflows/release.yml`) builds and publishes the release.

## Development (Taskfile)
[`Taskfile.yml`](Taskfile.yml) wraps the common loops (`brew install go-task`, then `task` to list
everything). The raw `cargo` equivalents are all in the sections below - the task runner is a
convenience, not a requirement.
```bash
task dev            # the viewer UI on YOUR build - own db/socket/port, isolated from ~/.supragnosis
task dev:snapshot   # same, but on a COPY of the live daemon's knowledge (real data, nothing at risk)
task app            # shell against the already-running daemon (builds nothing)
task server         # server only (MCP http + viewer socket), no desktop shell
task check          # clippy + viewer ESLint + tests
task viz -- /api/curation   # GET the viewer API over its unix socket
```
- `task dev` starts empty, because cozo/RocksDB is single-process and a running daemon holds the
  real store. `task dev:snapshot` copies it first, so you develop against real knowledge without
  stopping anything - RocksDB is crash-consistent, so a copy taken from under a live writer opens
  the way it would after a power cut. It is a snapshot both ways: dev edits never reach the real
  store, and later daemon writes never reach dev. `task dev:live` opens the real store directly and
  refuses to start while a daemon holds it.
- `task dev` pins `SUPRAGNOSIS_VIZ_SOCK`/`DATA_DIR`/`HTTP_ADDR` on purpose. The shell does
  attach-or-spawn: if the socket it resolves already answers it attaches and never consults
  `SUPRAGNOSIS_BIN`, so an unpinned socket would silently show you an installed daemon's build
  instead of the one you just compiled. Use `task app` when attaching is what you actually want.
- **`task check` runs the same checks CI does**, so a green run locally means a green run there:
  clippy + tests ([`rust.yml`](.github/workflows/rust.yml)) and the viewer's ESLint
  ([`frontend-lint.yml`](.github/workflows/frontend-lint.yml)). Keep the two in step - a check added
  to one belongs in the other.
- There is deliberately **no dev web server task**. The viewer is unix-socket-only (see the bind
  policy in [`docs/architecture.md`](docs/architecture.md) Section 10), so `task dev` runs the
  desktop shell, which proxies its webview onto the socket via `viz://`. Proxying the socket to a
  TCP port would re-expose the browser attack class v0.1.10 deleted the defenses for, and would
  launder the `/api/review` surface ceiling - don't.

## Build & run
```bash
cargo build                                          # default (keyword search) - lightweight build
cargo build -p supragnosis-cli --features fastembed  # includes semantic search (fastembed local ONNX model)
cargo test                                           # unit tests (network-dependent fastembed tests are excluded via --ignored)
./target/debug/supragnosis                           # stdio MCP server (launched by the MCP client as a child process)
```
- Environment variables:
  - `SUPRAGNOSIS_HOST` - host id for provenance (default `localhost`). This is a display label only;
    the federation `node_id` is derived from the node keypair, not from this value.
  - `SUPRAGNOSIS_WORKSPACE` - default workspace (default `default`).
  - `SUPRAGNOSIS_STORE` - `cozo` (default, file-persistent) | `redb` (file-persistent, pure Rust) |
    `mem` (non-persistent).
  - `SUPRAGNOSIS_DATA_DIR` - store directory (default `~/.supragnosis/db` for cozo,
    `~/.supragnosis/redb` for redb - separate on purpose, so both can exist while you try the new one).
  - `SUPRAGNOSIS_EMBED` - `fastembed` (default when compiled with the feature, local ONNX) | `hashing` (for development) | `none`. If it is absent or fails, degrades to keyword search.
  - `SUPRAGNOSIS_CONFIG` - path to `supragnosis.toml` (default `~/.supragnosis/supragnosis.toml`). No file = a standalone node.
  - `SUPRAGNOSIS_VIZ_SOCK` - viewer unix socket path (daemon default `~/.supragnosis/viz.sock`). The
    viewer serves HTTP over UDS only - no TCP port; the socket file's 0600 mode is the access control.
  - `SUPRAGNOSIS_HTTP_ADDR` - MCP streamable-HTTP bind consulted by `serve`/`start`/`status`
    (loopback only; `start`/`status` default `127.0.0.1:7373`). Running with no arguments stays a
    stdio MCP server regardless.
  - `SUPRAGNOSIS_SESSION` - session label grouping the viewer's activity stream (falls back to
    `CLAUDE_CODE_SESSION_ID`, then `<host>-<timestamp>`).
- Tools (13): `observe`, `search_knowledge` (hybrid recall, `scope` = local | remote | both),
  `get_entity`, `traverse`, `workspace_map` (co-occurrence hyperedges), `define_type` (T-Box glossary),
  `propose` / `review` / `list_proposals` / `get_proposal` (the canon gate, Principle 23),
  `sync_status` / `sync_pull` / `sync_push` (federation).
- Resources: `supragnosis://workspaces`, `supragnosis://workspace/{ws}/graph`,
  `supragnosis://workspace/{ws}/hypergraph`, `supragnosis://workspace/{ws}/types`,
  `supragnosis://observation/{id}`.
- Crates: `core` (domain/ports), `store` (adapters), `engine` (services), `embed` (embedder adapters),
  `sync` (federation), `mcp` (rmcp tools/resources), `viz` (live viewer), `cli` (binary).
  `e2e/` is a separate real-model measurement suite (Ollama/Anthropic scorecards, `#[ignore]`d by
  default) - a scorecard, not a regression guard.

## Desktop app (macOS)
`app/` is a thin Tauri shell over the daemon's unix-socket viewer: it attaches to a running
daemon (launchd / `supragnosis start`) or spawns one as a child (reaped on quit), proxies the
webview onto the viz socket via a `viz://` custom protocol, and bridges the SSE event stream.
The UI itself is served by the daemon - the shell embeds no frontend.

The shell is **tray-resident**: closing the window hides it (macOS: the app leaves the dock too)
while the daemon keeps running in the background; the menu-bar mark reopens the viewer, shows
daemon status (spawned vs externally managed), restarts the daemon, and quits. Quit reaps a
spawned daemon but never an attached external one.
```bash
cargo run -p supragnosis-app    # dev run (finds the server binary via SUPRAGNOSIS_BIN,
                                # ~/.local/bin, the debug build, or PATH)
```
Packaged install: `brew install supragnosis` - the release workflow attaches a signed/notarized
universal `.app.zip` to each release and the cask installs it, depending on the
`supragnosis-server` formula for the daemon (the app bundles no sidecar).

## Usage (CLI)
The single binary is controlled through subcommands. Run it **with no arguments** and it
comes up as a stdio MCP server (the backward-compatible path where the MCP client launches
it as a child process).
```bash
supragnosis                     # stdio MCP server (default, no arguments)
supragnosis serve --http 127.0.0.1:7373 --viz ~/.supragnosis/viz.sock   # foreground (HTTP daemon + viewer)
supragnosis start               # start the background daemon (default MCP :7373 + viewer socket ~/.supragnosis/viz.sock)
supragnosis status              # status (pid + port health)
supragnosis stop                # stop
supragnosis restart             # restart
supragnosis identity            # print this node's federation id / public key
supragnosis sync                # one-shot sync round against the configured servers
supragnosis reproject           # deterministic HLC-ordered re-materialization of the projection
supragnosis migrate             # re-create pre-0.1.x rows under the current content-address formula
supragnosis --help              # all options
```
### Trying the redb store
`redb` is a second file-backed store adapter: a pure-Rust embedded B-tree, no C++ RocksDB bridge (so
no `clang`/`libclang-dev` in the build) and no transitive dependencies. Both adapters are held to one
contract by [`port_conformance.rs`](crates/supragnosis-store/tests/port_conformance.rs), which runs
every case against every adapter.

```bash
supragnosis stop                     # both stores are single-process
supragnosis migrate-store --dry-run  # what would be copied
supragnosis migrate-store            # copy the log, then replay it into ~/.supragnosis/redb
supragnosis start --store redb       # or SUPRAGNOSIS_STORE=redb
```
- Only the **observation log** is copied; the entity/relation graph is a projection of it, so it is
  rebuilt by replay rather than transferred (Principle 1/16).
- The Cozo store is opened read-only and left untouched, so this is reversible: drop the `--store
  redb` flag to go back. **The default is still `cozo`.**
- Re-running is safe. `add_observation` absorbs at the content address, so a repeated or partial run
  converges to the same log (Principle 3).

- `sync` / `reproject` / `migrate` / `migrate-store` need the daemon **stopped** (an embedded store
  admits one process at a time). With a
  running daemon, use the `sync_*` MCP tools instead.
- Option precedence: flags > `SUPRAGNOSIS_*` environment variables > defaults.
- The `start` daemon is self-managed (no launchd needed): pidfile `~/.supragnosis/supragnosis.pid` + logs
  `~/.supragnosis/log`. For OS service registration such as auto-start on login, see [`deploy/README.md`](deploy/README.md).
- The MCP HTTP daemon is **loopback-only** (no auth = local trust surface). The viewer has no TCP
  port at all: it serves HTTP over a unix socket (0600, owner-only), e.g.
  `curl --unix-socket ~/.supragnosis/viz.sock http://viz/api/graph`. The authenticated network read
  tier is federation Phase 3.5. Example MCP client registration:
  - stdio: `claude mcp add supragnosis -- $(command -v supragnosis)`
  - HTTP (daemon): `claude mcp add supragnosis --transport http http://127.0.0.1:7373/mcp`

## Federation (hub-and-spoke)
A node can run as a **sync server (hub)** that aggregates and relays other nodes' observation logs.
Only the log replicates - never the projection - so every node re-materializes the same graph from the
same event set. Design: [`docs/federation.md`](docs/federation.md).

- **Identity**: an ed25519 keypair is generated once at `~/.supragnosis/node.key`; `node_id` derives
  from the public key (self-certifying, immutable). `supragnosis identity` prints it.
- **Protocol**: version-vector delta exchange (`advertise` -> `pull`/`push`), content-address dedup,
  HLC causal ordering, then a deterministic re-materialization pass.
- **Trust**: every attestation is ed25519-signed by its origin, and the receiver recomputes the
  content id before verifying - a forged id or a relay-tampered lineage never lands.
- **Sharing is opt-in** (Principle 17): only workspaces on `[sync] share_workspaces` leave the node,
  and the hub authorizes each peer per workspace.
- **Configuration** (`supragnosis.toml`, unknown keys rejected loudly so a typo cannot silently
  disable a role):
  ```toml
  [sync]
  share_workspaces = ["supragnosis"]
  servers = ["https://hub.example:7420"]
  auth_token = "..."            # bearer presented to the hub
  origin_keys = { }             # node_id -> public key directory (manual until canon-policy lands)

  [server]                      # only when this node runs a hub
  listen = "0.0.0.0:7420"
  tls_cert = "cert.pem"
  tls_key  = "key.pem"
  allowlist = [ ]               # per-node: node_id, public key, bearer hash, shared workspaces
  ```
- **Current limits**: single-principal only (multi-principal governance - the `tbox_change` gate and
  the canon-policy artifact - is Phase 5); a non-loopback bind requires TLS **and** a non-empty
  allowlist; embeddings do not replicate (they are a node-local recall aid, so synced knowledge answers
  keyword search immediately but needs local re-embedding for semantic recall).

## Core ideas
- Knowledge arrives as **immutable observation events** (the source of truth, preserving
  provenance), and the entity/relation graph is **materialized** from the log (event sourcing).
- **Local-first + topology-independent log replication** - whether local-only / central server (hub)
  / direct peer / hybrid, any connection topology converges without conflict under the same merge semantics.
- A hexagonal (port/adapter) structure isolates the store/embedder/extractor so they are swappable.

For details, see the [architecture design doc](docs/architecture.md).
