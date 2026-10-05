# Compatibility - what one version of supragnosis promises another

> What a store, an encoding and a machine-read output carry from one release to the next, and what
> a release refuses instead of misreading. Companion to [architecture.md](architecture.md) (the
> write path), [crash-recovery.md](crash-recovery.md) (the first store change that needed this) and
> [federation.md](federation.md) (the wire, whose version is decided there, not here).
>
> Status: **built** (Section 9). One part waits for its occasion: the tripwire in Section 3.6
> belongs to the first release that raises `min_reader` past 2.

## 1. Why this exists

Nothing in a store says which release wrote it. The redb file holds one metadata key, the
embedder's identity, and no format or version. The only era signal is structural: a store without
the `owed_projection` table predates v0.4.4. So when two releases meet over one store, each assumes
the other wrote what it would have written. Measured in the 2026-10 adversarial review, and
re-read from the code since:

- **An older binary destroys what it does not know.** Rows are serde JSON, and no stored type denies
  unknown fields. An older build reads a row from a newer one, drops the fields it has no name for,
  and writes the struct it has back. Re-observing the same content, a sync `apply`, `backfill`,
  `migrate` and `rekey` all rewrite rows this way. Principle 3 forbids destructive overwrite, and
  this is one, done silently, by the binary that is supposed to be the conservative choice.
- **An older binary hides what it cannot parse.** A stored enum value it does not know fails the
  row: enumerations drop it with a log line, point reads answer an error, traversal and semantic
  search skip it without a word. Principle 24 reserves exactly this case for refusal. "No
  knowledge" and "knowledge unavailable" are different answers, and a store this build cannot read
  must not come up as the first.
- **An older binary skips duties it never had.** A build from before v0.4.4 never opens the
  ledger, so its appends are not recorded and its crashes are not repaired. Nothing detects that it
  wrote. By inference, an older binary that cannot parse a newer row can also reissue a sync
  sequence number the newer one already used, because the next number comes from scanning the log
  for this node's own stamps.

None of this needs a deliberate downgrade. `task dev:live` runs a development build against the
real store and hands it back to the released daemon afterwards. That is a newer writer followed by
an older one, by design, on the store that matters most. Homebrew can also install an older formula,
and a second machine can restore a backup taken by a newer release.

Around the store, every surface that another program reads is unversioned:

- **Machine-read CLI output.** `status --json` (and `connect --json`, `server --json`) is parsed by
  the desktop app. It has no schema field, and no test runs the producer against the consumer. The
  app's tests feed hand-written fixtures, so the two can drift without a failure.
- **The MCP surface.** A test holds the tool names, but not their arguments. Worse, every MCP
  client is told the server is **`rmcp 3.5.0`**, not supragnosis: `Implementation::from_build_env()`
  expands `env!` inside the rmcp crate, so it names the library that compiled it. Checked with a
  throwaway in-memory server: `serverInfo` is `{"name": "rmcp", "version": "3.5.0"}`.
- **The registry's claim.** Principle 3 requires "every encoding the log has ever used stays
  readable", and the registry discharges it with the refusal to start beside a Cozo store. That
  refusal is real, but it says nothing about the redb era. No store written by an earlier release is
  opened by any test, and no content id or signature is pinned to a known answer. Every format test
  builds its data with the structs of the build under test, so an encoding change passes them all.

## 2. What this is NOT

- **Not the sync wire's version.** Two nodes at different releases exchanging events need a
  protocol version and a rule for fields one side does not know. That is federation's design
  ([federation.md](federation.md), and the federation-correctness review). This document fixes the
  encodings both sides compute (Section 4), which the wire's rule will lean on, and stops there.
- **Not a migration framework.** A format change brings its own upgrade step, as v0.4.4's ledger
  seed did. What this adds is the record of which steps a store has had, and the refusal when a
  binary is behind it.
- **Not a promise to read a Cozo store.** The v0.2.0 refusal stays exactly as it is; v0.1.21 remains
  the way through.
- **Not a downgrade path.** A store a newer release has changed incompatibly is refused by older
  releases, not converted back. Section 3.5 keeps a copy so going back is possible at all.

## 3. The store's format

### 3.1 Two numbers and a name

The `meta` table gains three keys:

| Key | Holds |
|---|---|
| `format` | The highest store format any writer of this store has used. |
| `min_reader` | The lowest format a binary must implement to open this store safely. |
| `format_by` | The release that last raised either number, or first recorded them, by version string. |

Each build carries two constants. `FORMAT` is the format it writes. `MIN_READER` is the lowest
format that can safely share a store this build has written.

The numbers are separate because a format change is not always a hazard for older binaries. A new
table that only holds derived data, and that an older writer can leave stale without harm, raises
`format` and leaves `min_reader` alone. A new stored field raises both, because an older writer
would strip it (Section 1).

### 3.2 The formats so far

| Format | Layout | `min_reader` | Written by |
|---|---|---|---|
| 1 | The redb tables as v0.2.0 created them. | 1 | v0.2.0 - v0.4.3 |
| 2 | Adds the `owed_projection` ledger. A format 1 writer appends without recording what it owes. | 2 | v0.4.4 onward |

Format 2 should have raised `min_reader` when it shipped. It could not, because there was nowhere to
write it. The first release with the stamp writes format 2 and `min_reader` 2. That does not raise
either number, because a v0.4.4 to v0.4.7 binary ignores `meta` keys it does not read, so the stamp
alone is safe for them.

### 3.3 Opening

Opening reads the stamp in a read transaction, before anything writes. Today `RedbStore::open`
creates tables and seeds the ledger in its first transaction, so the check has to come first.

With the store's `format` S and `min_reader` R, and this build's `FORMAT` F:

| Case | Result |
|---|---|
| R > F | **Refuse.** Nothing is written. The message names `format_by` and this build's version, says the store is unchanged, and says to install `format_by` or later. |
| R <= F, S > F | Open as usual. A newer release wrote additive changes this build handles safely. `format` is not lowered. |
| S < F | Upgrade in place. Run each format's upgrade step from S+1 to F, then write `format` = F, `min_reader` = max(R, `MIN_READER`), and `format_by` = this version, in the same transaction as the last step. |
| S = F | Open as usual. |

A refusal comes from `open` itself, so every path that opens a redb store for writing gets it:
`serve`, `start`, `sync`, `migrate`, `rekey-workspace`, `reproject`. The lock probe that lifecycle
commands use (`redb_in_use`) opens read-only, reads nothing, and is unaffected.

Refusing is the Principle 24 case. Proceeding would serve rows this build silently drops, and its
first rewrite would destroy the fields it does not know. Opening read-only instead is not a safe
middle ground: the read paths are where the dropping happens.

### 3.4 A store from before the stamp

A store without `format` gets one from its structure:

- **No tables at all:** a fresh file. Stamp it with F.
- **`owed_projection` present:** format 2.
- **Otherwise:** format 1.

Then the upgrade in 3.3 runs. Format 2's upgrade step is the existing ledger seed, which owes every
log row once. It moves from "the table did not exist" to "the step from 1 to 2". The behaviour is
the same, and it is now recorded.

### 3.5 Raising `min_reader` keeps a copy

An upgrade that raises `min_reader` first copies the store file to `knowledge.redb.format-<S>`
beside it, after a committed transaction and before the first write at the new format. The log
states the path. This copy is what makes going back possible at all. An older release opens the
copy and loses only what was written since the upgrade, which is the ordinary cost of restoring a
backup. Principle 24 names the alternative, an upgrade people fear, as the failure. Copies are not
pruned automatically. A raise is rare, a copy is one file, and deleting someone's only way back is
not a decision to make silently.

### 3.6 Binaries from before the stamp

v0.4.7 and earlier never read the stamp, so they cannot refuse. The guarantee starts at the first
release that writes it. Until a release raises `min_reader` above 2, that gap costs nothing: no
format between them has changed.

The first release that raises `min_reader` above 2 also needs a tripwire. That release re-types one
table that every pre-stamp build opens at startup. A pre-stamp build then fails at open with redb's
table-type error, instead of stripping fields. The error message is redb's, not ours. But it is a
refusal at the door, which is the property that matters.

### 3.7 What raises which number

A change raises `min_reader` when a binary of the previous format, writing to the store, would lose
or contradict something. That covers:

- a field added to a stored row type, or a variant added to a stored enum, because the absorb
  rewrite strips the one and the parse fails on the other;
- a new duty kept on write, like the ledger;
- a changed meaning of an existing key or value;
- a change of redb's own file format. An older redb refuses it anyway, but with "Corrupted". `open`
  maps that error to the Section 3.3 message, minus `format_by`, which it cannot read.

A change raises only `format` when an older writer leaves it stale without harm, such as a table of
derived data that the next newer open rebuilds.

Either kind of change adds its upgrade step, its row in Section 3.2, and a golden store (Section 5).

## 4. Encodings that cross versions

Two encodings outlive the process that computes them:

- **The content id.** blake3 over (workspace, content, assertions). It is an observation's identity,
  on every node, forever.
- **The attestation signing bytes.** The positional, length-prefixed bytes an origin signs and every
  receiver rebuilds.

Neither carries a version or field tags. Principle 14 already forces a decision when a field is
added: both functions destructure exhaustively, so a missing field is a compile error. What it
lacks is the rule for adding the field without breaking every id and signature already made. Today,
even an absent optional field writes a presence byte. An optional field added to the signing bytes
would therefore change the bytes of every attestation, and every existing signature would fail.

**The rule.** A field added to either encoding goes after every existing field and is written only
when present, as a one-byte tag followed by its length-prefixed value, in ascending tag order. An
absent field contributes no bytes. Every id and signature made before the field existed then
recomputes unchanged, and the base encoding never changes again.

**The guard.** Known-answer vectors pin both encodings for fixed inputs. One vector has every
optional field absent, and one has every field present. A change that alters either output fails
the test. The fix is an extension under the rule above, never an edited vector. The 2026-10 review
showed a golden signing fixture working across versions. This makes it permanent.

What a receiver does with an extension it does not know, whether it rejects it, stores it opaquely
or ignores it, is the wire's rule (Section 2).

## 5. Golden stores

Each format has one store written by a real release of that format. It is checked in compressed,
about 6 KB for a 1 MB redb file, beside the script that wrote it
(`crates/supragnosis-cli/tests/fixtures/stores/make.sh`). The script drives the release's own MCP
surface and a sync round between two throwaway nodes on loopback. A test opens a copy with the
build under test and checks:

- every row is enumerated, none dropped;
- every content id recomputes, and every signature verifies, both the node's own and a peer's;
- the engine reads the projection, a type definition, the proposal fold and a search out of it;
- the open recorded the store's era as Section 3 says.

- **The first golden store** is format 2, written by the released v0.4.7 binary into a temporary
  directory, never from a live store.
- **Each later format** gets its own when a release moves past it, written by the last release
  that wrote it.
- **Format 1** has none. Writing one would mean installing v0.4.3 to produce it. The format 2 store
  covers every encoding format 1 had, since the row types have not changed since v0.2.0. Format 1's
  layout differs only by the missing ledger, and the ledger-seed test already covers that.

This is what discharges "every encoding the log has ever used stays readable" for the redb era. The
Cozo refusal keeps discharging it for the era before.

## 6. Machine-read CLI output

`status --json`, `connect --json` and `server --json` each gain a top-level `"schema": 1`.

- **What stays at the same schema:** adding a field, and adding a value to an enumerated string
  field that consumers already treat as open.
- **What raises the schema:** removing or renaming a field, changing a field's type, and changing
  what a value means.
- **The app's side:**
  - It reads `schema` before anything else.
  - A missing `schema` is a CLI from before the contract. It is handled as today: those fields
    already exist, and the app already degrades on what is absent.
  - A `schema` higher than the app knows is a CLI newer than the app. The app shows "this
    supragnosis-server is newer than the app - update the app" on the tray line and the Settings
    page, and stops reading the payload.

Each output has one example document, checked in at `crates/supragnosis-cli/tests/fixtures/json/`.
Two sets of tests read the same file:

- **The CLI's tests** build each output from fixed inputs and require the same keys at every
  level as the example, and the same kind of value wherever both hold one. Each document is built
  by a function of facts already gathered, so no daemon is needed. `status` gathers its probes
  first and then shapes them.
- **The app's tests** list every field the app reads, by JSON pointer, and require each one in the
  example. They also pass the examples through the same functions that read a live answer.

A field the CLI renames or drops fails the CLI's side. A field the app starts reading that the CLI
never sends fails the app's side.

The plain-text lines the app reads (the first line of an outcome, `Error:` stripped) are not a
contract and do not become one. The app shows them as messages and never branches on their wording.

## 7. The MCP surface

**The server says who it is.** `get_info` names supragnosis and its version, the same string as
`supragnosis --version`, in place of `from_build_env()`. A client log, a registry and a bug report
then name the release that answered.

**The tools are pinned.** A golden file holds the `tools/list` result, with names, descriptions and
input schemas. A test compares it to the live list. Rewriting the file takes an explicit environment
variable, so a change reaches review as a diff of the file.

| Change | Kind |
|---|---|
| A tool added; an optional argument added | Additive. Golden file updated, nothing else. |
| A description changed | Additive. It is the agent's documentation, reviewed as a diff. |
| A tool removed or renamed; an argument removed or renamed; an optional argument made required; an argument's type narrowed | **Breaking.** Listed under CHANGELOG.md's Breaking changes and at the top of the release note. |

The cache hints `list_tools` adds (`ttlMs`, `cacheScope`) are part of the golden file too. Their
absence once made a strict client reject the whole list (v0.3.1).

## 8. The app and the daemon

The app and the formula ship together, but nothing makes them run together. The cask depends on
the formula with no version, and a `brew upgrade` can move one without the other. Two couplings
cross that gap:

- **The CLI's JSON** is covered by Section 6.
- **The viewer page the shell decorates.** `shell-init.js` comes from the app and runs inside the
  page the daemon serves. It finds `#settingsBtn`, `header` and `h1` there, and stands in for
  `EventSource("/api/events")` through `onmessage` alone. A viz test pins those as the page's
  contract with the shell. A page without a header (the shell's own splash) is not decorated at
  all. A viewer without the gear keeps its own settings, and `shell-init.js` logs that it found
  none.

The app does not compare its own version with the daemon's, and this does not add that check. Drift
is reported between the CLI and the daemon, which is the pair that has to agree for a restart to
fix it.

## 9. Ordering

1. **Store.**
   - The stamp, refusal and upgrade bookkeeping in `RedbStore::open`, with the ledger seed moved
     into the step from format 1 to 2.
   - The copy on a `min_reader` raise, and redb's "Corrupted" mapped to the refusal.
   - The first golden store, from v0.4.7.
   - Known-answer vectors for the content id and the signing bytes.
   - Registry entries for each.
2. **MCP.** `serverInfo` names supragnosis, and the golden tool list.
3. **CLI output.** `schema` on the three outputs, the shared example documents, the app's check, and
   the viz test for the shell's page contract.
4. **Policy text.** Section 10's promise goes in the README in place of its current one line, with
   CHANGELOG.md's Breaking changes section named as where a broken promise is announced.

Steps 1 and 2 are independent of each other. Step 3 touches the app and the CLI together. Step 4
landed with the last of them, because a promise belongs in the README only once it is kept.

## 10. The promise

What a release of supragnosis promises:

- **A store is never misread.**
  - A release opens every redb store an earlier release wrote, and upgrades it in place.
  - A release refuses a store a later release has changed in a way it cannot handle safely. It says
    which release wrote it and what to install, and it changes nothing.
  - Raising `min_reader` happens only in a minor release (0.x.0). Its release note opens with the
    fact, and the store is copied aside first.
- **An identity or a signature, once made, verifies forever.** The encodings only grow, by the rule
  in Section 4.
- **A machine-read output says its schema.** A field is never removed or renamed without raising the
  schema.
- **A breaking change to the MCP tools is named** in CHANGELOG.md's Breaking changes and at the top
  of its release note.
- **What two nodes promise each other on the wire** is decided in federation's design.

## 11. Closure map

| Gap (Section 1) | Closed by |
|---|---|
| Older binary strips unknown fields on rewrite | 3.3 refusal, 3.6 tripwire, 3.7 rule |
| Older binary hides rows it cannot parse | 3.3 refusal |
| Pre-v0.4.4 binary skips the ledger | 3.2 (format 2, `min_reader` 2) and 3.6 |
| No era recorded in the store | 3.1 |
| "Every encoding stays readable" unguarded for redb | 5 |
| Content id and signing bytes unpinned | 4 |
| `status --json` unversioned and untested against the app | 6 |
| `serverInfo` names rmcp | 7 |
| Tool arguments unpinned | 7 |
| Shell depends on unpinned page elements | 8 |
| No stated compatibility policy | 10 |
