# Crash recovery - a node that stops mid-write comes back whole

> How the projection catches up with the log after a process ends between the two, whatever ended
> it. Companion to [architecture.md](architecture.md) (the write path) and
> [daemon-lifecycle.md](daemon-lifecycle.md) (who starts and stops the process).
>
> Status: **built** (Section 7 steps 1-4). Step 4, on 2026-10-23, made the engine's writers commit
> whole (Section 3a). Two things building it changed are recorded in Section 8.

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

- **Not atomic writes - for appends that project later.** The engine's own writers became atomic
  with Section 3a: `observe`, `define_type`, `propose` and `review` commit their row and its
  projection in one transaction, so no gap exists for them. The ledger remains for the appends that
  are projected after the fact - a sync `apply`, a stamping backfill, `migrate` and `rekey` - whose
  re-materialization is a separate act.
- **Not a shutdown protocol.** Correctness does not depend on how a process ends. Graceful shutdown
  would make recovery rarer, but it cannot make it unnecessary - power loss and panics do not send
  SIGTERM - so it is not what this rests on.
- **Not idempotent observe.** A client whose connection drops after the append, and which retries,
  appends a second attestation of the same content (Principle 3's union keeps both). Telling a retry
  from a second witness needs an idempotency key on the surface. That is a separate design.
- **Not storage repair.** A torn file is redb's to recover on open. This document assumes the store
  opens and is about the derived rows on top of it.

## 3. The mechanism: an owed-projection ledger

**Every append projected after the fact records, in the same store transaction, that its
projection is owed.** `add_observation` - the way the sync crate's `apply`, `migrate` and `rekey`
enter the log - writes the observation's id and workspace into an `owed` table inside the
transaction that writes the row. The store does this itself, so no caller can append through it
without the entry. That includes the sync crate, which holds only an `AssertionStore` and never sees
the engine. The engine's writers - `observe`, `define_type`, `propose`, `review` - enter through
`commit_projected` instead, which writes the projection in the row's transaction (Section 3a).

**An entry is cleared only by the projection that repays it.**

- The engine's writers record no entry and clear none (Section 3a).
- A `reproject` of a workspace clears that workspace's entries. It clears only the entries it read
  before it began, because an append that lands while the reproject runs may be missing from the log
  snapshot it projected.

### 3a. The engine's writers commit whole (2026-10-23)

The atomic-write follow-up, for the writers the engine runs. Each folds the projection its row
calls for from the log as it would read with the row in it - the stored row, absorbed with the new
attestation, standing in for it - and hands the row and the projected entities and relations to the
store's `commit_projected`, which writes them in one transaction. So:

- **Nothing is owed for them.** The row and its projection commit together or not at all; no entry
  is recorded and none is cleared. A failed projection write fails the whole write, and the log is
  as it was.
- **No clear covers a re-arrival.** An entry the ledger already held for the same id - a sync
  `apply` of the same content, landed first - is left for the re-materialization that repays it.
  The accepted exception Section 8 recorded is gone.
- **One commit instead of many.** An `observe` paid one commit for its append and one for each
  touched row; it now pays one. A reproject writes its whole projection the same way
  (2026-10-24): it paid a commit, and a disk flush, for every entity and relation row, and a reader
  could see a projection half rewritten. It now pays one commit, and a reader sees the old
  projection or the new one.
- **A verdict calls for rows only when it moves a tier.** A merge of a `claim_promotion` or a
  `claim_demotion` changes the tier the entity fold weighs its targets' assertions by, and with it
  which spelling and kind those entities' rows hold, so those rows commit with it. Any other verdict
  commits its row alone: proposal state and merge forwarding are read-time folds.

The fold still reads under the engine's write lock, so no engine writer lands between its read and
its commit; a sync `apply` can, and records its own entry, as before.

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
  daemon means an append projected after the fact - a sync, a stamping backfill, a re-key - has not
  been re-materialized, or its re-materialization failed. The fact is in the log, and the graph will
  show it after the next open.
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
| **K1** | Every append a ledger-aware build makes either commits with its projection in one store transaction (the engine's writers, Section 3a) or records its owed projection in the transaction that writes the row. No append path in such a build can do neither; appends an older build made are Section 4's downgrade case. |
| **K2** | An owed entry is cleared only after the projection that repays it is written, by a reproject that read the entry before it began. No writer clears an entry it did not read so: the engine's writers clear none (Section 3a). |
| **K3** | A writer process that opens a store with owed entries re-projects those workspaces before it serves a read or accepts a write. |
| **K4** | Correctness does not depend on how a process ends. SIGKILL, panic, power loss, a failed projection write: each leaves either nothing written - an engine writer's whole commit (Section 3a) - or entries the next open repays. |
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
2. **Clearing** in `reproject`, and in the engine's writers until step 4.
3. **Recovery on open** for every writer process, with the report on `/api/health` and `status`.
4. **Whole commits** for the engine's writers (Section 3a): `commit_projected` on `KnowledgeStore`
   in both adapters, and `observe`, `define_type`, `propose` and `review` folding their projection
   from the log with their row in it.

Guarded by:

- `every_append_owes_its_projection_until_cleared` - K1 on every adapter, in the port conformance
  suite.
- `redb_owed_projections_survive_a_reopen` - the ledger outlives the process that wrote it.
- `a_store_from_before_the_ledger_is_owed_in_full_once` - an upgraded store repays its history once.
- `an_append_whose_projection_never_ran_is_projected_at_the_next_open` - K3 and K4.
- `a_daemon_repays_what_it_owes_before_it_serves` - K3 at the process: the engine every serving
  entry is handed comes back from `build_engine` over a scratch redb store with nothing owed and
  the owed append in its graph.
- `a_completed_write_owes_nothing` and `a_reproject_repays_only_its_own_workspace` - K2.
- `a_projected_commit_lands_whole_and_owes_nothing` - Section 3a on every adapter, in the port
  conformance suite: the row and its projection land together, owing nothing and clearing nothing.
- `an_engine_write_that_fails_leaves_nothing_written` - K4 for the engine's writers: a store refusing
  the commit leaves no row, no projection and no entry.
- `an_engine_write_leaves_a_re_arrivals_entry` - K2: an observe of the content an apply delivered
  keeps the apply's entry, and writes the row a replay writes.
- `an_observe_folds_the_gate_that_names_its_row` - Section 3a: the row about to be committed is
  read by every fold, the gates included, so a merged promotion of it that arrived first applies
  and the observe writes the row a replay writes.
- `a_reproject_that_fails_leaves_its_entry` - K2 under a fault: a store refusing projection writes
  keeps an apply's entry through a reproject that says it failed, and the next open repays it.
- `a_pull_that_stamps_rows_re_materializes_them` - the hub repays what stamping owes.
- `health_reports_owed_projections_and_the_last_recovery` - K5.

Checked end to end: a stdio server killed with SIGKILL during a burst of 60 `observe` calls on a
scratch store left one owed observation. The daemon then opened the store, re-projected the
workspace before it bound, and `status` reported "recovered at start".

Checked end to end again after step 4: six stdio servers, each killed with SIGKILL at a random point
in a burst of 60 `observe` calls on a scratch store, left nothing owed, and each graph held exactly
the entities its log named. In one run the log held a write whose reply never left the process, and
its projection was there with it.

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
- **A clear named an id, so one edge was accepted rather than closed** - until Section 3a. If a sync
  `apply` delivered a re-arrival of the very observation an `observe` was still projecting, the
  observe's clear removed the entry for both, and the debt was lost if the process died before the
  apply's re-materialization. The engine's writers now clear nothing, so the apply's entry stays.

