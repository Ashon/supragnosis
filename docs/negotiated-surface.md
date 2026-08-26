# Negotiated surface - a peer's entitlement as a routing input (federation.md 6e, F21)

> What federation.md 6e requires, how it is built, and in what order. 6e says what must hold; this
> document says which code moves and why the steps are in this sequence rather than another.
>
> The work is M4 Phase 7. Nothing here is built: `ping` already carries the answer and no caller
> consumes it, which is why F21 sits in the coverage registry as unmet with nothing pinning it.

## 1. The answer is already on the wire

`/sync/ping` responds with the hub's node id, its version, and **the caller's own** shared workspaces
(Section 5, `PingResp`). The handler comment states the intent plainly: a spoke can verify
connectivity, auth AND authorization in one round trip.

One caller consumes it. The daemon's health loop reads the version, discards the workspace list, and
then iterates the spoke's **local** share list to build the per-workspace drift view. So the view is
assembled from what this node believes it shares rather than from what the host says it admits, and
where the two disagree the workspace leaves the view without a word. The MCP `sync_status` tool does
not ping at all.

That is the whole defect. No mechanism is missing - the data is produced by the Phase 3 transport and
the Phase 4 allowlist, and the change is that it is kept instead of dropped.

## 2. Where the map lives

The crate graph decides this, not preference. `mcp` depends on `engine` and `sync`; `viz` depends on
`engine` and knows nothing of `sync`. The federation status blob is a viz type - an opaque
`serde_json::Value` behind a lock, deliberately so, because that crate renders JSON and does not
model federation.

A tool handler therefore cannot read the map through the viz type without `mcp` taking a dependency
on `viz`, which is one adapter reaching into another and points the wrong way for P20.

So the typed map belongs in **`supragnosis-sync`**: the negotiated surface is what that crate's
transport produces, and both `mcp` and the CLI wiring already depend on it. The blob keeps carrying a
serialized copy for the viewer, so `viz` changes not at all and the crate graph gains no edge.

## 3. The three-bucket difference

The operator value is in the disagreement, not the agreement. Intersecting the two lists silently
hides the misconfiguration that produced the gap, so the difference is reported in three buckets:

| Bucket | Meaning |
|---|---|
| **both** | this node shares it and the host admits it |
| **local-only** | this node lists it, the host does not admit it - a setup error that today vanishes from the drift view |
| **peer-only** | the host would admit it, this node does not share it - knowledge left on the table |

Each carries the time it was negotiated, because a grant can be narrowed at any moment (6a) and a map
without its timestamp invites treating a stale entry as current.

## 4. Routing, and the failure it must not introduce

Today a wrong allowlist entry fails loudly: `authorize_workspace` answers `403` and names the
workspace and the node. Narrow the fan-out on the negotiated map and the same misconfiguration
becomes a host quietly skipped - a smaller result set with nothing to say it is smaller, which is the
reading P5 exists to prevent and the shape of failure this project has already paid for once (the
release notes for v0.3.1 record a task promising "real data" serving zero rows against a store
holding 168).

**Labelling is therefore a precondition of filtering, not a companion to it.** A response the map
narrowed states which hosts it consulted and which it skipped as not-admitted, and that has to ship
before anything starts skipping.

## 5. Per-server credentials, the prerequisite

`[sync] auth_token` is one token presented to every configured server. One host therefore learns the
bearer that also authorizes at another and could present it there as this spoke. That is tolerable
while the token only fetches knowledge; it is not tolerable once a host's answer decides where
knowledge goes, because the answer is worth exactly as much as the identification of the caller.

The config gains per-server entries mirroring the per-peer shape `[[server.allowlist]]` already has:

```toml
[[sync.server]]
url = "https://hub-cloud.internal:7420"
auth_token = "..."
share_workspaces = ["cloud"]
```

Two rules come out of P5 rather than convenience. Both shapes present is a **startup failure**, not a
precedence rule - a precedence rule is a silent degrade wearing a specification. And the same
validation is where F14's unmet leg is repaid: a node whose own id sits in its own allowlist starts
without complaint today, and the check belongs beside the one that reads those entries.

## 6. Ordering

Three steps. Each is useful alone, each lands on a green tree, and only the third changes behaviour.
The sequence is not the order the work was first imagined in - filtering was the interesting part and
came first, until the review put two constraints on it.

**Step 1 - per-server credentials, and F14's refusal.** Config shape, validation, the coexistence
failure, the self-collision refusal. No behaviour depends on it yet. It is first because Step 3 rests
on an identification that a shared credential does not support (Section 5), so doing it later means
shipping a routing decision on a premise known to be weak.

**Step 2 - keep the answer, report the difference.** The health loop stops discarding
`shared_workspaces` and writes the typed map; the drift view becomes the three buckets with their
negotiation time; `sync_status` and the viewer's federation blob expose it. Read-only throughout.

> **The constraint this step exists under.** F11 - sync never blocks a tool handler - is already
> unmet: the `sync_*` tools ship as ordinary blocking calls. Negotiating inside a handler would add a
> round trip per configured server to a call that already blocks, deepening an invariant this build
> does not meet. The health loop already pings on a slow interval and already holds shared state, so
> **negotiation stays there and handlers read the cached map**. This is not a new norm; it is F11
> applied to work that had not existed yet.

**Step 3 - route, and say what was skipped.** The four fan-out sites - `sync_push`, `sync_pull`,
federated search, and the CLI's one-shot round - consult the map, and every narrowed response names
the hosts it consulted and those it skipped. Step 2 first, for the reason Section 4 gives: filtering
without labelling introduces a silent failure in exchange for removing a loud one.

**Not in this track.** F11 proper (pollable tasks) and F12's store leg (which wants a fault-injecting
adapter the port conformance suite would use broadly) are independent. F11's claim gets weaker after
Step 3, since a round that consults a map is no longer the "one small delta exchange" its deferral
rests on.

## 7. Invariants

These are implementation obligations. What must hold is F21; these are what the code has to do so
that it does.

| | Invariant |
|---|---|
| **N1** | Negotiation happens in the background health loop only. A tool handler reads the cached map and performs no network I/O to obtain it (F11). |
| **N2** | The map is link-local runtime state and is never written to the observation log. Nothing folds over it, so no proposition depends on it (F21.3). |
| **N3** | The map may only narrow what this node asks of a host. It never extends `share_workspaces` - a read-authorization answer must not become a write-authorization decision (F21.2). |
| **N4** | An unreachable host yields *unknown*, never an empty grant set. A host that is down must not read as a grant that was revoked (F21.4, F12). |
| **N5** | Every response the map narrowed names the hosts consulted and the hosts skipped as not-admitted. Filtering does not ship before labelling (F21.5, P5). |
| **N6** | The difference is reported in three buckets and never as the intersection, which would hide the misconfiguration that produced it. |
| **N7** | The map carries the time it was negotiated and is never a premise for a durable conclusion (F21.6). |
| **N8** | Two configuration shapes present at once is a startup failure, not a precedence rule (P5). |
| **N9** | A node whose own id appears in its own allowlist is refused at startup (F14). |
| **N10** | What a host advertises is the caller's entitlement and never the host's inventory. This is a property of the existing handler and must survive the change (F21.1, P17). |

## 8. Closure map

| Demand | Where it is answered |
|---|---|
| P17 - nothing leaves by default, on the host axis and not only the workspace axis | Sections 3, 4; N3, N10 |
| P17 - the boundary governs the remote read surface too | Section 4; N5 |
| P18 - a remote claim never binds the receiver's own policy | Section 5; N3 |
| P5 - absence is unknown, never negation | Section 4; N4, N5 |
| P5 - explicit configuration works or fails, never degrades silently | Section 5; N8 |
| P16 - a response says which surface, and which hosts, answered it | Section 4; N5 |
| P16 - convergence and monotonicity are different properties | Section 3; N7 |
| P20 - adapters depend inward, never on each other | Section 2 |
| P21 - a recurring intent, not a new tool: `sync_status` already reports the share list | Section 6 step 2 |
| F11 - sync never blocks a tool handler | Section 6 step 2; N1 |
| F12 - a failure is reported as a failure, never as empty | Section 4; N4 |
| F14 - the sync role refuses a colliding node id | Section 5; N9 |
| F21 - all six clauses | Sections 3, 4, 5; N1-N10 |
| Host discovery, sub-workspace grain, peer admission | Out of scope by federation.md Section 11 |
