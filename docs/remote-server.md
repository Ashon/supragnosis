# Remote server - one client setup, any supragnosis server

> How an AI app, the desktop app, or an agent on another machine uses a supragnosis server it can
> reach over the network - and how that server decides what each of them may read and write.
> Companion to [client-connect.md](client-connect.md) (the bridge every AI app launches) and
> [federation.md](federation.md) (nodes that replicate; Section 6 is the trust model this extends).
>
> Status: **built** (Section 10 steps 1-3); step 4 is specified in [remote-viewer.md](remote-viewer.md),
> and step 5 here. What building it changed is in Section 12.

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
- **Not remote governance, yet.** Verdicts, T-Box changes and node operations need an identity
  stronger than a bearer token (federation.md 6d, level ii). Until principals can sign their own
  acts, the remote surface refuses them (Section 4.3).

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
  is reached over HTTPS only, verified against the system roots or the profile's CA. There is no
  option to skip verification, because a skipped check hands the credential to whoever sits on the
  path.
- **Headless use (c).** `SUPRAGNOSIS_SERVER_URL` and `SUPRAGNOSIS_SERVER_TOKEN_FILE` select a server
  without a profile file. This is for a container that runs the bridge. An agent whose MCP client
  speaks HTTP can skip the bridge and send the token itself.

## 4. The server: a governed agent surface on the hub

### 4.1 Where it is served

On the hub's sync listener, at `/mcp`, beside the sync API. This is the listener that already
terminates TLS in-process and admits only what its configuration names (federation.md 6a, F10). The
bind rule generalizes: a non-loopback listen requires TLS, plus at least one admitted node or one
principal. A listener with neither still refuses to start (Principle 24: no surface open that the
configuration did not authorize).

### 4.2 Principals

A **principal** is a person or an agent the hub admits to its agent surface. Each one is configured
in `supragnosis.toml` as a name, the hash of its bearer credential, and per-workspace `read` and
`write` grants.

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

Workspace enumeration, wherever it appears, is filtered to the principal's grants. A no-workspace
global query has no remote form (federation.md 6c, the second-door rule).

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

Withdrawal stops future serving. It cannot recall what a principal has already read; that is true of
every disclosure, and the reason this one asks first.

### 4.6 Limits

The remote surface accepts request bodies up to 1 MB and times requests out. It records one audit
line per request: the principal, the tool and the workspace, never the content. Rate limits per
principal are recorded as owed (Section 10), following the adversarial review's DoS findings for the
sync surface.

## 5. The desktop app as a client

The tray gains a **Server** submenu listing the profiles, with the active one checked:

- **The status line names the server and whether it answers**, so a person can tell at a glance
  which body of knowledge their AI apps are using.
- **On a remote profile, Start at Login and Restart Daemon do not apply** and are disabled, since
  the daemon is not this machine's to run.
- **AI Apps is unchanged.** It registers the same bridge whichever server is active - which is the
  point of the profile.
- **The viewer cannot show a remote server yet.** It reads the local daemon over a unix socket.
  Showing a remote server needs federation's network read tier (Phase 3.5): TLS, per-principal
  grants filtering every read, no state-changing request, and the web hardening federation.md 6d
  lists. That is Section 10's step 4, specified in [remote-viewer.md](remote-viewer.md). Until it
  lands, the viewer window on a remote profile says which server the AI apps use and that browsing
  it needs a newer release - not an empty graph, which would read as "no knowledge" (P5).

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
| **R1** | The local MCP daemon stays loopback-only. Only the hub's TLS listener serves MCP beyond loopback, and only with TLS and at least one admitted principal or node. |
| **R2** | Every remote request is authenticated as exactly one principal. There is no anonymous remote read or write (F19). |
| **R3** | A principal reads and writes only granted workspaces, enumeration included. Every tool has a declared remote policy, and a tool without one is refused. |
| **R4** | A remote write is recorded `host` = hub, `on_behalf_of` = principal, at no higher than `AgentExtracted`. A client-supplied `on_behalf_of` cannot replace the principal. |
| **R5** | Content another node originated is served remotely only with that node's consent for that workspace. Without it the answer is a refusal, never a partial view. |
| **R6** | No credential appears in an AI app's configuration or on a command line. The bridge and the CLI read it from a 0600 file. |
| **R7** | Verdicts, T-Box changes and node operations are refused on the remote surface until principals can sign their own acts. |
| **R8** | A remote profile is reached over verified TLS only. There is no option to skip verification. |

## 9. What this revises

- **federation.md 6a** says the MCP loopback bind guard is unchanged. It stays unchanged for the
  local daemon (R1). The hub's sync listener gains the governed agent surface of Section 4, under
  the generalized bind rule of Section 4.1.
- **federation.md 6c's second-door rule** is what Section 4.3 implements: the same workspace grants,
  and no remote global query.
- **client-connect.md Section 2** says the bridge reaches only the local daemon. It reaches the
  server its active profile names; the local daemon is the default profile. C1 (the bridge opens no
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
   changes (federation.md 6d level ii).

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

- **Client profiles**: `a_remote_server_is_reached_over_verified_tls_or_loopback`,
  `the_environment_names_a_server_only_with_a_credential_file`.
- **The remote policy (R3, R4, R7)**: `every_tool_has_a_remote_policy`,
  `a_principal_reads_only_its_grants`, `a_remote_write_is_the_principals_own`,
  `governance_and_node_operations_are_refused_remotely`.
- **The hub, end to end over HTTP through the bridge (R2-R4)**:
  `principals_are_admitted_and_revoked_through_the_file`,
  `an_id_outside_the_grants_reads_as_an_unknown_one`,
  `a_principal_sees_and_writes_only_what_it_was_granted`.
- **Consent (R5)**: `another_nodes_knowledge_needs_its_consent`,
  `consent_rides_a_header_and_an_older_node_sends_none`, `consent_is_kept_and_can_be_withdrawn`,
  `serving_is_narrowed_to_what_is_shared`.

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

- **Consent travels in a header, not a request field.** An older hub ignores a header it does not
  know. A new field in the sync request body could fail its strict request parsing. An older node,
  in turn, sends no header, which the hub reads as no change rather than as a withdrawal.
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
  observations in its own workspace. Writing remote-viewer.md found it (Section 3.4 there).

