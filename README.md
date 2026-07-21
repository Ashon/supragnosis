# supragnosis

An embedded, file-based Rust server that structures knowledge arising across
multiple **hosts** and **workspaces** into an **ontology (a concept/relation graph)**
and lets you query and explore it over **MCP**.

> `supragnosis` = *supra* (above/beyond) + *gnosis* (knowing) - knowledge above knowledge (meta-knowledge).

- Language/runtime: **Rust** (`rmcp` 0.16 official MCP SDK, `tokio`)
- Store: **embedded, file-based** `cozo`/RocksDB - unifies relational + graph + vector (HNSW).
- Status: **M2 - semantic search implemented** (fastembed semantic + keyword hybrid search, Cozo native HNSW, Cozo/RocksDB persistence, stdio MCP server + standalone HTTP daemon + CLI control + live viewer). Design docs -> [`docs/architecture.md`](docs/architecture.md), design principles -> [`docs/principles.md`](docs/principles.md), proposal workflow -> [`docs/proposal-workflow.md`](docs/proposal-workflow.md)

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
  - `SUPRAGNOSIS_HOST` - host id for provenance (default `localhost`).
  - `SUPRAGNOSIS_WORKSPACE` - default workspace (default `default`).
  - `SUPRAGNOSIS_STORE` - `cozo` (default, file-persistent) | `mem` (non-persistent).
  - `SUPRAGNOSIS_DATA_DIR` - Cozo data directory (default `~/.supragnosis/db`).
  - `SUPRAGNOSIS_EMBED` - `fastembed` (default when compiled with the feature, local ONNX) | `hashing` (for development) | `none`. If it is absent or fails, degrades to keyword search.
- Tools: `observe` (load knowledge), `search_knowledge` (semantic + keyword hybrid search), `get_entity` (look up entity + relations + provenance), `traverse` (traverse the relation graph), `workspace_map` (overview of co-occurrence context / hyperedges).
- Crates: `core` (domain/ports), `store` (adapters), `engine` (services), `embed` (embedder adapters), `mcp` (rmcp tools), `viz` (live viewer), `cli` (binary).

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
supragnosis --help              # all options
```
- Option precedence: flags > `SUPRAGNOSIS_*` environment variables > defaults.
- The `start` daemon is self-managed (no launchd needed): pidfile `~/.supragnosis/supragnosis.pid` + logs
  `~/.supragnosis/log`. For OS service registration such as auto-start on login, see [`deploy/README.md`](deploy/README.md).
- HTTP/viewer is loopback-only (no auth = local trust surface). Example MCP client registration:
  - stdio: `claude mcp add supragnosis -- $(command -v supragnosis)`
  - HTTP (daemon): `claude mcp add supragnosis --transport http http://127.0.0.1:7373/mcp`

## Core ideas
- Knowledge arrives as **immutable observation events** (the source of truth, preserving
  provenance), and the entity/relation graph is **materialized** from the log (event sourcing).
- **Local-first + topology-independent log replication** - whether local-only / central server (hub)
  / direct peer / hybrid, any connection topology converges without conflict under the same merge semantics.
- A hexagonal (port/adapter) structure isolates the store/embedder/extractor so they are swappable.

For details, see the [architecture design doc](docs/architecture.md).
