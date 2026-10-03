# Crash recovery - a node that stops mid-write comes back whole

> How the projection catches up with the log after a process ends between the two, whatever ended
> it. Companion to [architecture.md](architecture.md) (the write path) and
> [daemon-lifecycle.md](daemon-lifecycle.md) (who starts and stops the process).
>
> Status: **built** (Section 7 steps 1-3). Two things building it changed are recorded in Section 8.

## 1. Why this exists

The log is the source of truth and the graph is its projection (Principle 1). `observe` appends the
observation in one store transaction, then projects the entities and relations it touched in one
transaction per row. Nothing ties the two together. A process that ends after the append and before
the last projection write leaves knowledge the log holds and the graph does not show.

Measured, not supposed. In the 2026-10 adversarial review, a daemon killed with SIGKILL eight times
during `observe` calls left 8 of 23 observations partly unprojected: 61 entity rows and 173 relation
rows missing, and `get_entity` answering "not found" for an entity the log asserts. Nothing noticed.
`supragnosis reproject` repaired it, but only when someone knew to run it, with the daemon stopped,
one workspace at a time.

Ending a process that way is ordinary here, not exotic. The desktop shell stops the daemon it
spawned with SIGKILL, the daemon installs no SIGTERM handler (so a launchd bootout ends it at once),
and federation's `apply` appends replicated events and re-materializes in a separate step. Every one
of those is a gap between append and projection.

That is the case Principle 24 reserves for refusal rather than degradation: an answer that is wrong
rather than absent. "Not found" for a fact the log holds is exactly "knowledge silently absent".

## 2. What this is NOT

- **Not atomic writes.** The stronger design runs the append and every projection write in one
  transaction, so the gap never exists. It would also make `observe` faster, since today it pays one
  commit per touched row (a full reproject of the author's store is about 460 commits and 1.9 s, all
  of it I/O). But the projection reads the log through snapshot read contexts, and a single
  transaction means moving those reads inside the write. That reshapes the store port. It remains
  the follow-up, and nothing here gets in its way.
- **Not a shutdown protocol.** Correctness does not depend on how a process ends. Graceful shutdown
  would make recovery rarer, but it cannot make it unnecessary - power loss and panics do not send
  SIGTERM - so it is not what this rests on.
- **Not idempotent observe.** A client whose connection drops after the append, and which retries,
  appends a second attestation of the same content (Principle 3's union keeps both). Telling a retry
  from a second witness needs an idempotency key on the surface. That is a separate design.
- **Not storage repair.** A torn file is redb's to recover on open. This document assumes the store
  opens and is about the derived rows on top of it.

## 3. The mechanism: an owed-projection ledger

**Every append records, in the same store transaction, that its projection is owed.**
`add_observation` - the one way anything enters the log, used by `observe`, `define_type`,
`propose`, `review`, the sync crate's `apply`, `migrate` and `rekey` - writes the observation's id
and workspace into an `owed` table inside the transaction that writes the row. The store does this
itself, so no caller can append without it. That includes the sync crate, which holds only an
`AssertionStore` and never sees the engine.

**An entry is cleared only by the projection that repays it.**

- `observe` and the other engine writers clear their own observation's entry after its last
  projection write.
- A `reproject` of a workspace clears that workspace's entries. It clears only the entries it read
  before it began, because an append that lands while the reproject runs may be missing from the log
  snapshot it projected.

**A writer that opens the store repays what is owed before it serves.** Opening the engine in any
process that writes - the daemon, a stdio server, the one-shot CLI commands - reads the ledger.
Every workspace with an entry is re-projected, and the entries are cleared as above. Only then does
the process bind its sockets or accept a request. An empty ledger, the normal case, costs one table
read.

Recovery is a reproject, and a reproject's result equals the incremental write's
(resolution-identity.md IR3). So recovery produces exactly the graph the interrupted writes would
have produced, and running it twice changes nothing.

## 4. Decisions

- **Detect, then repay - do not prevent.** The ledger makes the gap visible and closes it on the
  next open. Preventing the gap is the atomic-write follow-up (Section 2).
- **Per observation, not a high-water mark.** A single "projected through" counter fails as soon as
  two writers interleave: an `observe` that finishes after a sync `apply` would mark the apply's
  appends as projected before their re-materialization ran. One entry per observation id has no
  ordering to get wrong.
- **In the store transaction, not beside it.** A marker written in its own commit before the append
  costs an extra fsync per write and still leaves a window between the two commits. Writing the
  entry in the append's own transaction costs nothing extra and leaves no window.
- **Repay all of a workspace, not just the owed ids.** Projection is a fold over the whole log for
  each touched entity, and an interrupted write's touched set is not recorded. Re-projecting the
  workspace is what IR3 already guarantees to be correct. At today's scale it costs about 1.9 s per
  workspace, paid only after an interruption.
- **Before serving, not in the background.** A node that serves while recovering answers "not found"
  for facts it holds. That is the wrong-rather-than-absent case Principle 24 says to refuse. A
  launchd job or the desktop shell waiting a few seconds longer for the socket is the cheaper cost.
- **Loud where it is read** (Principle 24). A startup log line scrolls away. The last recovery -
  when, which workspaces, how many observations it repaid - and the ledger's current size are
  reported by the viewer's `/api/health` and by `supragnosis status`. A non-zero ledger on a running
  daemon means a projection failed after its append. The fact is in the log, and the graph will show
  it after the next open.
- **A store the ledger has never seen is owed in full, once.** Appends made before the ledger
  existed carry no record of whether they were projected, and a store that ran earlier builds may
  already hold the gap Section 1 describes. So the first open by a ledger-aware build - the one that
  creates the table - records every log row as owed, and that open re-projects each workspace. After
  that an empty ledger means nothing is owed. An upgrade therefore repairs history without an
  operator step, at the cost of one reproject per workspace on the first start.
- **The ledger is invisible to older builds.** It is a new table. A build without it neither writes
  nor clears entries, so after a downgrade and an upgrade the ledger can hold entries that were
  repaid long ago, or miss appends the older build made. The first costs one unnecessary reproject;
  the second is the gap this document exists for, back for as long as the older build ran. Running
  `supragnosis reproject` per workspace after such a round trip closes it.

## 5. Invariants

| | Invariant |
|---|---|
| **K1** | Every append records its owed projection in the same store transaction as the row. No append path can skip it. |
| **K2** | An owed entry is cleared only after the projection that repays it is written: the appending writer's own projection, or a reproject that read the entry before it began. |
| **K3** | A writer process that opens a store with owed entries re-projects those workspaces before it serves a read or accepts a write. |
| **K4** | Correctness does not depend on how a process ends. SIGKILL, panic, power loss, a failed projection write: each leaves entries the next open repays. |
| **K5** | Recovery is reported where it is read: `/api/health` and `supragnosis status` carry the last recovery and the ledger's current size. |

## 6. Closure map

| Principle | Where this closes it |
|---|---|
| P1 - the graph is the log's projection | K1-K3: the projection cannot lag the log across a restart. |
| P5 - unknown is not absent | K3: no read is served from a graph known to be behind its log. |
| P24 - refuse when proceeding is worse; be loud where it is read | Section 4 (before serving), K5. |
| P16 - convergence | Recovery is a reproject, and IR3 makes it equal to the interrupted writes. |

## 7. Ordering

1. **The ledger** in the store port: written by `add_observation` in its transaction, read and
   cleared through `KnowledgeStore`, which only the engine holds. In-memory and redb adapters.
2. **Clearing** in the engine's writers and in `reproject`.
3. **Recovery on open** for every writer process, with the report on `/api/about` and `status`.

Guarded by:

- `every_append_owes_its_projection_until_cleared` - K1 on every adapter, in the port conformance
  suite.
- `redb_owed_projections_survive_a_reopen` - the ledger outlives the process that wrote it.
- `a_store_from_before_the_ledger_is_owed_in_full_once` - an upgraded store repays its history once.
- `an_append_whose_projection_never_ran_is_projected_at_the_next_open` - K3 and K4.
- `a_completed_write_owes_nothing` and `a_reproject_repays_only_its_own_workspace` - K2.
- `a_pull_that_stamps_rows_re_materializes_them` - the hub repays what stamping owes.
- `health_reports_owed_projections_and_the_last_recovery` - K5.

Checked end to end: a stdio server killed with SIGKILL during a burst of 60 `observe` calls on a
scratch store left one owed observation. The daemon then opened the store, re-projected the
workspace before it bound, and `status` reported "recovered at start".

## 8. What building it changed

- **The surface is `/api/health`, not `/api/about`.** About describes the build - name, version,
  licence - and a settings dialog reads it as such. The ledger and the last recovery are the node's
  runtime state, so they got their own route rather than a field that would make "about" mean two
  things.
- **The projection already lagged the log in one place nothing reported.** Backfill stamps
  unstamped rows before an export, and stamping rewrites rows whose attestations the projection
  carries. The hub's pull handler stamped and exported but never re-materialized; only pushes did.
  With the ledger in place, that showed up as entries left owed after every pull that stamped
  anything - correctly, since the graph was behind. The pull handler now runs the hub's
  re-materialize hook after a stamping backfill, and `sync_push` re-projects and reports it.
- **A clear names an id, so one edge is accepted rather than closed.** If a sync `apply` delivers a
  re-arrival of the very observation an `observe` is still projecting, the observe's clear removes
  the entry for both. The apply's own re-materialization still runs. The debt is lost only if the
  process dies between that clear and that re-materialization, and only for the same content
  arriving from two directions at once. Closing it needs a per-append sequence in the ledger, which
  the atomic-write follow-up makes unnecessary.

