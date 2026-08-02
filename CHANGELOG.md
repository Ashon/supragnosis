# Changelog

Every release has a note in [`docs/releases/`](docs/releases/), written by hand at the time and
committed with the version bump. This file is the index; the notes are the record.

They are not Keep a Changelog. A list of changed lines answers *what moved*, and these answer *what
was wrong and why the fix is shaped the way it is* - which is the thing worth writing down once the
diff is already public.

| Version | |
|---|---|
| [v0.2.1](docs/releases/v0.2.1.md) | Upgrade if you are on v0.2.0. The guard that was supposed to stop a Cozo-era node from starting |
| [v0.2.0](docs/releases/v0.2.0.md) | Cozo is gone. redb is the store - the only file-backed one - and the C++ RocksDB bridge that |
| [v0.1.21](docs/releases/v0.1.21.md) | A second store. redb - a pure-Rust embedded B-tree with no transitive dependencies - now sits |
| [v0.1.20](docs/releases/v0.1.20.md) | Four repairs from a full closure review - the periodic audit that asks whether the principles, the |
| [v0.1.19](docs/releases/v0.1.19.md) | Two fixes on the MCP surface that share a shape: the daemon held the answer and did not say it. Neither |
| [v0.1.18](docs/releases/v0.1.18.md) | Two halves. The first is cost: every fold on the read path is a pure function of the same rows and |
| [v0.1.17](docs/releases/v0.1.17.md) | M3.5 closes here. The proposal gate had a specified set of blocking checks and enforced none of |
| [v0.1.16](docs/releases/v0.1.16.md) | The release where the guarantees stopped being assertions. architecture.md Section 14 claims |
| [v0.1.15](docs/releases/v0.1.15.md) | A viewer/UX release. It refines how the observation log (v0.1.14) reads, and makes the minimap a |
| [v0.1.14](docs/releases/v0.1.14.md) | The observability release. The log is the source of truth and the graph is a projection of it |
| [v0.1.13](docs/releases/v0.1.13.md) | The resolution-layer release. M3's belief and identity halves land: the "current belief" is no |
| [v0.1.12](docs/releases/v0.1.12.md) | A desktop-app bugfix release. The knowledge model and the MCP tool surface are unchanged; the |
| [v0.1.11](docs/releases/v0.1.11.md) | A packaging release: the Homebrew surface gets its final token layout and a correct upgrade |
| [v0.1.10](docs/releases/v0.1.10.md) | The desktop release: the viewer leaves TCP for a unix socket, a tray-resident macOS app wraps it, |
| [v0.1.9](docs/releases/v0.1.9.md) | A viewer maintainability release: the ontology viewer's frontend moves out of an inline Rust string |
| [v0.1.8](docs/releases/v0.1.8.md) | A security-hardening release: follow-up fixes from a full design + implementation review. |
| [v0.1.7](docs/releases/v0.1.7.md) | Clicking a proposal used to preview the change on the graph only for entity_merge (the fold |
| [v0.1.6](docs/releases/v0.1.6.md) | supragnosis.dev and the live viewer used to look like two different products; now they are one. |
| [v0.1.5](docs/releases/v0.1.5.md) | v0.1.4 shipped the federation library; v0.1.5 wires it into the product. A node can now run as a |
| [v0.1.4](docs/releases/v0.1.4.md) | The groundwork for hub-and-spoke knowledge sharing, specified first |
| [v0.1.3](docs/releases/v0.1.3.md) | The workspace can now surface how its knowledge could be tidied, and every change to the |
| [v0.1.2](docs/releases/v0.1.2.md) | Knowledge is now more than names and types: you can attach a human-readable explanation to |
| [v0.1.1](docs/releases/v0.1.1.md) | The supragnosis binary is now driven by subcommands (clap). Running it with no |
| [v0.1.0](docs/releases/v0.1.0.md) | The first release of an embedded, file-based Rust server that integrates knowledge from multiple |

## Breaking changes

- **v0.2.0** replaced the store: Cozo is gone, redb is the only file-backed one, and a node running
  a Cozo store must migrate through v0.1.21 first. The build refuses to start on an un-migrated
  store rather than coming up empty beside it. See
  [the v0.2.0 note](docs/releases/v0.2.0.md) and [docs/store-migration.md](docs/store-migration.md).
  **If you upgraded to v0.2.0 with a pre-0.2 launchd agent, read the
  [v0.2.1 note](docs/releases/v0.2.1.md) first** - the guard had a hole shaped like exactly that
  configuration.
