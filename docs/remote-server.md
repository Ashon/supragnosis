# Remote server - one client setup, any supragnosis server

> How an AI app, the desktop app, or an agent on another machine uses a supragnosis server it can
> reach over the network - and how that server decides what each of them may read and write.
> Companion to [client-connect.md](client-connect.md) (the bridge every AI app launches) and
> [federation.md](federation.md) (nodes that replicate; Section 6 is the trust model this extends).
>
> Status: **built** (Section 10 steps 1-3), **then removed in 0.11.0** (Section 13); step 4 was
> specified in [remote-viewer.md](remote-viewer.md) and step 5 here, and neither ships. What building
> it changed is in Section 12.
>
> **Removed in 0.11.0 (decided 2026-10-24).** The hub is kept minimal: each machine joins it, syncs,
> and uses its own daemon - its AI apps reach that daemon through the bridge as before - so nothing
> needs AI apps pointed at a server elsewhere. Frozen on 2026-10-23, the surfaces this document built
> are removed: Section 13 says what goes, what stays, and what a machine that used them meets after
> upgrading. Sections 1-12 are the design as it was built.

## 1. Why this exists

A supragnosis node serves MCP on loopback only, and its viewer on a unix socket. That is deliberate
(federation.md 6a): the local surfaces stay on the machine. Another machine can reach a node's
knowledge in two ways today: by running a node of its own and syncing with a hub, or through an SSH
tunnel to the viewer socket. Three situations need more than that:

- **(a) One person, several machines.** A laptop and a desktop share one body of knowledge kept on
  an always-on machine. Running a full node on every machine is one answer, and it stays available.
  But a machine that only needs to ask and record should not have to run a daemon, hold a replica,
  and sync.
- **(b) A team.** People who have not installed a node - and may never - need to read and add to
  the workspaces they were given, under their own names.
- **(c) Agents elsewhere.** An agent in CI, in a container or on a remote box speaks MCP over HTTP.
  It has no desktop app, no bridge it can rely on, and no node of its own.

The client half of the answer already exists. Every AI app is configured with `supragnosis bridge`
(client-connect.md), and the bridge decides where requests go. If the bridge, the desktop app and
`status` all read a **server profile**, then pointing a machine at another server is one local
change. Every AI app's configuration stays exactly as it is. What is missing is the server half: a
node that serves MCP beyond loopback, to principals it knows, under the same sharing rules as sync.

## 2. What this is NOT

- **Not a hosted service.** The server is a supragnosis node someone runs - usually the hub. There
  is no account, no cloud, and nothing reachable that its operator did not configure.
- **Not a replacement for local-first.** The default profile is this machine. A machine with its own
  node keeps full sovereignty, works offline and can still sync. A remote profile trades all three
  for not running a node, and that trade is the person's to make, per machine.
- **Not a widening of the local daemon.** The local MCP daemon stays on loopback and the viewer
  stays on its unix socket. Only the hub's TLS listener - the surface that already admits other
  machines for sync - gains a second, separately governed door.
- **Not remote governance, yet.** Verdicts and T-Box changes need an identity stronger than a
  bearer token (federation.md 6d, level ii), and a T-Box change also needs the `tbox_change` gate
  (F18). Until principals can sign their own acts, the remote surface refuses both (Section 4.3).
  Node operations are refused for good: they are the node's acts, not a principal's.

## 3. The client: server profiles

The client side holds a list of servers and one active profile:

- **`~/.supragnosis/client.toml`** names the profiles. Each remote profile has a URL, an optional CA
  bundle for a server whose certificate a private CA issued, and the path of its credential. The
  credential itself is a 0600 file under `~/.supragnosis/servers/` - never inline, never in an AI
  app's configuration (client-connect.md C2, extended).
- **`local`** is the built-in profile: the loopback daemon and its token file, exactly as today. It
  is the default and cannot be removed.
- **The CLI** manages the list:
  - `supragnosis server` shows the active profile and whether it answers.
  - `supragnosis server add <name> <url>` reads the credential from stdin, so it never appears on a
    command line.
  - `supragnosis server use <name|local>` switches; `supragnosis server remove <name>` deletes.
- **The bridge** reads the active profile on every connect, as it already reads the token. Switching
  profiles therefore takes effect at the next session with no change in any AI app. A remote profile
  is reached over HTTPS, verified against the system roots or the profile's CA; plain HTTP is
  accepted only to a loopback host, where no network sits between the two ends. There is no option
  to skip verification, because a skipped check hands the credential to whoever sits on the path.
- **Headless use (c).** `SUPRAGNOSIS_SERVER_URL` and `SUPRAGNOSIS_SERVER_TOKEN_FILE` select a server
  without a profile file. This is for a container that runs the bridge. An agent whose MCP client
  speaks HTTP can skip the bridge and send the token itself.

## 4. The server: a governed agent surface on the hub

### 4.1 Where it is served

On the hub's sync listener, at `/mcp`, beside the sync API. This is the listener that already
terminates TLS in-process and admits only what its configuration names (federation.md 6a, F10). The
bind rule generalizes: a non-loopback listen requires TLS, plus at least one admitted node or one
principal. A listener with neither still refuses to start (Principle 24: no surface open that the
configuration did not authorize). *(Revised by pairing.md: an open invitation is also an admission,
and a listener that may not start leaves the rest of the node serving, saying why.)*

### 4.2 Principals

A **principal** is a person or an agent the hub admits to its agent surface. Each one is configured
in `supragnosis.toml` as a name, the hash of its bearer credential, and per-workspace `read` and
`write` grants.

The word names an **access identity**, not the governance principal of federation.md Section 7b (a
human or an org, whose authority a verdict carries). Admission authenticates a caller and scopes what
it reads and writes; it says nothing about who stands behind an act, and the operator may give an
agent or a pipeline its own credential (Section 6, (c)). So an admitted principal is never, by
being admitted, a stakeholder: it does not count as a distinct principal for I9, and it cannot carry
a human's direct act (I17, F17). That is why verdicts and T-Box changes stay off this surface (R7).
Whether an access identity can ever stand as a governance principal is decided where principals
enroll signing keys (federation.md 6d, 7a; M4 Phase 5), and nowhere sooner.

- `supragnosis principal add <name> --read <ws,..> --write <ws,..>` generates a credential, prints
  it once for the operator to hand over, and stores only its hash. `principal remove` revokes it.
  Admission is read per request, like the node allowlist, so either change takes effect without a
  restart.
- `write` implies `read` for the same workspace.
- A principal is not a node. It has no signing key and replicates nothing, and the hub records its
  acts under the hub's own identity (Section 4.4).

### 4.3 What each tool may do remotely

Every tool has a declared remote policy, and a tool without one is refused. A tool added later stays
closed until someone decides what it may do (R3).

| Tool | Remote | Rule |
|---|---|---|
| `search_knowledge` | read | Workspace must be granted. Omitted means the principal's first read grant; `*` means the union of its read grants. `remote`/`both` scopes are refused, since the hub does not fan a principal's query out to its peers. |
| `get_entity` | read | The entity's workspace must be granted. An id outside the grants is answered exactly as an unknown id: "no entity with that id in the workspaces you may read", which does not claim absence (P5) and does not confirm existence elsewhere (Section 12). |
| `traverse` | read | The start must be granted, and a start outside the grants reads as an unknown one; hits outside the grants are dropped. |
| `workspace_map` | read | Workspace must be granted. |
| `list_proposals`, `get_proposal` | read | Workspace must be granted. |
| `observe` | write | Write grant on the target workspace. Recorded as Section 4.4 says. |
| `propose` | write | Write grant. A proposal is an assertion; committing it is a verdict, which is refused below. A `claim_promotion` or `claim_demotion` names only observations in its own workspace. |
| `review` | refused | A verdict needs the principal's own key (federation.md 6d level ii, P23 I17). |
| `define_type` | refused | A T-Box change. It waits for the same signed acts and for the `tbox_change` gate. |
| `sync_status`, `sync_pull`, `sync_push` | refused | Node operations, not a principal's. |
| Resources (`graph`, `hypergraph`, `observation`) | read | The workspace must be granted. An observation id outside the grants reads as an unknown one. |

Workspace enumeration, wherever it appears, is filtered to the principal's grants. The node-wide
query has no remote form (federation.md 6c, the second-door rule): an omitted workspace is the
principal's first read grant, and a `*` is answered from its grants alone, computed per workspace.

### 4.4 Provenance of a remote write

A remote `observe` is recorded with `host` = the hub, `on_behalf_of` = the principal's name, and
`session` = the MCP session (Principle 2: identity as a delegation chain). A client-supplied
`on_behalf_of` cannot replace the principal; the client's claim is not the authenticated fact. The
trust tier is at most `AgentExtracted`, the tier any agent's assertion gets. A remote write is an
attack surface like every other write (Principle 18), and a tier never rises by crossing the wire.

This is federation.md 6d's level (i), "honest but weak": the hub signs everything it records, so a
compromised hub could forge a principal's acts. That is acceptable for ingest, which commits nothing
to canon. It is not acceptable for verdicts, which is why Section 4.3 refuses them.

### 4.5 Content from other nodes needs its origin's consent

A hub's workspace can hold attestations that spokes synced to it. A spoke shared them **for
replication**. Serving them to people and agents the spoke never heard of is a further disclosure,
and Principle 17 says the origin decides that (federation.md 6d, republication consent).

- **A spoke consents per workspace.** It sets `[sync] serve_workspaces`, which must be a subset of
  `share_workspaces`, and says so when it syncs. The hub records each node's consent durably, and a
  later sync without the workspace withdraws it.
- **Origins are checked per workspace.** The hub serves workspace W to principals only if every node
  whose attestations W contains is the hub itself or has consented for W. Otherwise the remote
  answer is a refusal that names the unconsented origins - never a silently filtered partial view,
  which would read as "that is everything" (P5).
- **Older nodes have not consented.** A node too old to send consent has not given it, so the rule
  fails closed.

Withdrawal stops future serving. It cannot take back what a principal has already read; that is true of
every disclosure, and the reason this one asks first.

### 4.6 Limits

The remote surface accepts request bodies up to 1 MB and times requests out. It records one audit
line per request: the principal, the tool and the workspace, never the content. Rate limits per
principal are recorded as owed (Section 10), following the adversarial review's DoS findings for the
sync surface.

## 5. The desktop app as a client

> **Revised ([settings-page.md](settings-page.md)).** The Server submenu became the Server section of
> the app's settings page, which can also add a profile - credential included, handed to the CLI on
> stdin - and remove one. The tray keeps the status line naming the server and its state.

*(Revised by design-system.md step 50b: the tray's line reads "Server home - answering", "Server
home - not answering" or "Server home - refused the credential", an act line "Check the Server..."
follows it while the server does not answer or refuses, and the icon's trouble shape shows then,
design-system.md Section 29.13.)*

The tray gains a **Server** submenu listing the profiles, with the active one checked:

- **The status line names the server and whether it answers**, so a person can tell at a glance
  which body of knowledge their AI apps are using.
- **On a remote profile, Start at Login and Restart Daemon do not apply** and are disabled, since
  the daemon is not this machine's to run.
- **AI Apps is unchanged.** It registers the same bridge whichever server is active - which is the
  point of the profile.
- **The viewer does not show a remote server.** It reads the local daemon over a unix socket. On a
  remote profile the window says which server the AI apps use, whether it answers, and how to switch
  back - not an empty graph, which would read as "no knowledge" (P5). The hub's read tier exists
  (Phase 3.5, [remote-viewer.md](remote-viewer.md)), but the app's way to it was withdrawn before
  release on 2026-10-23, when the hub was kept minimal: a machine that wants the hub's knowledge in
  its window joins the hub and syncs, and reads its own console.

## 6. The three cases, end to end

- **(a) One person, several machines.**
  - **Server**: the always-on machine runs a node with `[server]` configured, TLS, and one principal
    for the person, with read and write on their workspaces.
  - **Machines without a node**: the person runs `supragnosis server add home <url>` and
    `server use home`; AI Apps stays as it was.
  - **Machines with a node**: they keep syncing, and set `serve_workspaces` so the hub may serve
    what they share.
- **(b) A team.** The hub's operator adds a principal per member with the grants each should have.
  Members without a node use a remote profile. Members with one keep syncing and decide, per
  workspace, whether the hub may serve their contributions to the others.
- **(c) Agents elsewhere.** The operator adds a principal per agent or per pipeline, usually with
  write on one workspace. The agent either runs the bridge with `SUPRAGNOSIS_SERVER_URL` and a token
  file, or points its HTTP MCP client at `https://<hub>/mcp` with the token as a bearer header. Its
  writes arrive as `on_behalf_of = <agent>` at `AgentExtracted`, like any agent's.

## 7. Decisions

- **One listener, two doors.** Serving remote MCP on the sync listener reuses the one network
  surface the hub already governs: its TLS, its bind rule, its per-request admission. A second
  network listener would be a second thing to configure safely.
- **Bearer credentials now, keys later.** Section (c) needs a credential an ordinary HTTP MCP client
  can send, and that is a bearer token. federation.md 6d's user keys - proof of possession,
  principal-signed acts - remain the path to remote governance. The bridge, being our code on the
  person's machine, is where that signing will happen.
- **Refuse, do not filter, when consent is missing.** A partial view presented as complete is the
  silent absence Principle 5 forbids. A refusal that names the missing consent tells the principal
  what to ask for.
- **The profile lives with the client, not the AI apps.** That one indirection is what lets every AI
  app's configuration survive a change of server.

## 8. Invariants

| | Invariant |
|---|---|
| **R1** | The local MCP daemon stays loopback-only. Only the hub's TLS listener serves MCP beyond loopback, and only with TLS and at least one admitted principal or node, or an open invitation (pairing.md PA1). *(Revised by pairing.md.)* |
| **R2** | Every remote request is authenticated as exactly one principal. There is no anonymous remote read or write (F19). *(Revised by pairing.md: `preview` and `join` carry an invitation's secret and `leave` a joined node's credential; none is a principal's request, and none reads or writes knowledge.)* |
| **R3** | A principal reads and writes only granted workspaces, enumeration included. Every tool has a declared remote policy, and a tool without one is refused. |
| **R4** | A remote write is recorded `host` = hub, `on_behalf_of` = principal, at no higher than `AgentExtracted`. A client-supplied `on_behalf_of` cannot replace the principal. |
| **R5** | Content another node originated is served remotely only with that node's consent for that workspace. Without it the answer is a refusal, never a partial view. |
| **R6** | No credential appears in an AI app's configuration or on a command line. The bridge and the CLI read it from a 0600 file. |
| **R7** | Node operations are never served to a principal. A verdict is refused on the remote surface until it can arrive as the principal's own signed act, and a T-Box change until it can arrive that way through the `tbox_change` gate (F18). |
| **R8** | A remote profile off loopback is reached over verified TLS only; plain HTTP is accepted only to a loopback host. There is no option to skip verification. |

## 9. What this revises

- **federation.md 6a** says the MCP loopback bind guard is unchanged. It stays unchanged for the
  local daemon (R1). The hub's sync listener gains the governed agent surface of Section 4, under
  the generalized bind rule of Section 4.1.
- **federation.md 6c's second-door rule** is what Section 4.3 implements: the same workspace grants,
  and no remote node-wide query.
- **client-connect.md Section 2** says the bridge reaches only the local daemon. It reaches the
  server its active profile names; the local daemon is the default profile. client-connect.md C1 (the bridge opens no
  store and starts no daemon) is unchanged.

## 10. Ordering

1. **Server profiles**: `client.toml`, `supragnosis server`, the bridge reading the active profile,
   and the headless environment variables. The tray's Server submenu and status line follow.
2. **The hub's agent surface**: principals and `supragnosis principal`, `/mcp` on the sync listener,
   the remote policy table and its guard, Section 4.4 provenance, Section 4.6 limits. Until step 3,
   it serves only workspaces whose content the hub itself originated (R5, failing closed).
3. **Consent**: `serve_workspaces` on spokes, recorded by the hub, checked per workspace.
4. **The remote viewer**: federation's Phase 3.5 network read tier, and the desktop app browsing a
   remote server ([remote-viewer.md](remote-viewer.md)).
5. **Signed acts**: principal keys held by the bridge, and with them remote verdicts and T-Box
   changes (federation.md 6d level ii). [not scheduled: due when a second person uses a hub - the hub
   is kept minimal, 2026-10-23]

Owed beyond these: per-principal rate limits, and credential rotation without removal and re-adding.

## 11. Closure map

| Principle | Where this closes it |
|---|---|
| P2 - provenance, identity as a delegation chain | Section 4.4; R4 |
| P5 - unknown is not absent | Sections 4.3 (`get_entity`), 4.5 (refusal, not partial view), 5 (viewer on a remote profile) |
| P17 - knowledge sovereignty | Sections 4.3 (grants, second door), 4.5 (republication consent); R3, R5 |
| P18 - writes are an attack surface | Section 4.4 (tier cap); R4, R7 |
| P23 - gate to canon, I17 | R7: verdicts stay off the remote surface until they can be a principal's own act |
| P24 - refuse when the configuration did not authorize | Section 4.1's bind rule; R1, R8 |
| F19 - no unauthenticated write surface | R2 |

Guarded by:

- **Client profiles**: the two tests of `profile.rs`, removed with it in 0.11.0 (Section 13).
- **The remote policy, the hub end to end and consent (R2-R5, R7)**: the tests of `principal.rs`,
  `remote.rs`, the MCP surface and the consent header that held them, removed with them in 0.11.0
  (Section 13). `a_node_says_no_to_serving_on_every_workspace_request` holds what stays: the "no" a
  0.11 node sends.

Checked end to end with two scratch homes - a hub on loopback with one principal, and a client
whose active profile points at it:
- `server` reported the hub answering.
- Through the bridge, an observe that claimed `on_behalf_of: "mallory"` landed in the hub's log as
  `alice`, at `agent_extracted`.
- Search answered inside the grant and was refused outside it, and `review` was refused.
- Every call left an audit line.
- `principal remove` on the running hub made the next request fail with the credential file named,
  without a restart.

## 12. What building it changed

- **Consent travels in a header, not a request field,** because it is about the whole request - the
  workspace it names - not about one event in it. An older hub ignores the header, as it would
  ignore an unknown body field: the sync wire types do not deny unknown fields (an earlier version of
  this line said otherwise; what an older peer cannot read is an unknown enum value,
  sync-correctness.md Section 12). An older node, in turn, sends no header, which the hub reads as
  no change rather than as a withdrawal.
- **Only `search_knowledge` takes `*` remotely.** `workspace_map`, `list_proposals` and
  `get_proposal` answer a `*` with the principal's grants, so the refusal lists the choices. A
  remote `*` search runs once per granted workspace and merges by score. It is never the node-wide
  search filtered afterwards, because a search hit does not carry its workspace.
- **Origins are read per call, not cached.** The engine's log epoch does not move when sync applies
  events (a known gap). A cache keyed on it could keep serving a workspace after another node's
  unconsented knowledge arrived. The scan is linear in the workspace's log; caching it is owed along
  with that gap.
- **rmcp's Host allowlist is off on the remote router.** It guards a loopback server against DNS
  rebinding. This surface is reached by whatever name the operator gives the hub, and every request
  carries a bearer credential that a rebinding page cannot attach.
- **Only POSTs are timed out (60 s).** A GET is the event stream a client keeps open on purpose.
- **An id outside the grants reads as an unknown one (v0.4.6).** Section 4.3 first answered such an
  id "not in a workspace you are granted", and an unknown one "not found". Both are true, but an
  entity id is the hash of a workspace and a name, and an observation id the hash of a workspace and
  a content. A principal who guessed both could therefore tell from the answer whether a named
  entity, or a sentence, was recorded in a workspace it was never granted. `get_entity`, `traverse`'s
  start and the observation resource now give one answer for both cases. A gate proposal checked its
  target observations against the whole log for the same reason, so on this surface it names only
  observations in its own workspace. Writing remote-viewer.md found it (remote-viewer.md Section 3.4).

## 13. Removed in 0.11.0

The author's machines sync through one hub and each uses its own daemon, console and bridge, so no
one uses a server elsewhere (architecture.md Section 14, M4). On 2026-10-24 the author chose to
remove what was frozen rather than keep carrying it: about 3,000 lines of code and 1,500 of tests
that every change to the MCP surface, the hub and the app had to step around.

**What goes.**
- **Server profiles:** `supragnosis server` (`add`, `use`, `remove`, `--json`), `client.toml`, the
  bridge's remote mode and `SUPRAGNOSIS_SERVER_URL`, `_TOKEN_FILE` and `_CA`, the remote branches of
  `setup` and `connect`, and the app's Server pane, tray line and remote state (Section 5).
- **Principals:** `supragnosis principal` (`add`, `remove`, `list`), `[[server.principals]]`, the
  hub's `/mcp`, the MCP surface's remote mode - `remote::admit`, its policy table and every tool's
  remote branch (Sections 3-4) - and the admin socket's `/server/refresh`, which only `principal`
  called.
- **Consent to serve:** `[sync] serve_workspaces`, `join --serve`, and the hub's record of consent,
  `served-consent.json` (Section 4.4), which is left in place and not read.
- **The read tier:** the hub's `/viz/`, its policy table and its audit lines (remote-viewer.md step
  1), and what steps 2-3 left in the app, the window's remote page.

**What stays.** The hub's listener for sync and pairing; each machine's daemon, console and bridge
to its own daemon; federated retrieval over sync (`search_knowledge`'s `scope`); and the local
viewer's `/api/surface` as it is, `surface: "local"` included, since the page gates its acts on it.
The viewer's branches for the read tier stay too: with the surface always local they are always
taken.

**What a machine that used them meets.**
- **A key that is no longer read.** `[[server.principals]]` and `[sync] serve_workspaces` still
  parse, as `insecure_tls` does, and do nothing. The daemon starts, and its log and `sync_status`'s
  notes say the key is no longer read since 0.11.0 and can be removed (Principle 24). Refusing the
  file would stop a node's own knowledge over a key that now does nothing. `hub leave` still removes
  the `serve_workspaces` entries a 0.10 join recorded.
- **Consent given before.** A 0.11 spoke still sends the `supragnosis-serve` header, saying no on
  each workspace request, so a 0.10 hub withdraws its consent for every workspace the two still
  sync: sharing less, the direction Principle 24 allows. The spoke's side of the header is the one piece of consent that stays; revisit when no
  0.10 hub remains.
- **A client that used a server elsewhere.** Nothing reads `client.toml` or `SUPRAGNOSIS_SERVER_*`
  any more, and the bridge uses this machine's daemon. No one uses a server profile today, and an AI
  app writing to its own machine's daemon leaks nothing, so this is said where an upgrade is read:
  the 0.11.0 release note says to delete `client.toml` and its `servers/*.token` credentials. A
  guard in the bridge is owed (Appendix A, assurance against proportion); revisit when a profile is
  reported still in use.
- **The pairing wire.** A hub's answer keeps `serves_principals`, declared on both sides and always
  false: a 0.10 joiner requires the field, and a 0.11 joiner that dropped it would refuse a 0.10
  hub's answer after being admitted.
- **Machine-read output.** `server --json` goes with its command, a breaking change named in
  CHANGELOG.md and its release note. `status --json` keeps its `server` field, always `{"name":
  "local", "remote": false}`, so its schema stays 1 (compatibility.md Section 6). Its `url`, present
  only on a remote profile, goes with the profiles; no app reads it.
- **The knowledge.** What a principal wrote stays in the log as written (Principle 3). Nothing in
  the store changes, and no store format moves.

**Invariants and the registry.** R1-R8 here and V1-V9 in remote-viewer.md stay in their tables,
since Sections 1-12 keep the design as built and other documents cite their rows, and the coverage
registry holds each of their clauses as removed: no route serves `/mcp` or `/viz/` on the hub. What
R1 kept that still matters is held elsewhere: the local daemon serves loopback only (Principle 17),
and the hub's listener binds off loopback only with TLS and an admitted node or an open invitation
(pairing.md PA1, whose admitted principal goes in step 3). A clause in another family that rested on
a removed test is decided in the step that removes it: Principle 17's read-tier clause rests on
federated retrieval's tests instead, F19's clauses return to owed, and the rest are reworded to what
still holds or retired.

**Order.** Each step is one change of about 500 lines without its tests, with its documents.
1. This section and the status lines that point here.
2. Server profiles in the CLI, the bridge, `setup` and `connect`; then in the app.
3. Principals and the read tier: the hub's router and `/server/refresh`, the MCP surface's remote
   mode, the read tier's policy, `[[server.principals]]` read and ignored, and PA1, F10 and
   Principle 24's example without the principal.
4. Consent: `serve_workspaces` read and ignored, `join --serve` removed, the header kept saying no,
   and the pairing field kept.
5. What is left in other documents, README.md, SECURITY.md and `deploy/`, and the principles page,
   whose answer demo routes two questions through a shared server.
