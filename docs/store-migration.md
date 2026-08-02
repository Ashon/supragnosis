# Migrating the store: Cozo to redb

> How to move a node's knowledge from the Cozo (RocksDB) store to the redb store, what the move
> preserves, and the one thing it deliberately does not carry across.
>
> **Run this with v0.1.21**, the last release that reads a Cozo store. From v0.2.0 the store is redb
> only, and that build refuses to start when it finds an un-migrated Cozo store rather than coming up
> empty beside one - so the migration cannot be skipped by accident, only postponed.
>
> Every step is reversible: the Cozo store is opened read-only and never written.

## 1. Why the store changed

Cozo's last release was 2023-12-11. The dependency that actually costs, though, is the one it
carried: `cozorocks`, a C++ RocksDB bridge, which was the only reason building supragnosis needed
`clang` and `libclang-dev`. From v0.2.0 it needs neither.

What the Datalog buys was measured rather than assumed. The adapter runs nineteen query shapes, of
which exactly one is genuinely recursive - `traverse`'s bounded BFS. The rest are point get/put,
scans with a workspace filter, a two-rule union for `relations_of`, and an ANN lookup. No time-travel
operator appears anywhere. And the `query` passthrough has never been opened (Principles 12/21), so
Datalog is an implementation detail of the store adapter, not a surface anything else depends on.

`redb` is a pure-Rust embedded B-tree with no transitive dependencies. Both adapters implement the
same `KnowledgeStore` port and are held to it by one suite
([`port_conformance.rs`](../crates/supragnosis-store/tests/port_conformance.rs)) that runs every case
against every adapter - a backend does not get to bring its own reading of the contract.

Measured differences are in [architecture.md](architecture.md) Section 6. The short version: the read
path is faster, markedly so when rows carry embeddings, and semantic recall is slower above roughly
5-6k embedded rows because redb scans where Cozo has an ANN index.

## 2. What moves, and what is rebuilt

**Only the observation log is copied.** The entity/relation graph is a projection of that log
(Principle 1), so it is rebuilt by replay at the end rather than transferred. Copying a projection
would carry over whatever state it happened to be in; a replay is defined by the rows alone and is
what any node would compute from the same log (Principle 16).

Preserved exactly: content, assertions, every provenance attestation, `derived_from` lineage,
observation ids (including legacy-formula ids, which are carried verbatim rather than recomputed),
proposal events and their verdicts, type definitions.

Rebuilt from the log: entities, relations, aliases, the type glossary, proposal state, belief
resolution.

Recomputed rather than carried: **entity name embeddings**, if an embedder is attached during the
replay. They are a node-local recall aid, exempt from the convergence norm (Principle 16, 4th
revision), so regenerating them changes no answer the graph is required to agree on. Observation
embeddings are copied as-is.

## 3. Before you start

**Both stores are single-process.** A running daemon holds the lock. Stop it first:

```bash
supragnosis stop                        # or: brew services stop supragnosis-server
```

**Restart a stale daemon before you compare anything.** `brew upgrade` swaps the binary on disk but
does not restart a running daemon, and `supragnosis --version` reports the binary, not the process.
A daemon that has been up across an upgrade is answering with older code, which makes a comparison
against a freshly migrated store read as a data difference when it is a code difference. This is not
hypothetical: it is how a 40-row gap appeared during the first real migration of the author's own
store, in the all-workspaces view alone, because the running process predated the live-set-door fix
of v0.1.20.

## 4. The procedure

```bash
supragnosis stop
supragnosis migrate-store --dry-run     # what would be copied, and from where
supragnosis migrate-store               # copy the log, then replay it
supragnosis start --store redb          # or: SUPRAGNOSIS_STORE=redb
```

The source is the Cozo store at `SUPRAGNOSIS_DATA_DIR` (default `~/.supragnosis/db`). The target
defaults to `~/.supragnosis/redb` and is set with `--to`. The two live in **separate directories on
purpose**: RocksDB owns its directory, and keeping them apart is what lets both exist while you are
still deciding.

**The source store is opened and never written.** That is what makes this reversible: drop the
`--store redb` flag and the Cozo store is exactly as it was.

**Re-running is safe and is how you catch up.** `add_observation` absorbs at the content address, so
a repeated run unions each row with itself and converges (Principle 3). A migration taken while the
daemon was still serving can therefore be topped up later by stopping the daemon and running it
again - the second pass copies only what the first one could not have seen.

## 5. What a replay does not reproduce

**A fresh store contains exactly what the log asserts. Nothing else.**

That is the correct behavior, and it is worth stating plainly because it can look like data loss. On
the existing store the same replay is invisible: `reproject` upserts and never deletes, so any row
that was written into the projection outside the log survives every re-materialization. A migration
starts from an empty store, so those rows simply do not appear.

In the author's own store the gap was **72 entity rows out of 440**:

| origin | count |
|---|---|
| never asserted anywhere in the log | 35 |
| name asserted, but under a different workspace | 37 |

They were early-era rows, written before the current ingest path existed - some through the raw
`put_entity` that `Engine::store()` still exposes, some test sentinels. By Principle 1 they are not
knowledge: no assertion supports them, so nothing can reproduce them and the log cannot explain them.

**Check yours before you switch**, so the decision is informed rather than discovered:

```bash
# entity ids the log actually asserts, vs rows the projection holds
supragnosis migrate-store --dry-run     # observation count and workspaces
```

then compare `/api/graph` node counts between the old store and the migrated one, per workspace (see
below). A difference is this class of row. If any of it is knowledge you want, **re-assert it through
`observe` before switching** - that puts it in the log, where a replay can find it, which is where it
should have been.

## 6. Verifying the move

Serve the new store beside the old one and compare through the same read surface:

```bash
# the migrated store, on its own port and socket
SUPRAGNOSIS_STORE=redb supragnosis serve --http 127.0.0.1:7396 --viz /tmp/redb.sock

curl -s --unix-socket /tmp/redb.sock 'http://localhost/api/observations?workspace=*&limit=100000'
curl -s --unix-socket /tmp/redb.sock 'http://localhost/api/graph?workspace=*'
```

What to expect:

- **The log matches exactly**, per workspace and in total. This is the check that matters: the log is
  the source of truth, and a difference here is a real defect, not a projection artifact.
- **The graph may be smaller**, by the rows of Section 5.
- **The union of the scoped views equals the unscoped view** on both stores. If it does not, the
  reader is stale (Section 3), not the data.

## 7. Rolling back

There is nothing to undo - the Cozo store was never written. On v0.1.21, going back is dropping the
flag:

```bash
supragnosis stop
supragnosis start                       # v0.1.21 defaults to cozo
```

Delete `~/.supragnosis/redb` if you want the disk back; going forward again is another
`migrate-store`. Note that rolling back means **staying on v0.1.21**: v0.2.0 has no Cozo adapter, so
it is the migration that unblocks upgrading, not the other way round. Anything observed while running
on redb lives only in the redb store, so decide before the two logs diverge.
