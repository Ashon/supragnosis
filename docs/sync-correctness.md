# Sync correctness - what a round may not lose, count twice, or let through

> Ten defects in federation's shipped sync, found by the 2026-10 adversarial review and re-traced
> against v0.4.7, and the rules that close them. Companion to [federation.md](federation.md) (the
> protocol this revises, Section 12) and [compatibility.md](compatibility.md) (which hands the
> wire's version to this document).
>
> Status: **built** (Section 13, all five steps). Each defect has a test that failed on the code
> before it.

## 1. Why this exists

federation.md promises convergence: two nodes holding the same events compute the same graph, and
a round is idempotent and resumable. The promise holds for the cases its tests exercise: one round
at a time, every event accepted, a store that is never restored, a hub reached over verified TLS.
Outside them, the code does things the design says it does not. Each item below was traced in the
code, not inferred from the design.

**Order and delivery**

- **D1. A stamp carries the export time, not the authoring time.** Backfill stamps a local
  attestation with the node clock when the round runs. federation.md Section 10 says this "changes
  when the stamp is applied, not its meaning". It changes the meaning. Node A observes kind X at
  t=100 and does not sync. Node B observes kind Y at t=500 and syncs. A pulls Y, and Y wins on A. A
  exports at t=1000, so X is stamped 1000 and the older edit now wins everywhere, A included.
  `rekey` and `migrate` strip stamps, so the next export flattens their whole history the same way.
- **D2. Two rounds at once stamp one act twice.** Nothing serializes backfill. Two MCP sessions
  calling `sync_push`, or a hub answering two pulls, snapshot the same unstamped row and each stamps
  it with its own seq, HLC and signature. Absorb keeps both, because only an *unstamped* base is
  superseded by a stamp. principles.md (P3, the enrichment relation) says this double count is ruled
  out. Entity source counts then over-count, and the two copies can reach peers unevenly.
- **D3. A seq can be issued twice.** The node key lives in `~/.supragnosis`; the next seq comes from
  scanning the store. Restore the store from an older backup, keep the key, and the node issues seqs
  it already used, for different events. Every peer's version vector already covers those seqs, so
  the new events are never offered. They are lost without a sign. Backfill runs before pull, so the
  node does not even learn its own higher seqs first.
- **D4. One rejected event leaves a permanent hole.** The version vector is a max per (origin,
  workspace). If event k is rejected and k+1 accepted, the receiver's max is k+1 and k is never
  offered again, even after the cause is gone. A rejection is reported once, in that round's
  response. The causes are ordinary: a content-id or validation difference between releases, a key
  not yet configured.

**Transport and admission**

- **D5. One signature, many spellings.** `hex_decode` accepts uppercase and a leading `+`, so one
  signature has several spellings that all verify. Absorb compares signatures as strings, so a
  respelled copy is a second attestation, exported onward and counted twice. Any admitted peer can
  do it; the bytes need no forging.
- **D6. A peer writes for someone else.** At the hub, push checks that the *pushing peer* is granted
  the workspace, and that each event is signed by *some* admitted origin. It does not check that the
  event's origin is granted that workspace. A peer granted A can bring in genuinely signed events
  from an origin admitted only to B. Under remote-server.md's rule R5, one such event also makes A
  unservable to the hub's principals, and the origin cannot repair it.
- **D7. Remote search sends queries the node does not share.** `search_knowledge` with a remote
  scope sends the workspace and the query text to every hub, whether or not the workspace is in
  `share_workspaces`. `sync_push` checks the share list. Search, which ships the user's words,
  does not.
- **D8. `insecure_tls` turns off verification for every hub, then sends the bearer.** It installs a
  verifier that accepts any certificate, on every link, with no loopback limit and no warning. A
  plain `http://` URL to another host is accepted too. The deployment guides recommend the flag for
  a self-signed hub, and there is no way to name the hub's certificate instead.
- **D9. No size or time bounds, and no batching.** A push sends the whole surplus in one body. Past
  axum's implicit 2 MB limit it gets 413, the round aborts before its pull, and the next round sends
  the same, larger body. A pull answers with every missing event in one body, and the client buffers
  it with no timeout. The hub's principal surface has a 1 MB cap, a 60 s timeout and constant-time
  credential compares. The sync surface beside it has none of them.

**The wire's version**

- **D10. Nothing on the wire says which release is talking.** A newer peer's unknown enum value
  fails the whole request body (422), every round, and stops every other event in it. The hub cannot
  learn a peer's release, so an old spoke whose missing consent makes a workspace unservable is
  indistinguishable from one that withheld it. The comment and docs justifying the consent header
  cite "strict request parsing" that the wire types do not do: unknown fields are ignored, unknown
  enum values are fatal.

## 2. What this is NOT

- **Not a new replication model.** The version vector stays a per-stream max, and convergence still
  rests on content addressing, HLC order and the deterministic fold. Dotted version vectors would
  close D4 more generally, at the price of a wire change every peer must make. Section 6 closes it
  with a rule on the receiving side.
- **Not the clock-drift bound.** A far-future stamp from a compromised origin is still deferred
  (federation.md Section 4). Nothing here makes it worse.
- **Not rate limiting.** Per-peer rates remain owed (remote-server.md). This adds bounds on size and
  time, which a single honest round can hit.
- **Not multi-hop relay.** Topology is hub and spoke. The admission rule in Section 8 keeps that
  boundary rather than widening it.

## 3. A stamp carries the authoring time (D1)

**Rule.** Backfill stamps an attestation with the HLC of when it was observed:

- `wall` is the attestation's `observed_at`;
- `counter` separates attestations of this node with the same `wall`, counting up in the pass's
  deterministic order;
- `node` is this node.

The node's clock then merges the stamp (the HLC receive rule), so a later local event still orders
after it.

This is the order the node already showed before export: an unstamped row orders by
`Hlc::legacy(observed_at)`. Stamping no longer reorders anything on the node that stamps. Across
nodes, two edits now order by when they were made. They order by HLC only as far as the wall clocks
agree, which is the same assumption every HLC stamp makes. Rows that `rekey` or `migrate` stripped
keep their `observed_at`, so their restamped order is their original order.

Stamps already made keep their HLC. They are signed, and the log is append-only.

## 4. One act, one stamp (D2)

**Rule.** Backfill holds a lock in the `SyncNode`, from its snapshot of a workspace to its last
write. It is the same node object every caller in a process shares. Under the lock, seqs are
allocated and written in ascending order, so the order in which they commit is the order of the
seqs. A second caller waits, then finds the rows already stamped. Cross-process races are already
impossible, because a redb store has one writer.

Rows that a past race already stamped twice keep both stamps. The log is append-only, and the two
stamps are both validly signed. They over-count one act by one, and nothing else.

## 5. A seq is issued once (D3)

**Rule.** The counter lives with the identity.

- **A high-water mark beside the key.** `node.seq` records, per workspace, the highest seq this
  identity has issued. It sits next to `node.key`.
- **Reserved before use.** Before a backfill stamps n attestations, the mark is raised by n and
  written durably. A crash then leaves a hole, which is harmless (Section 6). It never leaves a
  reuse.
- **The next seq** is one past the greater of the mark and the store's own maximum. A restored store
  continues where the identity left off. A store from before the mark continues from its scan, as
  today.
- **The host is asked first.** Every round that stamps advertises first, and the host's copy of
  this node's own stream floors the counter before anything is stamped. An own event pulled back
  floors it the same way when it is applied.

The case this cannot catch is one identity on two machines at once, by copying `~/.supragnosis`
whole. The marks are copied with the key. That is one node id with two histories, not a seq problem.
The CLI says so where it creates the key: the file is the node, so move it with the store and never
copy it to a second running node.

## 6. A stream is held, not holed (D4, D10)

**Rule.** Apply takes a batch's events in (origin, workspace, seq) order. At the first rejection in
a stream, it stops applying that stream for the rest of the batch. Every later event of the stream
is reported as held behind the rejected seq, and the other streams carry on.

The receiver's max for the stream then stays below the rejected seq, so the rejected event and
everything held behind it are offered again every round. The failure becomes loud and persistent,
not silent and permanent. It clears by itself once the cause is gone: an upgrade, a configured key.

**Events are decoded one at a time.** The batch is read as a list of JSON values, and each is
decoded into an event separately. An event this release cannot decode, such as an enum value from a
newer one, is rejected as `Undecodable`, naming what failed. The rest of the body proceeds. Today
the whole request fails, and with it every other stream in the batch.

Holes that no event fills, seqs whose stamps `rekey` or `migrate` removed, stay harmless. Nothing
waits for a seq that is not offered. The rule above only stops the receiver advancing past one that
is.

## 7. Canonical encodings on the wire (D5)

**Rule.**

- **`hex_decode` accepts only lowercase hex digits, in pairs.** Every spelling but the one
  `hex_encode` writes is malformed. A signature or key in any other spelling fails verification, and
  the event is rejected (`BadSignature`).
- **Keys in configuration are checked when the file is read.** An uppercase or malformed
  `public_key_hex` in the allowlist, or in `[sync.origin_keys]`, becomes a configuration note naming
  the entry. The entry is not used. That direction only shares less (P24).

Every signature and key this software has written is lowercase, so nothing it made stops verifying.

## 8. Who may write into a workspace (D6)

**Rule.** At a hub, an event from origin O may enter workspace W through a push only when one of
these holds:

- O is the pushing peer;
- O is an allowlisted node whose grant includes W;
- O is the hub itself.

Any other event is rejected as `OriginNotAdmitted`, and under Section 6 its stream is held. This is
the hub's own admission rule applied per event: what a node could not push in its own name, another
node cannot push for it. Pulls are unaffected. A spoke verifies what it pulls against its
`[sync.origin_keys]`, as today.

## 9. What leaves the node (D7)

**Rule.** Remote search sends a query for a workspace only if that workspace is in
`share_workspaces`. For any other workspace the remote half is skipped, and the response says so
beside its results, the way a narrowed sync round names the hosts it skipped. The local half answers
as usual.

## 10. Transport (D8)

**Rules.**

- **A hub's certificate can be named.** `[[sync.server]]` gains `ca`, a PEM file of the CA or
  self-signed certificate to trust for that hub, the same option a remote MCP profile already has.
  This is what the deployment guides and the sandbox use in place of `insecure_tls`.
- **`insecure_tls` applies to loopback hubs only.** For any other host it is ignored, and a
  configuration note says to name the certificate with `ca`. Ignoring it makes a link fail rather
  than send the bearer unverified, so it errs toward sharing less (P24).
- **No bearer over plain HTTP off loopback.** A `http://` URL to a non-loopback host disables that
  link, with a configuration note. This is the rule `normalize_url` already applies to remote
  profiles.
- **The hub compares bearers in constant time**, as its principal surface does.

## 11. Size and time (D9)

**Rules.**

- **An observation's content is capped at 1 MiB at ingest.** A bigger one is refused with the limit
  named. Without a cap, one event could exceed any body limit on its own, and a stream would be held
  forever by an event that can never be delivered.
- **The sync routes declare their body limit, 8 MiB,** instead of inheriting axum's default unnamed.
- **Push is batched.** At most 1 MiB of encoded events, or 500 events, in (origin, workspace, seq)
  order. A round pushes batch after batch until the surplus is gone or a batch fails.
- **Pull is paged.** `PullReq` gains `limit`, an event count, and `PullResp` gains `more`. A newer
  client loops while `more` is true. An older hub ignores `limit` and never sends `more`, so a newer
  client reads one page, which is everything, as today. A newer hub serves an older client one page
  of the default size, and the client's next round continues from its version vector. Pages are in
  (origin, workspace, seq) order, so the version vector advances exactly as far as the page went.
- **Every sync request has a timeout:** 60 s on the client, and the same on the hub's sync routes,
  as on its principal routes.

## 12. The wire's version (D10)

**Rule.** `PROTOCOL` is the sync protocol's version, an integer. This document's changes make it
1; everything before has no version and reads as 0.

- **Every request carries it** in a `supragnosis-sync` header, with the binary's release beside it
  in `supragnosis-release`. Every response carries the hub's in the same headers, and `PingResp`
  gains `protocol`.
- **The hub records each peer's** protocol and release in its peer table, shown with the known
  peers in `sync_status` and the viewer. Where R5 refuses a workspace, each node that has not
  consented is named with the release it last said it runs - or "release not known", with the note
  that a release before v0.4.5 cannot send consent at all - not merely "has not consented".
- **The spoke shows each hub's** protocol beside its release, in the viewer's federation panel and
  its `/api/federation` answer, without comparing anything.

**What raises `PROTOCOL`** is a change an older peer would mishandle: a new enum value on the wire,
a new signed field (compatibility.md Section 4), or a changed meaning. Adding an optional field, an
endpoint or a header does not, because older peers ignore them. A peer seeing a higher protocol from
the other side reports it on the operator's surface (P24) and keeps exchanging what both understand:
Section 6 holds the events it cannot decode instead of failing their batch.

The consent header's documented rationale is corrected. An older hub ignores an unknown body field
exactly as it ignores an unknown header. The header is still the right place for consent, because it
applies to the whole request, not to one event.

## 13. Ordering

Each step is independent and lands with its tests. Order is by what each loses if left.

1. **Delivery** (Sections 3-6): the authoring-time stamp, the backfill lock, the seq mark with
   pull-before-backfill, held streams with per-event decoding. Each has a test that fails today:
   - two concurrent backfills stamp a row once;
   - a restored store does not reuse a seq;
   - a rejected event is offered again and fills its gap once accepted;
   - an export after a peer's newer edit does not win.
2. **Admission** (Sections 7-9): canonical hex, the origin rule, the outbound share check.
3. **Transport** (Section 10): `ca`, `insecure_tls` and plain HTTP limited to loopback,
   constant-time compare. The sandbox and the deployment guides move to `ca` in the same change.
4. **Bounds** (Section 11): the ingest cap, the declared limit, push batches, pull pages, timeouts.
5. **The wire's version** (Section 12): the headers, `PingResp.protocol`, both sides' displays.

## 14. What this revises

- **federation.md Section 10, Phase 2.** "It changes when the stamp is applied, not its meaning"
  becomes true by Section 3. The HLC is the authoring time whenever the stamp is applied.
- **federation.md F7.** It stays hole-tolerant for holes no event fills. It gains Section 6: a
  rejected event holds its stream until it is accepted.
- **federation.md F6** gains `OriginNotAdmitted` (Section 8) and `Undecodable` (Section 6).
- **principles.md P3** already claims the enrichment relation rules out double-counting after
  backfill. Section 4 is what makes that true.
- **remote-server.md** (consent in a header): the "strict request parsing" rationale is replaced by
  Section 12's.
- **compatibility.md Section 2** points here for the wire's version.

## 15. Closure map

| Defect | Closed by |
|---|---|
| D1 export-time HLC | 3 |
| D2 concurrent backfill double-stamps | 4 |
| D3 seq reuse after a restore | 5 |
| D4 permanent hole after a rejection | 6 |
| D5 malleable hex | 7 |
| D6 a peer writes for an origin not granted the workspace | 8 |
| D7 remote search ignores the share list | 9 |
| D8 `insecure_tls` and plain HTTP send the bearer unverified | 10 |
| D9 unbounded bodies, one-shot push, no timeouts | 11 |
| D10 no protocol version; one unknown value fails a whole body | 6, 12 |
