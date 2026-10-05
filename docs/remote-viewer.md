# Remote viewer - browsing a supragnosis server from the desktop app

> How the desktop app shows the knowledge of a supragnosis server on another machine: the read tier
> the hub serves to its principals, the relay that carries it to this machine, and what the window
> shows when it cannot. Step 4 of [remote-server.md](remote-server.md) Section 10, and federation's
> Phase 3.5 read tier ([federation.md](federation.md) 6d).
>
> Status: **specified**, not built.

## 1. Why this exists

Since v0.4.5 a machine's AI apps can use a server elsewhere, and the person cannot see what they are
using. On a remote profile the viewer window says which server is active and that browsing it
arrives later (remote-server.md Section 5):

- **Case (a), one person with several machines.** The knowledge the laptop records is invisible
  from the laptop.
- **Case (b), a team.** A member who never installed a node cannot look at the shared workspaces at
  all. The console is the one human surface supragnosis has, and it reads only a local daemon.

Federation planned for this as Phase 3.5: the hub serves its viewer to the people it admits,
read-only and filtered by their grants. Two things have changed since that plan was written.

- **Principals exist.** The hub already admits people and agents with a bearer credential and
  per-workspace grants (remote-server.md Section 4.2). Spokes already consent, per workspace, to
  having their contributions served (Section 4.5). The read tier needs neither a second identity
  system nor a second consent.
- **The desktop app is a client.** It reads the server profile, so it already knows which server
  the person means. What it lacks is a way to reach that server's viewer: the shell speaks HTTP
  over a unix socket and holds no TLS client.

There is also a guard owed since the viewer left TCP. Once a network read tier exists, workspace
enumeration and `workspace=*` must be filtered by the reader's grants:

- architecture.md Section 14, overdue item 4;
- the P17 registry row "an authenticated network read tier filters workspace enumeration by the
  reader's grants".

This is that tier, so it is that guard.

## 2. What this is NOT

- **Not a web app for browsers.** The hub serves no login page and sets no cookie. Its read tier
  answers requests that carry a principal's credential in a header, which the desktop app (through
  the relay, Section 5) and `curl` can send and an ordinary browser tab cannot. A cookie session
  would bring CSRF, session fixation and logout into a surface that has none of them. A browser
  login is owed with keys (Section 10).
- **Not a write surface.** Nothing on the read tier changes state, whatever the principal's grants:
  - A principal with a write grant writes through MCP (`observe`, `propose`).
  - Verdicts stay on a node's own console (remote-server.md R7).
  - The console's verdict path, which records a human's direct act and can grant
    `human_confirmed`, is never reachable from the network.
- **Not a change to the local viewer.** On the local profile the window reads the local daemon over
  its 0600 unix socket, with the full console, exactly as today.

## 3. The read tier on the hub

### 3.1 Where it is served

The read tier is served at `/viz/` on the hub's sync listener, beside `/mcp` and the sync API. It
shares the agent surface's TLS, bind rule and per-request admission (remote-server.md Section 4.1),
and is mounted wherever `/mcp` is. Every request authenticates as one admitted principal, the page
and its script included (V1). A request without one is answered 401 before any route runs.

The tier serves the hub's own page. The page and the API it calls are then always the same version,
whatever version the client's app is.

### 3.2 What each path may do

Every path has a declared policy, and a path without one is refused. An endpoint added to the
viewer later therefore stays closed until someone decides what it may do (V2). Only GET is accepted.

| Path | Remote | Rule |
|---|---|---|
| `/`, `/viewer.css`, `/viewer.js` | served | The page. |
| `/api/about` | served | Which build answers. |
| `/api/surface` | served | New, on both surfaces (Section 3.5). |
| `/api/workspaces` | read | The principal's granted workspaces that hold knowledge, unservable ones included (V3). |
| `/api/graph`, `/api/hypergraph`, `/api/types`, `/api/curation`, `/api/proposals`, `/api/observations` | read | The workspace as Section 3.3 resolves it. |
| `/api/proposal` | read | The named workspace must be granted and servable. `*` is refused, because a proposal lives in one workspace. |
| `/api/explain?entity=`, `/api/observations?entity=` | read | The entity's own workspace must be granted and servable. An id outside the grants is answered exactly as an unknown id is (Section 3.4). |
| `/api/events` | read | Filtered as Section 3.6 says. |
| `/api/federation`, `/api/peer/share`, `/api/health` | refused | A node's own operations and state - its peers, its sharing boundary, its store's ledger. They are not a principal's. |
| `/api/review`, `/api/resolve` | refused | Verdicts. These stamp the console surface, the one that records a human's direct act (V2). |
| `/api/reify`, `/api/propose_merge`, `/api/propose_split` | refused | Writes. A principal with a write grant reaches `observe` and `propose` through MCP. |

A refusal is a 403 whose reason says where the act can be done instead.

### 3.3 Which workspace a read means

The local viewer resolves an omitted workspace to the node default, and `*` to the whole store.
Neither means anything for a principal. The read tier resolves both against the grants, as the agent
surface does (remote-server.md Section 4.3):

- **Omitted** is the principal's first readable grant, in name order.
- **Named** must be granted and servable (R5). Otherwise the answer is a 403 that names the grants,
  or the origins that have not consented.
- **`*`, `all` or empty** is the union of the principal's grants:
  - Each projection runs once per granted workspace, and the results are merged. It is never the
    node-wide projection filtered afterwards, which would put the decision about what to show
    after the code that read it (V3).
  - If any granted workspace is unservable, the union is refused and the refusal names it. A union
    missing one workspace would read as the whole of what the principal may see (V4). The other
    workspaces can still be read one at a time.

### 3.4 Reads by id

An entity id is not a secret. It is the blake3 hash of the workspace and the lowercased name
(P14), so anyone who can guess a workspace name and an entity name can compute it. A read by id
that answered "not in a workspace you are granted" for an entity in someone else's workspace, and
"unknown" for one that does not exist, would let a principal test whether a named entity exists in a
workspace it was never granted.

So an id outside the grants is answered exactly as an unknown id is: the same status and the same
words, "no entity with that id in the workspaces you may read". The sentence is true in both
cases. It does not call the entity absent (P5), and it does not say where else it might be (P17).
The same holds for an observation id. A granted workspace that the hub may not serve is different:
the principal was granted it, so the refusal can say so and name the missing consent (R5).

The agent surface shipped in v0.4.5 does not do this. `get_entity`, `traverse` and the observation
resource refuse an id outside the grants in words that differ from their answer for an unknown id.
Section 10 closes that first.

### 3.5 `/api/surface`

The page needs to know whether it is the local console or a principal's read-only view. The
principal needs to know what they can see, and why some of it is closed. `/api/surface` answers
both:

- **Locally** it answers `{"surface": "local"}`, and the page behaves exactly as today.
- **On the read tier** it gives:
  - the principal's name, and that the surface is read-only;
  - the workspace an omitted one resolves to;
  - each granted workspace, whether it is servable and, if not, why.

The relay adds what only the client knows: the profile's name and URL. The page can then say "home
(hub.example) as alice, read only" without the hub knowing what this machine calls it.

### 3.6 The event stream

Locally, `/api/events` carries every MCP call: observes, searches with their query text, lookups,
traversals and sync rounds, each with the session it came from. On the read tier it carries
knowledge changes only:

- **It carries `observe` events** in workspaces the principal may read and the hub may serve, with
  the session id removed.
- **It does not carry searches, lookups or traversals.** What one principal asked, or the hub
  operator's own agents, is not knowledge in a workspace. Showing it to another principal would
  disclose their reading (V7).
- **It does not carry sync rounds.** Their counterpart is a peer node of the hub, which is the hub's
  network and not a principal's business.

The page polls as well, so a principal misses no change. All they lose is the live pulses of other
people's reads, which they should not have had.

### 3.7 Hardening

federation.md 6d made four demands of a network viewer. Each one stands as follows.

- **Output escaping.** Guarded today. Every untrusted value that reaches an innerHTML sink passes
  through `esc()`, a test pins it, and `no-unsanitized` runs over the sinks in CI.
- **A Content-Security-Policy.** Every viewer response carries one today, with a strict
  `script-src`. The read tier sends the same policy.
- **No credentials in URLs.** The credential travels only in the `Authorization` header that the
  relay adds (Section 5), and the page never holds it. A script that slipped past both defences
  could still read only what the principal may read. The policy's `connect-src 'self'` leaves it
  nowhere else to send what it read.
- **No state-changing GET.** The read tier's routes are a separate table that contains no write path
  (Section 3.2). A test holds every path the local router serves to a declared remote policy.

### 3.8 Limits and audit

- **Requests.** GET only, with no body. Each request times out at 60 seconds, except the event
  stream, which a client holds open on purpose.
- **Conditional answers.** Answers carry an `ETag` over their body and honour `If-None-Match`. The
  page polls the graph every 2.5 seconds, and across a network an unchanged graph should cost a
  hash, not a transfer. The relay (Section 5) keeps the last body for each request target and asks
  conditionally, so the page needs no change.
- **Audit.** A line is logged at most once a minute per principal and workspace. It names the
  principal, the path, the workspace and the number of reads since the last line, never the
  content. "Who read what, and when" stays answerable to the minute, without a line every 2.5
  seconds for every open window.
- **Rate limits.** Per-principal rate limits remain owed, as on the agent surface (remote-server.md
  Section 10).

## 4. Identity and consent: one of each

- **The principal is the reader.** The read tier admits the principals the agent surface admits,
  with the same credential and the same grants. Read is read, whichever door it comes through, and
  revoking a principal closes both doors at the next request.
- **`serve_workspaces` is the consent.** A spoke that lets the hub serve a workspace to its
  principals has consented to that workspace's knowledge being shown to them. MCP or the viewer is a
  difference of rendering, not of disclosure. federation.md 6d's `sync+web-read` share grade is
  this consent, so a spoke has one setting to reason about, not two.

## 5. The relay

The shell cannot reach the hub by itself:

- It holds no TLS client.
- It shares no code with the server, by design.
- The parts that reach a server correctly already live in the CLI, in the bridge: the active profile,
  the private CA, the credential read per request, and verification that cannot be turned off.

A second implementation in the shell would be a second R8 to keep right. So the CLI relays the
viewer the way the bridge relays MCP, with `supragnosis bridge --viewer <socket>`:

- **It binds a 0600 unix socket** inside the 0700 `~/.supragnosis`, and accepts what the shell
  already speaks: HTTP over a unix socket. A live socket at that path is refused rather than taken
  over, as the daemon refuses one.
- **It forwards GETs only** to the active profile's server. The address is the profile's MCP URL
  with its last path segment `mcp` replaced by `viz`: `https://hub.example:7420/mcp` becomes
  `https://hub.example:7420/viz/`. A profile URL that does not end in `mcp` is refused with the URL
  named, rather than guessed at.
- **It reads the credential from its 0600 file on every request**, and sends it as
  `Authorization: Bearer`. The credential is never written anywhere else, never put in a URL, and
  never handed to the shell or the page (V5).
- **It verifies TLS** against the system roots or the profile's CA, with no way to skip the check.
  Plain HTTP is allowed to loopback only (R8).
- **It streams `/api/events` through** as events arrive.
- **It runs until its stdin closes.** The shell starts it with a pipe, so the relay cannot outlive
  the app that started it, whether the app quits or crashes.
- **It reports failures in a form the shell can show.** Three cases come back as JSON with a
  `state`, never as an empty graph:
  - nothing answered;
  - the credential was refused, with the credential file named;
  - the server answered 404 at `/viz/`, meaning it is older than this tier.

  The shell renders each one (Section 6).
- **On the local profile it refuses to start.** The local viewer is the daemon's own socket, so
  there is nothing to relay.

## 6. The desktop app

On a remote profile, the window shows the server instead of saying it cannot:

- **The shell starts the relay** as a child, and points its `viz://` proxy and its event bridge at
  the relay's socket, `~/.supragnosis/remote-viz.sock`:
  - Switching to another remote profile restarts the relay.
  - Switching to this Mac stops it and points both back at the daemon's socket.
  - The page reloads on every switch.
- **The page reads `/api/surface`.** On the read tier it:
  - shows a banner naming the server and the principal, and marks the view read-only;
  - offers nothing the tier refuses: no verdict or accept controls, no reify, no propose actions on
    merge and split suggestions, no peers panel and no narrowing;
  - shows a granted workspace the hub may not serve with its reason, rather than leaving it out
    (P5).
- **A failure is a page, not an empty graph.** The shell shows the state and what to do about it,
  as it does today for a daemon that has not started (V8). The states are:
  - nothing answers;
  - the credential was refused;
  - the server is too old;
  - the principal may read nothing.
- **The tray is unchanged** from remote-server.md Section 5. The status line names the server and
  whether it answers, and the daemon controls stay disabled.

## 7. Decisions

- **Bearer credentials for the read tier now; keys later.**
  - federation.md 6d specified proof of possession of an enrolled user key, with a challenge and a
    short session.
  - The agent surface chose bearer credentials so that an ordinary MCP client can use it. The read
    tier takes the same credential, rather than a second identity system for the same people.
  - Keys arrive with step 5 and are held by the bridge. The relay is the bridge, so the read tier can
    accept a signed challenge then without the app changing.
- **The relay lives in the CLI, not the shell.** One implementation reaches a server: the profile,
  the CA and the per-request credential read. The shell keeps speaking HTTP over a unix socket, to
  whichever socket the profile says.
- **The hub serves the page.** The page and the API it calls are one version. An app newer or older
  than the hub shows the hub's console, which knows the hub's answers.
- **Read-only even for writers.** The read tier changes nothing:
  - Opening proposals from the remote window would need state-changing requests on a network
    surface, which means POST and the hardening that comes with it.
  - Verdicts need step 5's signed acts.

  Both wait. Writers have MCP meanwhile.
- **A union is computed, not filtered, and refused, not trimmed.** See Section 3.3. It is the same
  rule `search_knowledge` follows remotely (remote-server.md Section 12).
- **The remote event stream carries knowledge, not activity.** See Section 3.6.

## 8. Invariants

| | Invariant |
|---|---|
| **V1** | Every request on the read tier authenticates as exactly one admitted principal, the page and its assets included. There is no anonymous network read (F19). |
| **V2** | Every path on the read tier has a declared policy, and a path without one is refused. No path on it changes state, and the console's verdict path is unreachable from the network. |
| **V3** | A principal reads only granted workspaces. This covers enumeration, omitted and `*` workspaces, id-based reads and the event stream. `*` is the union of grants, computed per workspace. An id outside the grants is answered as an unknown id is, on both the read tier and the agent surface. |
| **V4** | A workspace is shown only when the hub may serve it (R5). A union that includes one it may not serve is refused with the reason, never shown in part. |
| **V5** | The credential never appears in a URL, the page or the shell. The relay reads it from its 0600 file per request and sends it as a header. |
| **V6** | The relay reaches a remote server over verified TLS only (R8), relays only GET, and cannot outlive the app that started it. |
| **V7** | One principal's reads are not shown to another. The remote event stream carries knowledge changes in readable, servable workspaces, without session ids, queries or lookups. |
| **V8** | On a remote profile, the window never shows an empty graph for a failure. Unreachable, credential refused, server too old and no grants each have their own page. |
| **V9** | The local viewer is unchanged: the daemon's unix socket, the full console, no credential. |

## 9. What this revises

- **federation.md 6d's read tier.** These revisions are applied to federation.md when step 1 of
  Section 10 lands:
  - Authentication is by an admitted principal's credential, not by proof of possession of an
    enrolled user key, until step 5.
  - The `sync+web-read` share grade is `[sync] serve_workspaces`.
  - Browser access, which 6d imagined through WebCrypto or a passkey, waits for those keys.
  - F19's first sentence is revised the same way.
- **remote-server.md Section 5** says the viewer cannot show a remote server yet. It can, through
  this tier.
- **remote-server.md Section 4.3**, the `get_entity` row. "Not in a workspace you are granted"
  becomes the unknown-id answer, for `traverse`'s start and the observation resource too (Section
  3.4).
- **architecture.md Section 14, overdue item 4, and the P17 registry row.** The guard owed "the
  moment federation Phase 3.5 opens" is Section 3.3 and V3. The row moves from deferred to
  evidenced when its test exists.
- **The viewer's bind policy is unchanged** (architecture.md Section 10). The daemon's viewer stays
  on its unix socket. The network tier is the hub's listener serving the same page through a
  different router, riding the sync crate's TLS stack as the bind policy already said it would.

## 10. Ordering

0. **Close the existence oracle on the agent surface** (Section 3.4). It ships in v0.4.5 and needs
   none of the rest, so it goes first and can be released on its own.
1. **The hub's read tier.**
   - In the viz crate, a remote router beside the local one: the path policy table, workspace
     resolution over a reader's grants, the filtered event stream and `/api/surface`.
   - It takes the reader and the servable check as plain values and functions, so the viz crate
     still knows nothing of the sync crate or the config file (P20).
   - The CLI mounts it at `/viz/` beside `/mcp`, behind the same admission, with the ETag and audit
     of Section 3.8.
2. **The relay**: `supragnosis bridge --viewer`.
3. **The desktop app**:
   - the relay's lifecycle, the socket switch and the failure pages;
   - in the page, `/api/surface` and the read-only controls.
4. **Docs**: federation.md 6d and F19, remote-server.md Section 5, architecture.md, and the README's
   "Using a server from another machine".

Owed beyond these:

- a browser login, with step 5's keys;
- per-principal rate limits;
- opening proposals from the remote window, with step 5;
- caching the origin scan behind R5 (remote-server.md Section 12).

## 11. Closure map

| Principle | Where this closes it |
|---|---|
| P5 - unknown is not absent | Sections 3.3 (refusal, not a partial union), 3.4 (an id is unknown in the grants, never called absent), 6 (failure pages, unservable workspaces shown); V4, V8 |
| P17 - knowledge sovereignty | Sections 3.3, 3.4 (no existence oracle), 3.6, 4; V3, V4, V7. This is the owed "network read tier filters enumeration by the reader's grants" |
| P18 - writes are an attack surface | Sections 2 (no writes), 3.7 (hardening, a page that holds no credential); V2, V5 |
| P20 - purity of the domain | Section 10 step 1: the viz crate takes the reader and the servable check as values, and knows nothing of the sync crate or the config file |
| P21 - a narrow surface for the LLM | The read tier is the human channel served remotely; it adds nothing to the MCP tool surface |
| P23 - gate to canon | V2: verdicts stay off the network until they can be a principal's own act |
| P24 - degrade loudly, refuse when proceeding is worse | Sections 3.1 (mounted only beside the agent surface, behind its bind rule), 6 (a failure is a page that says what happened); V1, V6, V8 |
| F19 - no unauthenticated surface | V1 |
