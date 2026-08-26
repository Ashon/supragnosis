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
narrowed states which hosts it skipped as not-admitted, and that has to ship before anything starts
skipping.

One thing this section claimed and should not have: that routing carries P17's "nothing leaves by
default" onto the host axis. It does not. A host is dropped only on an explicit refusal (N11), so the
host-axis default is to ask - and it has to be, or ignorance reads as refusal. What may leave was
already settled by `share_workspaces` (F9) and what may be read by the host's allowlist; the map
narrows rounds across two boundaries that already existed rather than adding a third.

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
```

A per-server `share_workspaces` belongs on the same entry and is deliberately **not** accepted yet. A
share list that parses and is not obeyed reports a narrowing the system did not perform, so it
arrives with Step 3, the step that obeys it. Adding a field later is the open direction of P10;
accepting one before its consumer exists is a silent degrade.

Both shapes present resolves by **precedence, loudly**: the per-server entries win, the flat keys are
reported as ignored, and the node starts. Refusing was the first reading of P5 here and it was too
strict - P5 asks configuration to work or fail and never to degrade *silently*, and a precedence rule
that names what it dropped is not silent. Per-server wins because the other direction is worse than
losing a host from a round: one flat token presented to a host that has its own would be the wrong
credential on the wire.

The same reasoning covers a missing `auth_token` and F14's self-admitting entry - see N8 and N9. None
of the three stops the daemon. A release that breaks startup for a configuration mistake teaches
operators to fear upgrades, and every one of these has a reading that narrows instead.

## 6. Editing this configuration from a console

The per-peer narrowing act (federation.md 6a) already writes `supragnosis.toml` from the viewer:
`toml_edit` parses the file into a mutable document, one value is replaced, and the document is
written back with the operator's comments and layout intact. Nothing about that machinery is specific
to narrowing - it edits any key - so the question of whether a console may manage the rest of this
configuration is a policy question, not a capability one.

**The answer given in 6a does not transfer to the local desktop shell, and reading it as if it did was
a mistake worth recording.** 6a's argument is about a threat: an act that can only narrow means a
mistake, or a console left open, can only share less. That distinguishes a console from a file editor
only where the console is reachable by someone who could not edit the file. On the hub that is exactly
the case - the human surface is a network surface authenticated by enrolled user keys (F19), a
different boundary from the file - and 6a is written for the hub. Locally it is not: the viewer is a
unix socket at mode 0600, so whoever can reach it is whoever can write the config, the same OS
account. Applying the hub's asymmetry to the local shell forbids nothing an attacker could not
already do and costs the operator a surface for no gain.

What does constrain a console editor is elsewhere, and it is P5 rather than P17.

- **Writing works; taking effect mostly does not.** `fed::load` runs at exactly two points in the
  daemon: startup, and the narrowing handler's re-read. That handler feeds one live structure, the
  admission directory. Every other value - the server links, their credentials, the outbound share
  list, the origin keys - is a startup snapshot cloned into the running state. A console that edits
  them and reports success reports an application that did not happen, and the store is single-process
  so the restart that would apply it is not free. Either a value gets a live path or the surface says
  "on restart"; what it must not do is stay silent about which.
- **Validate before writing, not after.** The narrowing handler writes first and re-reads second, and
  its own error string admits the order: "narrowed on disk, but re-reading the config failed". For one
  narrow act that is survivable. A general editor inheriting that order can leave a document on disk
  that the next start refuses - unknown keys, a bind that violates F10, two server shapes at once -
  which turns one console mistake into a node that will not come up. The candidate document has to
  parse into the config type before it replaces the file.
- **A credential is write-only on this surface.** The status blob already declines to publish
  `bearer_hash`, on the grounds that a credential-shaped field with no reader is only a liability
  (P17/P18). A console may send a token; it must not be able to read one back.

Ordering follows from this. The three-bucket difference (Section 3) is what makes a configuration
editor worth having, because it puts the host's own answer beside the file being edited. Without it a
console edits blind against local belief - which is the state the drift view is in today, and the
reason a workspace the hub does not admit simply vanishes from it.

## 7. Ordering

Three steps. Each is useful alone, each lands on a green tree, and only the third changes behaviour.
The sequence is not the order the work was first imagined in - filtering was the interesting part and
came first, until the review put two constraints on it.

**Step 1 - per-server credentials, and F14's refusal. [done]** `[[sync.server]]` entries carrying a
url and a credential each, honored everywhere a client is opened: the health loop, the one-shot CLI
round, and the three MCP fan-out sites. Both configuration shapes present at once fails at load. A
node whose own id sits in its own allowlist is refused. It is first because Step 3 rests on an
identification a shared credential does not support (Section 5), so doing it later would ship a
routing decision on a premise already known to be weak.

The step landed narrower than this document first described it: the shape it showed included a
per-server share list, and Step 1 has no consumer for one (Section 5).

**Step 2 - keep the answer, report the difference. [done]** The health loop stops discarding
`shared_workspaces` and writes the typed map; the drift view becomes the three buckets with their
negotiation time; `sync_status`, the viewer's federation blob and the viewer's own federation panel
expose it. Read-only throughout.

One thing the step found rather than built: the old drift view called `advertise` for every workspace
on the local share list, and a host that does not admit one answers `403`, which the loop discarded
with a bare `continue`. So a misconfigured workspace did not merely go unexplained - it left the
panel. It is now named in `local_only`, which is the bucket that exists for it.

> **The constraint this step exists under.** F11 - sync never blocks a tool handler - is already
> unmet: the `sync_*` tools ship as ordinary blocking calls. Negotiating inside a handler would add a
> round trip per configured server to a call that already blocks, deepening an invariant this build
> does not meet. The health loop already pings on a slow interval and already holds shared state, so
> **negotiation stays there and handlers read the cached map**. This is not a new norm; it is F11
> applied to work that had not existed yet.

**Step 3 - route, and say what was skipped. [done]** The daemon's three fan-out sites - `sync_push`,
`sync_pull` and federated `search` - consult the map, and each narrowed response carries a `skipped`
list. The two halves shipped in one change, for the reason Section 4 gives: filtering without
labelling trades a loud failure for an invisible one.

**A host is skipped only when it said so.** `admits: None` is unknown - not asked yet, or the last
check failed - and skipping on that would turn "not asked" into "not allowed", the absence-as-
negation reading P5 forbids and the unknown-versus-empty collapse F12 names. Unknown is consulted,
so the host's own `403` stays reachable. An empty grant set, by contrast, IS an answer ("admitted,
may read nothing", 6a) and does narrow.

The wiring is checked over the real protocol, not by reading it. A context carrying two hosts - one
that admits the workspace, one that answered that it does not - drives `sync_push` and `sync_pull`
through an in-process MCP client, and the response both names the refusing host in `skipped` and
omits it from the per-server results. No transport is needed for that, because the routing decision
comes from the negotiated map, which is ordinary state.

**The CLI's one-shot round deliberately does not narrow**, which is a departure from this document's
first plan. `supragnosis sync --workspace X` is an explicit request for one workspace, and narrowing
it would turn that request into a silent no-op. Worse, the CLI holds no map - there is no health loop
in a one-shot process - so it would have to ping first to decide whether to advertise, and the
advertise it was going to make answers `403` with the workspace and the node named. That is strictly
more information for strictly less work. Narrowing earns its place where a round fans out across
hosts, not where one command names one target.

**Not in this track.** F11 proper (pollable tasks) and F12's store leg (which wants a fault-injecting
adapter the port conformance suite would use broadly) are independent. F11's claim gets weaker after
Step 3, since a round that consults a map is no longer the "one small delta exchange" its deferral
rests on.

## 8. Invariants

These are implementation obligations. What must hold is F21; these are what the code has to do so
that it does.

| | Invariant |
|---|---|
| **N1** | Negotiation happens in the background health loop only. A tool handler reads the cached map and performs no network I/O to obtain it (F11). |
| **N2** | The map is link-local runtime state and is never written to the observation log. Nothing folds over it, so no proposition depends on it (F21.3). |
| **N3** | The map may only narrow what this node asks of a host. It never extends `share_workspaces` - a read-authorization answer must not become a write-authorization decision (F21.2). |
| **N4** | An unreachable host yields *unknown*, never an empty grant set. A host that is down must not read as a grant that was revoked (F21.4, F12). |
| **N5** | Every response the map narrowed names the hosts skipped as not-admitted. Filtering does not ship before labelling (F21.5, P5). |
| **N11** | A host is dropped from a round only on an explicit refusal. Unknown is consulted, so "not asked yet" can never present as "not allowed" (P5, F12). |
| **N12** | Narrowing applies to fan-out, not to a command naming one target. A one-shot round keeps the host's own answer, which says more than a skip (P5). |
| **N6** | The difference is reported in three buckets and never as the intersection, which would hide the misconfiguration that produced it. |
| **N7** | The map carries the time it was negotiated and is never a premise for a durable conclusion (F21.6). |
| **N8** | A federation configuration mistake disables or narrows federation and never stops a node serving its own knowledge - the shape a missing embedder and an unbindable viewer socket already use. Every workaround is reported in `sync_status`, so degrading is loud rather than silent (P5). |
| **N9** | An allowlist entry naming this node is dropped in the admission directory, through every construction path, and the file is left alone (F14). |
| **N10** | What a host advertises is the caller's entitlement and never the host's inventory. This is a property of the existing handler and must survive the change (F21.1, P17). |

## 9. Closure map

| Demand | Where it is answered |
|---|---|
| P17 - what leaves is decided by this node's share list, which the map never widens | Sections 3, 5; N3, N10 |
| P17 - the boundary governs the remote read surface too | Section 4; N5 |
| P18 - a remote claim never binds the receiver's own policy | Section 5; N3 |
| P5 - absence is unknown, never negation | Section 4; N4, N5 |
| P5 - explicit configuration works or fails, never degrades silently | Section 5; N8 |
| P16 - a response says which surface, and which hosts, answered it | Section 4; N5 |
| P16 - convergence and monotonicity are different properties | Section 3; N7 |
| P20 - adapters depend inward, never on each other | Section 2 |
| P21 - a recurring intent, not a new tool: `sync_status` already reports the share list | Section 7 step 2 |
| F11 - sync never blocks a tool handler | Section 7 step 2; N1 |
| F12 - a failure is reported as a failure, never as empty | Section 4; N4 |
| F14 - the sync role refuses a colliding node id | Section 5; N9 |
| F21 - all six clauses | Sections 3, 4, 5; N1-N12 |
| P5 - unknown is not a refusal, and a narrowed answer says so | Section 7 step 3; N11, N12 |
| P5 - a console edit that does not take effect must say so | Section 6 |
| P5 - a written configuration parses, or it is not written | Section 6 |
| P17/P18 - a credential-shaped field has no reader on a status surface | Section 6 |
| Host discovery, sub-workspace grain, peer admission | Out of scope by federation.md Section 11 |
