# Pairing - a second machine joins a hub (Principles 2, 5, 17, 18, 21, 24)

> How a person's second machine starts syncing with their first without editing
> `supragnosis.toml`. The hub may be a Mac or a Linux server, and so may the machine that joins;
> the scenes below use two Macs, and Section 3a says what changes when the hub is a server with no
> screen. federation.md built the sync and left admitting a peer to the operator's hand: "Admission
> additionally needs the peer's key and bearer hash to arrive from somewhere, which is an
> out-of-band exchange the console cannot perform for the operator" (Section 11). This document
> makes the person the out-of-band channel - they carry a one-time code from one screen to the
> other and allow the join on the hub - and lets the two machines do the rest.
>
> Status: **specified; steps 1, 2 and 3a built** (Section 10). The author decided its choices on
> 2026-10-22 (Section 7, D1-D15), among
> them a revision of Principle 24's bind rule (D5) and an interim key anchor in federation.md 6a
> (D11).

## 1. Why this exists

The usability audit of 2026-10-22 found one journey hard from end to end: a second machine. Today a
person who wants their MacBook to share the memory their Mac mini keeps must:

- on the Mac mini, make a TLS certificate with `openssl`, write a `[server]` section with an
  address and the certificate's paths, pick a bearer token, run `supragnosis identity
  --hash-token` with the token on the command line, and write an `[[server.allowlist]]` entry
  naming the MacBook's node id, its public key, the token's hash and the workspaces it may read -
  after asking the MacBook for its node id and key;
- on the MacBook, write a `[[sync.server]]` entry with the Mac mini's address, the token and the
  certificate's path, copy the certificate across, add the Mac mini's key to `origin_keys`, and
  list the workspaces it shares;
- restart both daemons, since neither reads those sections while running;
- and then run a sync by hand, since the daemon only checks its hubs' state on its own.

`deploy/docker/sandbox-init.sh` scripts exactly this and calls itself the shortcut "a real
deployment must not do". Settings' Sync pane says "Granting a workspace, and admitting or removing
a peer, stay in the file." Nobody who does not already know the system gets through it.

## 2. What this is NOT

- **Not discovery.** Neither machine looks for the other. The code carries the hub's addresses,
  and finding hosts the person did not name stays federation.md Section 11's deferred mesh work.
- **Not a remote profile.** A machine that uses another's memory without a node of its own is
  remote-server.md's case (a), which already has a path. Here both keep a node and sync (D1).
- **Not more than two at once, yet.** A hub keeps one joined machine, and a machine is a hub or
  joins one, never both (D6). A machine that replaces a removed one still receives what the
  removed one wrote, since the hub vouches at the join for that one's key up to its last event
  (D11). Two joined at the same time would each need keys for the other's later events, which a
  hub vouching on every round would answer, left for later.
- **Not a team's admission workflow.** Admitting another person's node, rotating a peer's key, or
  revoking what a peer already holds stay deferred (federation.md Section 11). This is one person
  and their own machines; it admits a node, not a principal.
- **Not a takeback.** Removing a machine stops what syncs from then on. What it already holds stays
  on it (P17 filters at the boundary), and every surface that removes says so.
- **Not an agent's act.** No MCP tool invites, joins or removes, and no page in the app's window can
  reach the calls that do (D9).
- **Not Mac-only.** The CLI is the implementation on every platform; Settings is the Mac's face on
  it. A Linux server makes as good a hub, often a better one, since it stays on (Section 3a).

## 3. The two Macs, scene by scene

Call them the **hub**, the machine that stays on and that others sync to, and the **joining Mac**.

1. **The hub invites.** Settings > Sync, on a Mac with no sync set up, offers "Let another Mac join
   this one". It says what that does: "This Mac will listen on every network it joins, at port
   7420, with a certificate it makes for itself. Only a Mac you invite and allow can sync with it."
   When the daemon runs as the app's own child it adds: "Turn on Start at Login so it keeps
   listening when the app is closed." The person chooses the workspaces the two keep in step -
   none chosen means nothing is shared yet - and gets a code: one line of text, valid for 10
   minutes and for one join, with Copy beside it and the time it has left. Making the code opens
   the listener; macOS may ask whether the daemon may accept incoming connections.
2. **The person carries the code** - Copy and paste through Universal Clipboard, AirDrop, a message
   to themselves.
3. **The joining Mac reads it.** Settings > Sync offers "Join another Mac". The person pastes the
   code. The Mac reaches the hub, verifying it as the code says (Section 4.3), and shows the hub's
   name and the workspaces it will receive. Beside each is "Also send", unticked: this Mac sends
   only what the person ticks. If the hub also serves people it admits (remote-server.md), it asks
   whether the hub may serve this Mac's contributions to them, unticked. The person presses Join.
4. **The hub allows it.** The hub's sheet shows "MacBook asks to join" with three short words, and
   the joining Mac shows the same three words with "Allow it on Mac mini". The person sees that
   they match and presses Allow on the hub; Deny, or a request from any other machine with the
   same code, ends the invitation.
5. **The two exchange keys.** On Allow, the hub admits the Mac with a credential of its own and
   the chosen workspaces, and answers - signed with its node key, the credential sealed to the
   joining Mac alone - with that credential, its own key, the keys it vouches for and, when it
   made its certificate itself, that certificate (Section 4.3). The joining Mac writes them down
   and restarts its daemon, which then syncs on its own (Section 4.5).
6. **Both say so.** The hub's Sync pane lists the joined Mac by the name the person allowed, with
   what it keeps in step. The joining Mac's pane lists the hub with what is ahead on either side,
   received-only workspaces included, and Sync now.
7. **Removing a Mac.** On the hub, the joined Mac's row offers "Remove this Mac", which says first:
   "It stops syncing with this Mac now. What it already received stays on it." On the joining Mac,
   the hub's row offers "Stop syncing with Mac mini", which says the same in the other direction
   and, when the hub answers, removes this Mac from the hub too. Both keep what each already holds.

## 3a. A server as the hub

A Linux server, reached over SSH and with no Settings page, is a hub through the CLI:

- **The invitation.** `supragnosis invite --keep acme,supragnosis --address hub.example.net`
  prints the code and its time left, and waits. When a machine asks to join, it prints the
  machine's name and the three words and asks "Allow? [y/N]"; only `y` admits it. If the command
  ends first - the SSH session drops, Ctrl-C - the invitation stays open until it expires, and
  `supragnosis invite --show` prints it and any waiting request, and asks again.
- **The address.** `--address` names how the joining machine reaches the server - a DNS name, a
  Tailscale or VPN name, an address - and may be repeated. Without it the code carries the host's
  `.local` name where mDNS answers for it and the IPv4 address its default route leaves from.
- **The certificate.** A server often has a certificate of its own from a public CA, renewed every
  few months. When that certificate verifies against the root bundle built into supragnosis for
  every name `--address` gives, the code says to verify the hub by name, so a renewal breaks
  nothing; any other certificate - one the hub made, a private CA's, one self-signed by hand - is
  pinned by its fingerprint (D13). An IP address is never verified by name.
- **Staying up.** The daemon is the systemd user unit, which stops with the person's last session
  unless lingering is on (daemon-lifecycle.md, Linux). The invitation says so when it is off.
- **The firewall.** An unreachable hub's message names the platform's firewall - the macOS
  firewall, or the server's (ufw, firewalld) and the network's.

A joining machine on Linux runs `supragnosis join`, reading the code on stdin, and shows the three
words in the terminal while it waits for the hub's answer.

## 4. The mechanism

The CLI stays the implementation (settings-page.md Section 6): every scene is a command the
Settings page calls, and each works from a terminal on its own.

**The admin socket.** What changes the running hub - opening the listener, inviting, allowing,
removing, telling it a principal changed, starting a sync round - is a call on the daemon's admin
socket, `~/.supragnosis/admin.sock`, mode `0600` in the owner-only directory. Only this user's
processes reach it, so it needs no token and `SUPRAGNOSIS_MCP_AUTH=off` does not open it. The
bridge relays only `/mcp` and the app's proxy only the viewer socket's reads, so neither reaches
it; and none of its calls is an MCP tool, so no agent can mint a code and read it from a tool
result (P21, P17). On the network listener only `preview`, `join` and `leave` are new: the first
two require the code's secret, the last a joined machine's credential.

### 4.1 The listener: up only while something is admitted

- **At start**, any reason the listener cannot open - no TLS, nothing admitted, the port taken, a
  certificate it cannot read - no longer stops the daemon. The node starts without the listener
  and says why in `sync_status`'s `config_notes` and the Sync pane, as Principle 24's first bullet
  asks. Today the daemon exits, and launchd restarts it into the same exit. The hub's wiring - the
  peer directory, the principals' router and the network's pairing calls - is built at start when
  `[server]` exists and by the first invitation when it does not, so that invitation needs no
  restart. What reports on it is there from the start: the admin socket, the event channel, and
  the listener's state, which `sync_status` reads on a node with no sync as well. A node with no
  `supragnosis.toml` makes no node key until it invites.
- **At run time**, the daemon opens the listener when the first admission arrives - an invitation,
  a joined machine, a principal - and closes it when the last one ends, an unused invitation
  expiring among them. The bind rule is checked each time it would open (D5, PA1). `principal add`
  and `principal remove` tell the daemon through the admin socket after they edit the file.
- **The certificate it makes for itself.** The first invitation on a hub with no certificate
  makes an ECDSA P-256 key and a self-signed end-entity certificate (not a CA) naming the host's
  `.local` name, the `--address` names and `localhost`, valid for ten years, written under
  `~/.supragnosis/hub/` - the key `0600` - and writes `[server] listen = "0.0.0.0:7420"`,
  `tls_cert` and `tls_key` into `supragnosis.toml` (Section 4.4). A `[server]` already present
  keeps what it names, the operator's own certificate included.
- **Where it listens.** `0.0.0.0` is every IPv4 network the host joins. On a Mac the listener is
  the daemon Homebrew installs, so a macOS firewall that asks may ask again after an upgrade; an
  unreachable hub's message names the platform's firewall.

### 4.2 The invitation: `supragnosis invite`

- `supragnosis invite --keep <ws>,... [--address <name>]... --json` asks the daemon for an
  invitation and prints the code; the Settings page polls the admin socket for the join request
  rather than waiting on the command.
- **Who may invite** (D6): not a machine that syncs to a hub, and not a hub that already admits a
  node, joined or written by hand. Each is refused with what it is: "This Mac already syncs with
  MacBook. In this version a Mac syncs with one other." Removing that one lifts it.
- **The secret.** 32 random bytes; the daemon keeps their hash, never the secret, with the
  workspaces chosen and the expiry - and, once a machine asks, its request and later the signed
  answer (Section 4.3) - in `~/.supragnosis/hub/invites.toml` (`0600`), not in `supragnosis.toml`,
  which the person edits. A used, denied, cancelled or expired invitation stays there as a
  tombstone for a day after its expiry, so a code tried late is told "This code was already used."
  or "This code has expired." rather than "no such code".
- **One open at a time.** A new invitation cancels the open one, and the sheet says so.
  `supragnosis invite --cancel` ends the open one; a cancelled invitation stays a tombstone, as a
  used one does.
- **The code.** `sgjoin1-` and a base32 body (Section 4.3.2) holding the hub's node id; the
  expiry; the secret; how to verify its certificate - its SHA-256 fingerprint, or "by name"
  (Section 3a, D13); and its addresses with the port (the `--address` names, or its `.local` name
  and the IPv4 address of its default route). Every address is reached over HTTPS. The page's
  Copy writes the pasteboard through NSPasteboard and marks the item concealed, so clipboard
  managers that honor it do not keep it.
- **Expiry is wall-clock** - an operational timer like a login prompt's, not a fact any fold
  reads, so it does not touch P16 or I4.

### 4.3 Joining: `supragnosis join` and the hub's `preview` and `join` calls

- **The code arrives on stdin** (or the Settings page's field), never as an argument, so it never
  sits in a process list (settings-page.md S2).
- **Who may join** (D6): not a machine that already syncs to a hub, by `[[sync.server]]` or the
  older `servers` key, and not one that admits a node of its own. Each is refused with what it is.
- **It connects** to each address in turn. With a fingerprint it accepts exactly the certificate
  whose SHA-256 is the code's, and a different one ends the attempt with "This is not the machine
  that made the code."; the pin replaces the hostname check, so an address that changed since the
  code was made still works if the certificate is the same. "By name" verifies the certificate
  against the built-in root bundle and the address's name, and an address that fails tries the
  next.
- **`preview`**, with the secret, answers the hub's name, the workspaces the invitation keeps in
  step and whether the hub serves principals, and changes nothing.
- **`join`** carries the secret, the joining machine's node public key and name, a one-time public
  key made for this join, and a commitment to a random nonce of its own. The hub:
  - checks the secret's hash against an open invitation in constant time, and refuses an expired,
    used or denied one with what happened;
  - derives the node id from the node key, as core derives every node id, rather than taking
    one, and refuses its own and one already admitted;
  - checks the keys' form and cuts the name to 64 characters, shown as text;
  - holds the request and answers "pending" at once with a random nonce of its own, so no call
    waits on a person. The request's keys, its commitment and the hub's nonce are fixed by the
    first request and kept in `invites.toml`, so a restart of the hub keeps them. The joining
    machine asks again with its nonce revealed, and the hub, once the nonce matches the
    commitment, shows the request with the three words on its Sync pane, in `invite`'s terminal
    and through the admin socket. The joining machine keeps asking with the same secret, keys,
    commitment and nonce until it has an answer; a repeat with all of them the same is the same
    request. A request under the same code with other keys or another commitment, or a nonce that
    does not match the commitment, ends the invitation, and both are told;
  - **on Allow**, mints a 32-byte bearer credential and adds the allowlist entry - the node id, the
    key, the credential's hash and the invitation's workspaces - and records it in
    `~/.supragnosis/hub/peers.toml` with the name, as an entry pairing made; swaps its live peer
    directory so the entry admits at once; and keeps the answer for that request in
    `invites.toml` until the invitation's expiry, so neither a dropped connection nor a restart
    after Allow loses it - only the one-time key opens it;
  - **answers**, signed with its node key: the credential sealed to the one-time key, its node
    public key, the vouched keys (Section 4.3.1), the workspaces, and, for a pinned certificate,
    the certificate in PEM;
  - on Deny or expiry, answers that, and the invitation becomes a tombstone.
- **The three words** come from the secret, the joining machine's node key and one-time key, and
  both nonces, the same on both screens. Someone else holding the code makes a request the hub
  shows with words the person never saw on their own machine, and only the hub's Allow admits
  anyone. A party in the middle of a join verified by name, which sees the joining machine's
  request, still cannot choose keys that make the hub show the person's words: the hub fixes its
  nonce at the first request, the joining machine reveals the nonce that decides the words only
  after it has the hub's, and it ends its join if the hub's nonce ever changes. The party gets one
  guess in 2^24 (about 17 million), and a second request ends the invitation.
- **The joining machine** checks the answer's signature against the hub key the code's node id
  names, that it answers this request, a pinned PEM's fingerprint against the code's, and opens
  the credential with its one-time key. A party in the middle, even one holding a valid
  certificate for the name, can read neither the credential nor forge the answer. `preview` is not
  signed, so what the person ticked counts only where the signed answer agrees: a ticked
  workspace the answer does not name is not sent, and the serve question's yes is written only
  when the answer says the hub serves principals. Then it writes, under the lock of Section 4.4:
  - in `supragnosis.toml`, only keys a release before pairing knows: a `[[sync.server]]` entry
    with the address it verified (a `.local` name or DNS name rather than an address DHCP may
    change), the credential, and for a pinned certificate `ca` pointing at it saved as
    `~/.supragnosis/hubs/<hub's node id>.pem`; the hub's key in `origin_keys`; the ticked
    workspaces in `share_workspaces`, and in `serve_workspaces` if the person allowed it;
  - in `~/.supragnosis/hubs/<hub's node id>.toml` (`0600`): the hub's name, every invited
    workspace as the receive list (D12), and the vouched keys with their bounds.

  It sets `supragnosis.toml` to `0600` wherever `SUPRAGNOSIS_CONFIG` puts it, and restarts its
  daemon, since `[sync]` is read at start. If the restart is refused (daemon-lifecycle.md L1), it
  says "Joined. Restart Supragnosis to start syncing." and the row says so until it has.
- **The credential is never shown.** It goes, sealed, from the hub's answer to the joining
  machine's config file; the hub keeps only its hash.

#### 4.3.1 The vouched keys

- **Which.** The hub vouches for the origins whose events it stores in the invited workspaces,
  other than itself and the joining machine: in this version those are removed peers', since one
  joined machine is admitted at a time. A removed peer's key and the last sequence the hub stored
  from it stay in `peers.toml` as verify-only: they admit nothing, and they verify events up to
  that sequence and no later, so a removed peer still cannot add events through anyone
  (federation.md 6a).
- **Checked.** The joining machine accepts a vouched key only if its node id is the one derived
  from it, skips its own id, and refuses the answer if a vouched key conflicts with one written by
  hand.
- **Bounded.** The joining machine verifies events from a vouched origin only up to its bound;
  later ones are refused as from an unknown origin, as today.

#### 4.3.2 The wire

What both sides compute is fixed by the code's `sgjoin1` prefix. A change to any of it is a new
prefix, which a release that does not know it refuses by name.

Every hash this section derives is BLAKE3's `derive_key` under a context string of its own, so no
two of them can be confused; the secret, the nonces and every key are 32 bytes.

- **The code's body** holds, in this order, the hub's node id (16 bytes), the expiry (seconds,
  8 bytes), the secret, the certificate check (a tag, then a SHA-256 fingerprint or nothing for
  "by name") and the addresses with their ports (a count, then each with its length), then the
  first 4 bytes of a hash of all that as a checksum. It is written in lowercase base32 without
  padding. A code cut short or changed on the way is refused - "This code is incomplete or
  changed. Copy it again." - before any connection is made.
- **The commitment** is a hash of the node key, the one-time key and the joining machine's nonce.
- **The words** are three from a fixed list of 256 in the source, one for each of the
  first three bytes of a hash of the secret, the node key, the one-time key and both nonces.
- **The seal.** The hub makes an X25519 key for each join, derives the seal's key by hashing the
  shared secret and both public keys, and seals the credential with ChaCha20-Poly1305 under an
  all-zero nonce, which a key used once allows. A shared secret of all zeros - a one-time key of
  small order - seals and opens nothing. X25519 comes from curve25519-dalek, whose keys can be
  saved, unlike ring's; ChaCha20-Poly1305 and the certificate's SHA-256 come from ring. Both are
  already in the build through ed25519-dalek and rustls, and become direct dependencies of the
  CLI; the built-in root bundle is webpki-roots, already there through reqwest.
- **The signature.** The hub signs the answer's bytes with its node key under a context no
  attestation uses: core's message signing puts a fixed tag before the context, and an
  attestation's bytes can never start with it, so neither passes for the other. The signed bytes
  name the invitation (its secret's hash), the joining node id, its one-time key and both nonces,
  so an answer to one request, or one invitation, does not verify for another; and whether the
  hub serves principals, so the serve question rests on a signed word.
- **The one-time key** stays on the joining machine, with its nonce and the hub's, in
  `~/.supragnosis/hubs/joining.toml` (`0600`) until the join ends or the code expires, so `join`
  run again with the same code asks as the same request rather than ending the invitation. The
  file names the code it belongs to, so another code starts afresh. The hub's nonce is written
  there before the joining machine reveals its own or shows the words, so a run again that finds
  no kept hub nonce has revealed nothing. A hub nonce other than the one kept ends the join, and
  the person makes a new code.
- **Known answers** pin the code, the words, the commitment and core's message signing bytes for
  fixed inputs, as compatibility.md Section 4 does for the content id.

### 4.4 Editing `supragnosis.toml`

Every writer - the join on either side, a removal, the first invitation's `[server]` lines, the
narrowing act, `principal add` and `remove` - edits the file in place with the format-preserving
editor the narrowing act uses, keeping every line the person wrote, under one lock file beside it,
so two edits never lose each other's change. An edit adds only what the person chose in the act at
hand, and removes only what pairing itself recorded: an entry written by hand is never removed or
widened by these acts. Pairing writes no key a release before it does not know (D15).

### 4.5 The daemon syncs on its own

- **A round on its own.** A node with a hub runs a sync round - push, then pull - five minutes
  after its last, and once soon after it starts. A failed round waits longer before the next, up
  to an hour, and `sync_status` says when the last round ran, what it moved and why the last one
  failed (P24, P5).
- **One round at a time**, whatever starts it: the timer, Sync now, or the `sync_push` and
  `sync_pull` tools. Its store work runs off the async threads, as the tools' does (F11).
- **What it covers.** The workspaces in `share_workspaces`, each pushed and then pulled. Where a
  join recorded a receive list, the round and the `sync_pull` tool also pull those, and only
  those, whatever the hub's answer admits (PA12). A workspace the hub merely admits is not pulled
  on its own, so a hub's answer never widens what the node asks for (federation.md F21).
- **Sync now** in the Sync pane starts a round through the admin socket and the pane polls for its
  result. `supragnosis sync` does the same when a daemon runs, and runs a round itself, as now,
  when none does.
- **What a round does is what `sync_push` and `sync_pull` do** today: the same protocol, the same
  checks, the same convergence (federation.md Section 5). Only the trigger is new.

### 4.6 Removing: `supragnosis peer remove` and `supragnosis hub leave`

- `supragnosis peer remove <name>` on the hub asks the daemon to delete an entry pairing made,
  keeping its key verify-only with its last sequence (Section 4.3.1), and swap the live directory:
  the next request from that machine is refused. It closes the listener if nothing else is
  admitted. An entry written by hand stays in the file, and the act says so.
- `supragnosis hub leave` on the joining machine first asks the hub, with its credential, to
  remove its own entry - the `leave` call, which the hub accepts only for the machine the
  credential names - then removes the `[[sync.server]]` entry, the keys and the sidecar it wrote,
  and restarts its daemon. A hub that does not answer keeps the row until it is removed there, and
  the joining machine says so.
- Each says, before it acts, that what was synced stays.

## 5. Settings

The Sync pane (settings-page.md Section 3.4) gains its acts; on a remote profile it is not shown,
as now (S6).

- **Without sync set up**, it offers "Let another Mac join this one" and "Join another Mac", in
  place of the pointer to the file.
- **On a hub**, the invitation sheet: the workspaces as checkboxes, none checked; Make code; the
  code with Copy and its time left; then the request - the joining machine's name, the three
  words, Allow and Deny; and "MacBook joined." Closing the sheet leaves the invitation open until
  it expires, and the Sync pane shows a waiting request. The joined machine's row offers Remove
  this Mac.
- **On a joining Mac**, the join sheet: the code field; on paste, the hub's name, the workspaces it
  will receive each with "Also send" unticked, and the serve question when it applies; Join; then
  the three words and "Allow it on Mac mini" until the hub answers. A wrong, expired, used or
  denied code, an unreachable hub (naming the firewall and the network) and a certificate that
  does not match each says what it is and what to do.
- The hub's row on a joining Mac offers Sync now and says when the last round ran.
- The hint "Granting a workspace, and admitting or removing a peer, stay in the file." becomes
  "Granting a joined Mac another workspace stays in the file." Widening stays a deliberate edit
  (federation.md 6a); joining and removing do not.

## 6. Security

| Threat | What holds |
|---|---|
| Someone on the network reaches the listener | TLS, and nothing answered without an admitted node, an admitted principal, or an open invitation's secret for `preview` and `join` (PA1) |
| Someone copies the code | Only the hub's Allow admits anyone, beside three words the person compares with their own machine's; a request from other keys ends the invitation; ten minutes, one use (PA2, PA11) |
| A machine pretends to be the hub | A pinned certificate is accepted from any address and no other; a certificate verified by name needs a public CA's certificate for that name, and even then the answer must be signed by the node key the code names (PA3, PA14) |
| A party in the middle reads the join | The credential is sealed to the joining machine's one-time key, and the answer is signed by the hub's node key (PA14); it cannot make the hub show the person's words for keys of its own, since they depend on a nonce the joining machine reveals only after the hub has fixed its own (PA11) |
| After the join, a link verified by name | It trusts the public CAs for that name, as any HTTPS does: someone with a valid certificate for it could read what the credential grants, and still could not forge events (P2, F6) |
| A joining machine claims another node's identity | The hub derives the node id from the key and refuses its own and an admitted one (PA8) |
| A machine pretends to be the joined one later | It has no credential: the bearer was minted at the join and never shown, and its events must be signed by the admitted key (P2, F6, PA4) |
| The joined machine sends what it was not granted | The hub accepts only the workspaces the entry names (F9, 6c) and evaluates its writes' tiers itself (P18) |
| The hub's answer widens what the joined machine pulls | It pulls only the receive list fixed at the join (PA12) |
| A vouched key is wrong or reused | The hub vouches in a signed answer, each key checked against its node id and bounded by the last event the hub stored; a vouched key verifies events and admits nothing (PA13) |
| The code is guessed | 256 bits of secret, checked in constant time; the hub keeps only its hash |
| An agent or a page opens the hub | Hub acts are calls on a `0600` admin socket, never MCP tools, never reachable through the bridge or the app's proxy (PA10) |
| A browser on the network reaches the listener | Nothing answers it without a secret or a credential; `preview` tells a code holder only what `join` would grant |

## 7. Decisions

- **D1 - Sync, not a remote profile** (the author, 2026-10-22). Both keep a node, so a laptop away
  from home still records and converges when it is back.
- **D2 - A hub with no certificate makes one, and the code pins it** (the author). No person
  handles a certificate.
- **D3 - A one-time code** (the author). Ten minutes, one join, carried by the person; the
  long-lived credential never appears on a screen.
- **D4 - Only what is chosen is shared** (the author). Nothing is checked to start with, on either
  side, as P17 asks of sharing.
- **D5 - An open invitation counts as an admission for the bind rule** (the author). Principle
  24's enforcement bullet is revised to "no node, no principal and no open invitation": the person
  made the invitation a moment ago, and it admits only its holder's `preview` and `join`. The
  rule's purpose stands - no surface the person did not authorize.
- **D6 - One joined machine per hub, and a machine is a hub or joins one** (the author). Two joined
  at once, or a chain, need keys vouched for on every round.
- **D7 - The invitation chooses the workspaces once** (the author). The hub's entry grants
  receiving and sending together; the joining machine receives them all and sends only what it
  ticks (D12).
- **D8 - The daemon syncs on its own, and Sync now** (the author). Without it "converges when it is
  back" would need a person each time.
- **D9 - Hub acts go through a `0600` admin socket** (the author). Not the MCP listener, whose
  token sits in clients' configs and which `SUPRAGNOSIS_MCP_AUTH=off` leaves open, and never an MCP
  tool.
- **D10 - The hub's Allow completes a join** (the author). A copied code then buys nothing without
  the person's press on the hub, beside words they compare.
- **D11 - The hub vouches at the join for keys it has verified** (the author). A machine that
  replaces a removed one receives the old one's events up to its last, and nothing it might send
  later. This is an interim anchor beside federation.md 6a's three, which fills a spoke's
  `origin_keys` until the log-borne policy that anchor 2 describes is built.
- **D12 - The joining machine sends nothing it did not tick** (the author). It receives the invited
  workspaces, recorded as its receive list, which settles D4 and D7 on that side.
- **D13 - A hub may be a Linux server, with its own certificate** (the author asked that a server
  work as the hub). The CLI carries every scene, with Allow in the terminal; a certificate that
  verifies against the built-in root bundle for the code's names is verified by name, since
  pinning it would break at its renewal, and any other is pinned.
- **D14 - The join is signed and sealed** (the author). The hub signs its answer with its node key
  and seals the credential to a one-time key of the joining machine's, so verifying a hub by name
  does not let a party in the middle take the join. *Kept, 2026-10-22, when step 3 was split:* its
  aim also needs the words' commitment (Section 4.3), since without it a party in the middle that
  sees the request could have the hub show the person's words for its own one-time key and take
  the credential on their Allow.
- **D15 - Pairing's own records live beside `supragnosis.toml`** (the author). The receive list,
  the hub's name and the vouched keys go in `~/.supragnosis/hubs/`, beside the hub's pinned
  certificate and, while a join is open, its one-time key, so a release before pairing, which
  refuses keys it does not know, still starts and syncs.
- **Invitations are not configuration.** They live beside `supragnosis.toml`, not in it, since a
  file the person edits should not hold a ten-minute secret.
- **Widening stays in the file.** Granting a joined machine another workspace is the act
  federation.md 6a keeps deliberate; the console still only narrows.

## 8. Invariants

| # | Invariant |
|---|---|
| PA1 | The hub's listener is open only with TLS and an admitted node, an admitted principal or an open invitation; an open invitation admits only its holder's `preview` and `join`; a listener that cannot open leaves the rest of the node serving and says why. |
| PA2 | An invitation is used at most once and not after its expiry; the hub keeps only its secret's hash, and a used, denied, cancelled or expired one is said as such for a day after its expiry. |
| PA3 | The joining machine accepts only the certificate the code names - by fingerprint, or by the built-in root bundle and the code's DNS names when the code says so - during the join and after it. |
| PA4 | The bearer credential a join mints is never displayed: it travels sealed in the join's answer to a `0600` file, and the hub keeps only its hash. |
| PA5 | A join grants exactly the invitation's workspaces; the joining machine sends only the ones it ticked; none chosen shares nothing (P17). |
| PA6 | Every edit of `supragnosis.toml` keeps every line the person wrote and runs under one lock; it adds only what the act chose, removes only what pairing recorded, and writes no key an earlier release refuses. |
| PA7 | Removing a machine refuses its next request and takes back nothing it holds, and the surface that removes says so before it acts. |
| PA8 | The hub derives a joining machine's node id from its key, and refuses its own and one already admitted. |
| PA9 | A hub admits one joined machine; a hub, or a machine admitting any node by hand, does not join one; a joined machine does not invite - in this version. |
| PA10 | No MCP tool and no page reaches a hub act: they are calls on a `0600` admin socket. |
| PA11 | A join completes only on the hub's Allow, for the keys whose request the hub showed with words drawn from both sides' nonces; a request under the same code with other keys or another commitment, or a nonce that does not match the commitment, ends the invitation. |
| PA12 | A joined machine pulls only the workspaces recorded at the join. |
| PA13 | A vouched key is accepted only for the node id derived from it, never for the joining machine's own, and verifies events only up to the bound the hub gave; it admits nothing. |
| PA14 | The joining machine acts on a join's answer only if it is signed by the node key the code names, and the credential in it is sealed to the one-time key it sent. |
| PA15 | A node with a hub runs a sync round on its own, over its shared workspaces, one at a time whatever starts it, and says when the last ran, what it moved and why it failed. |

## 9. What this revises

- principles.md Principle 24: the bind rule's "no node and no principal" gains "and no open
  invitation", and the listener refuses, not the node (D5).
- federation.md 6a and F10: an open invitation counts as an admission; a listener that cannot open
  leaves the node serving; the key anchors gain an interim fourth, a hub vouching once at a join,
  bounded (D11); a removed peer's key is kept verify-only up to its last sequence, which keeps
  6a's "a removed peer cannot have its relayed events verified" for every later event.
- federation.md Section 11: admitting a person's own second machine leaves the deferred list; a
  team's admission, key rotation and vouching on every round stay.
- federation.md F21 clause (2): revised - where a join recorded a receive list, a peer's answer
  never extends what this node pulls either.
- federation.md Section 9: the daemon runs sync rounds on its own (Section 4.5), and `supragnosis
  sync` runs one through it when it runs.
- remote-server.md R1, R2 and Section 4.1: the same bind rule; `preview` and `join` come from
  neither a principal nor a node but an invitation's secret, and `leave` from a joined machine's
  credential; a listener that cannot start leaves the rest of the daemon serving.
- settings-page.md Section 3.4: the Sync pane offers invite, allow, join, remove, stop and Sync
  now; widening stays in the file. S3 holds: each act is a CLI call, and the CLI asks the daemon.
- daemon-lifecycle.md L6 holds: a hub that runs as the app's child stops when the app quits, and
  the invitation says so; on Linux the invitation says when lingering is off.
- onboarding.md O6 holds: setup configures no sync. Step 5 below lets Setup on a second Mac point
  to "Join another Mac", which is a link, not a setting.
- compatibility.md: no key in `supragnosis.toml` is new (D15); a release before pairing ignores the
  sidecar files, starts, and syncs without the receive list's cap or the vouched keys. Section 4
  gains the join's wire and core's message signing bytes, which cross machines too.

## 10. Ordering

1. **The daemon syncs on its own** [built] (Section 4.5): the round, its timer and backoff, one at a time
   with the tools, and `sync_status`'s `last_round`. Guarded by a two-node case in which a write on
   one reaches the other with no call.
2. **The admin socket and the listener's life** [built] (Sections 4 and 4.1): the admin socket, `supragnosis
   sync` through the daemon, no exit at start for any listener failure, the listener opened and
   closed at run time as the bind rule allows, principals telling the daemon. Guarded by a case
   that walks one listener through an unreadable certificate, loopback and an unadmitted network
   address, and one that holds the admin socket to its owner.
3. **The CLI**, on macOS and Linux alike, in four parts:
   - **3a. The hub** [built]. The wiring on a node with no `[server]`, built by its first invitation; the
     certificate it makes (rcgen moves from a test dependency to the CLI's, already in the lock
     file and checked by cargo-deny), and the code's check - pinned, or by name when the
     certificate verifies against the built-in roots for every `--address` name, never for an IP;
     the bind rule counting an open invitation, and the listener closed when the last admission
     ends, an invitation's expiry among them; `invite` with `--address`, `--show`, `--cancel`,
     `--json`, the terminal's Allow and, on Linux, the lingering note; the admin calls that
     invite, allow, deny and cancel; the network's `preview`, `join` and `leave` with "pending",
     the commitment, the three words and the signed and sealed answer; `peers.toml`, the vouched
     keys with their bounds, and `peer remove`; the locked format-preserving edits, the narrowing
     act's and `principal`'s among them; the wire's known answers; the event channel and the
     listener's state there from the start, `sync_status` reading the state on a node with no
     sync. Guarded by cases in which the
     test plays the joining machine: an expired, a reused, a denied, a cancelled, a wrong and a
     changed code, a second request, another commitment, a nonce that does not match, a claimed
     identity, a hub that may not invite, an answer that opens only with its one-time key and
     verifies only under the hub's key and for its request, the request and the answer kept
     across a restart, the listener opening for an invitation and closing at its expiry and its
     state read by `sync_status` on a node that had no `supragnosis.toml`, and a removal refusing
     the next request.
   - **3b. The joining machine.** `join` reading stdin; the certificate checked by fingerprint or
     by name; `preview`; the commitment, `joining.toml` and the words in the terminal; the answer
     verified and opened, ticks bounded by it; the config and sidecar writes, `0600`, and the
     restart; the receive list applied to rounds and to `sync_pull`; `hub leave`; an unreachable
     hub naming the platform's firewall. Guarded by a two-node case that invites, allows, joins
     and syncs a workspace each way with no call, and cases for a wrong certificate, a tampered
     answer, a changed hub nonce, `join` run again, a ticked workspace the answer does not name
     and a serve yes the answer does not allow, neither written, a chain and leaving.
   - **3c. The vouched keys** on the joining machine: accepted only for the id derived from each,
     its own skipped, an answer refused when one conflicts with a key written by hand, and each
     verifying events only up to its bound on the apply path. Guarded by those three refusals and
     a replaced machine receiving the removed one's events up to the bound and none after.
   - **3d. A Linux hub** in a throwaway machine, invited over SSH with Allow in the terminal and
     joined from this Mac, with the run recorded in the pull request.
4. **Settings**: the invitation and join sheets with Allow, the rows' Remove, Stop syncing and Sync
   now, and the hint. `invite --json`, `invite --show --json` and `peer list --json`, which the
   sheets read, join compatibility.md Section 6 then, with their schema and examples.
5. **Setup** on a second Mac points to Join another Mac.

### 10.1 What splitting step 3 checked

| Against | Finding |
|---|---|
| P2 - provenance | Agrees. The join answer is signed under a context of its own, so it never passes as an attestation's signature (Section 4.3.2). |
| P5 - absence is not negation | Agrees. A code cut short or changed is said before any connection is made. |
| P17 - sharing, P18 - writes are an attack surface | Repays two gaps. The words drew only on the secret and the node key, so a party in the middle of a join verified by name, which sees the request, could have made the hub show the person's words for a one-time key of its own and, on Allow, read what the credential grants; the commitment, with both nonces fixed once, closes it (Section 4.3, PA11). And `preview` is unsigned, so a party in the middle could have offered workspaces the hub never invited for the person to tick and send to it; ticks now count only where the signed answer agrees (Section 4.3). |
| P21 - a narrow surface for the LLM | Agrees. Nothing new is an MCP tool. |
| P24 - degrade loudly | Agrees. The listener closes when an unused invitation expires and nothing else is admitted. |
| Decision D14 | Kept, with a dated note: its aim needs the words' commitment, which the decision did not name. |
| Section 4.1 | Revised to match step 3: with no `[server]`, the hub's wiring is built by the first invitation; what reports on it - the admin socket, the event channel, the listener's state - is there from the start, so P24's first bullet holds for a hub made that way. |
| compatibility.md Section 4 | Extended: the join's wire and core's message signing bytes cross machines too, fixed by the code's prefix and pinned by known answers. |
| Contradicts | None found. |
| Owed | None new. |

## 11. Closure map

| Principle | Where it holds here |
|---|---|
| P2 - provenance | The joined machine's id is derived from the key it signs with; vouched keys only verify, within their bounds; the answer is signed by the hub's node key (PA8, PA13, PA14) |
| P5 - absence is not negation | A wrong, expired, used or denied code, an unreachable hub and a certificate mismatch are each said; a listener that cannot open says why (PA1, PA2) |
| P17 - sharing | Only chosen workspaces; the joining machine sends only what it ticks and pulls only what it recorded; none chosen shares nothing; removal is not a takeback (PA5, PA7, PA12) |
| P18 - writes are an attack surface | The hub evaluates the joined machine's tiers; a copied code needs the hub's Allow; the credential is never shown (PA4, PA11) |
| P21 - a narrow surface for the LLM | No hub act is an MCP tool (PA10) |
| P24 - degrade loudly, refuse when proceeding is worse | The listener opens only with TLS and an admission, an invitation counted as one; when it cannot open, the node keeps serving and says why (PA1) |
