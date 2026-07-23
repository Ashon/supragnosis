# supragnosis

An embedded, file-based Rust server that structures knowledge arising across
multiple **hosts** and **workspaces** into an **ontology (a concept/relation graph)**
and lets you query and explore it over **MCP**.

> `supragnosis` = *supra* (above/beyond) + *gnosis* (knowing) - knowledge above knowledge (meta-knowledge).

- Language/runtime: **Rust** (`rmcp` 0.16 official MCP SDK, `tokio`)
- Store: **embedded, file-based** `cozo`/RocksDB - unifies relational + graph + vector (HNSW).
- Status: **M4 Phase 4 - federation is live** (v0.1.7). Semantic + keyword hybrid recall (M2), the
  proposal gate and curation console (M3.5), and hub-and-spoke log replication with ed25519-signed
  events over TLS (M4 Phases 0-4) are implemented. The **resolution layer (M3) has not started** -
  entity identity is still exact canonical-name match, so `M3` items in the roadmap remain open even
  though later milestones shipped. Per-milestone detail and the honest record of what is deferred:
  [`docs/architecture.md`](docs/architecture.md) Sections 12/14.
- Docs: architecture -> [`docs/architecture.md`](docs/architecture.md), design principles ->
  [`docs/principles.md`](docs/principles.md), proposal workflow ->
  [`docs/proposal-workflow.md`](docs/proposal-workflow.md), federation ->
  [`docs/federation.md`](docs/federation.md)

## Install (prebuilt binary)
```bash
# Detect platform -> install the latest release binary to ~/.local/bin (with checksum verification)
curl -fsSL https://raw.githubusercontent.com/Ashon/supragnosis/main/scripts/install.sh | sh
```
- Or download the platform tar.gz directly from [Releases](https://github.com/Ashon/supragnosis/releases), extract it, and put `supragnosis` on your PATH.
- Supported platforms: macOS (arm64/x86_64), Linux (x86_64). For other platforms, build from source below.
- The prebuilt binary is **keyword + hashing search**. For local ONNX **semantic search**, build from source with `--features fastembed`.
- On a `v*` tag push, GitHub Actions (`.github/workflows/release.yml`) builds and publishes the release.

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
  - `SUPRAGNOSIS_STORE` - `cozo` (default, file-persistent) | `mem` (non-persistent).
  - `SUPRAGNOSIS_DATA_DIR` - Cozo data directory (default `~/.supragnosis/db`).
  - `SUPRAGNOSIS_EMBED` - `fastembed` (default when compiled with the feature, local ONNX) | `hashing` (for development) | `none`. If it is absent or fails, degrades to keyword search.
  - `SUPRAGNOSIS_CONFIG` - path to `supragnosis.toml` (default `~/.supragnosis/supragnosis.toml`). No file = a standalone node.
  - `SUPRAGNOSIS_VIZ_PUBLIC=1` - opt in to read-only viewer exposure beyond loopback (writes stay loopback-gated).
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

## Usage (CLI)
The single binary is controlled through subcommands. Run it **with no arguments** and it
comes up as a stdio MCP server (the backward-compatible path where the MCP client launches
it as a child process).
```bash
supragnosis                     # stdio MCP server (default, no arguments)
supragnosis serve --http 127.0.0.1:7373 --viz 127.0.0.1:7374   # foreground (HTTP daemon + viewer)
supragnosis start               # start the background daemon (default MCP :7373 + viewer :7374)
supragnosis status              # status (pid + port health)
supragnosis stop                # stop
supragnosis restart             # restart
supragnosis identity            # print this node's federation id / public key
supragnosis sync                # one-shot sync round against the configured servers
supragnosis reproject           # deterministic HLC-ordered re-materialization of the projection
supragnosis migrate             # re-create pre-0.1.x rows under the current content-address formula
supragnosis --help              # all options
```
- `sync` / `reproject` / `migrate` need the daemon **stopped** (cozo/RocksDB is single-process). With a
  running daemon, use the `sync_*` MCP tools instead.
- Option precedence: flags > `SUPRAGNOSIS_*` environment variables > defaults.
- The `start` daemon is self-managed (no launchd needed): pidfile `~/.supragnosis/supragnosis.pid` + logs
  `~/.supragnosis/log`. For OS service registration such as auto-start on login, see [`deploy/README.md`](deploy/README.md).
- The MCP HTTP daemon is **loopback-only** (no auth = local trust surface). The viewer is loopback-only
  unless `SUPRAGNOSIS_VIZ_PUBLIC=1` opts in to read-only network exposure; writes (verdicts) stay
  loopback-gated and answer 403 to a remote peer. Example MCP client registration:
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
