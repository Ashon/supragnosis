# Running a supragnosis hub in a container

Counterpart to [../systemd/README.md](../systemd/README.md) (Linux service) and
[../README.md](../README.md) (macOS). A container is the right shape for a **hub** - a node whose
job is to hold the shared log and serve the federation sync API - and the wrong shape for a personal
node, whose viewer and MCP surfaces are deliberately local-only.

```bash
docker compose -f deploy/docker/compose.yaml up -d --build   # from the repository root
```

## The two things that will bite you

**The volume is not optional.** `node.key` is generated exactly once and the `node_id` derives from
it, permanently (F14). Every peer's allowlist names that id. Lose the volume and you have not
restarted a node - you have retired one and created another that nobody admits. Measured, because it
is the kind of claim worth checking: a named volume gives the same `node_id` across `docker rm` and
recreate; no volume gives a different one on the very first restart.

**One writer, always.** redb takes a single lock on the store file. Two containers on one volume is
not a performance question, it is a corruption question. `replicas: 1` is in the compose file with a
comment on it; do not switch the update order to start-first, because that overlaps the old and new
containers on the same lock.

## What the container changes

| | Host service | Container |
|---|---|---|
| State | `~/.supragnosis` | one volume at `$HOME/.supragnosis`, where `HOME=/var/lib/supragnosis` |
| MCP (7373) | loopback, unauthenticated by design | **not published** - see below |
| Viewer socket | `~/.supragnosis/viz.sock`, 0600 | inside the volume, reachable by `docker exec` |
| Federation (7420) | binds a configured address | publish it; the daemon still demands TLS + allowlist |

**Why 7373 stays unpublished.** The MCP daemon is unauthenticated - that is not an oversight but the
local trust surface Principle 17 describes, and it is why the host service binds it to loopback.
Publishing it from a container puts an unauthenticated *write* surface on the network. If you need
MCP from elsewhere, tunnel to the host and use `docker compose exec`, or wait for the authenticated
network read tier (federation Phase 3.5).

**The viewer socket is inside the container.** Its security property is a unix socket in a 0700
directory - one principal, no port. Bind-mounting it to the host hands it to whoever can read that
path there, which may be more people than you think. Read it with
`docker compose exec supragnosis curl -s --unix-socket ~/.supragnosis/viz.sock http://l/api/graph`
and mount it out only if you have decided who else may reach it.

## Federation

The sync API is the only surface allowed to bind non-loopback, and only with TLS **and** a non-empty
allowlist. The daemon refuses rather than serving in the open, and says which half is missing:

```
Error: refusing to bind 0.0.0.0:7420: TLS is not enabled
       (F10: non-loopback needs TLS + a non-empty allowlist)
```

So a hub needs a config and a certificate mounted in - both commented into `compose.yaml`:

```toml
# supragnosis.toml
[server]
listen = "0.0.0.0:7420"          # inside the container; publish it to the host address you advertise
tls_cert = "/var/lib/supragnosis/tls/cert.pem"
tls_key  = "/var/lib/supragnosis/tls/key.pem"

[[server.allow]]
node_id = "..."                  # from `supragnosis identity` on the peer
public_key = "..."
share_workspaces = ["..."]
```

`0.0.0.0` is the bind address *inside* the container; what peers dial is the host address you map
`7420` to. Those are different strings and only the second one belongs in a peer's config.

## The two-node sandbox

`compose.yaml` above runs one hub, which is the shape a deployment wants and the wrong shape for
watching federation work. A negotiated surface (federation.md 6e) is a statement about what
**another** host admits, so a single node cannot render one, and until `compose.sandbox.yaml` there
was no way to see federation end to end without two machines.

```bash
task sandbox:up                  # builds the working tree, brings up hub + spoke
open http://127.0.0.1:7531       # the spoke's viewer - the negotiated surface is in Federation
open http://127.0.0.1:7521       # the hub's viewer - known peers, served activity
task sandbox:seed                # knowledge in both nodes, then a sync round per workspace
task sandbox:surface             # the same data as JSON
task sandbox:down                # containers and volumes, both gone
```

The two sides are configured to **disagree on purpose**: the hub admits `shared` and `hub-only`, the
spoke shares `shared` and `spoke-only`. That is what makes all three buckets appear at once -

| Panel line | Bucket | What it means |
|---|---|---|
| `shared: in sync` | both | shared here, admitted there |
| `spoke-only: not admitted` | local_only | this node lists it, the host does not admit it |
| `hub-only: admitted, not shared` | peer_only | the host would admit it, this node does not share it |
| `surface negotiated Ns ago` | - | when the answer arrived; absent reads as *unknown*, not as a revoked grant |

Symmetric configuration would show one bucket and demonstrate nothing, which is why the mismatch is
in `sandbox-init.sh` rather than left to whoever runs it.

`task sandbox:seed` then puts knowledge in both nodes and runs a sync round per workspace, so the
three buckets stop being a configuration display and become an outcome. Measured on a fresh sandbox:

| workspace | spoke shares | hub admits | round |
|---|---|---|---|
| `shared` | yes | yes | `pushed 3 pulled 1` - both sides end at 6 entities, 3 relations |
| `spoke-only` | yes | **no** | `403: workspace "spoke-only" is not shared with node ...` |
| `hub-only` | **no** | yes | `pushed 0 pulled 1` - nothing left the node, knowledge arrived |

The last row is the one worth sitting with: a workspace this node does not share still **pulls**,
because `share_workspaces` governs what leaves and the hub's allowlist governs what may be read.
That is exactly what `peer_only` was telling the operator, and the round proves it rather than
implying it.

Seeding goes over **stdio**, not the http MCP surface. The daemon's streamable-http transport keeps
its session on the connection, so `curl` cannot hold one across initialize and a tool call - the
second POST is answered "expect initialize request". stdio is the transport built for one-shot use,
so each node is stopped for the length of its own seeding and a throwaway container becomes the
store's single writer. Re-running adds nothing: identical content resolves to the same id and dedups
(F2), which was checked rather than assumed - a second run left the observation count where it was.

### It is isolated from an installed release, by construction

A developer running this most likely has the released package on the same machine, holding
`~/.supragnosis` and port 7373. Nothing in the sandbox touches either, and the separation does not
rest on remembering:

- **No bind mount into `$HOME`.** Both nodes keep state in named volumes, prefixed
  `supragnosis-sandbox-` so they cannot collide with `supragnosis-state`, which the hub compose file
  uses. `down -v` deletes the sandbox's and no other.
- **Its own compose project** (`name: supragnosis-sandbox`), so `down` here cannot reach the other
  stack, and its own image tag (`supragnosis:sandbox`).
- **A separate port block, bound to loopback.** 752x/753x, each published as `127.0.0.1:`. 7373 and
  7374-7376 (an installed daemon), 7399 (`task dev`) and 7420 (the real hub compose) are untouched.
- **MCP is not published from either node**, for the reason it is not published above: it is an
  unauthenticated local trust surface by design (P17), and a sandbox is not a reason to move one
  onto a network interface.

### What it shortcuts, and why that is not a template

`sandbox-init.sh` mounts both nodes' state at once so it can write the hub's allowlist from the
spoke's freshly generated identity. A deployment cannot do that - the peer's `node_id`, public key
and bearer hash arrive out of band, which is the exchange federation.md Section 11 defers - so the
script is a sandbox convenience and not a pattern to copy into one.

The browser proxies are the other shortcut. The viewer has no TCP bind because the unix socket's
mode is the access control; `socat` in front of it puts the viewer's write endpoints
(`/api/review`, `/api/resolve`, `/api/reify`, `/api/propose_merge`) on a port with nothing
authenticating them. That is why the real hub answers those four with `403` at the nginx layer, and
why the sandbox's published ports are bound to `127.0.0.1` rather than every interface.

## Building

The image builds from source rather than from a release tarball, so it can be cut from any commit -
which is what running a hub ahead of a release requires - and so it cannot drift if a published
asset is ever replaced.

The builder is pinned to the workspace's declared floor, `rust:1.95`. That floor is set by
dependencies rather than by this code: `supragnosis-viz` takes `oxc` as a build-dependency to minify
the viewer assets in release builds, and `rmcp-macros` brings `darling`. The `msrv` job in rust.yml
builds at the declared version, so the number here and the number in `Cargo.toml` cannot quietly
disagree.

Runtime is `debian:bookworm-slim`, not Alpine: the binary is dynamically linked against glibc and
this workspace neither builds nor tests a musl target. The image is around 120MB.

Search is keyword/hashing, as in every prebuilt binary. Semantic search needs `--features fastembed`,
which pulls the ONNX runtime and downloads a model - a deliberate build, not the default hub image.

## Operating

```bash
docker compose -f deploy/docker/compose.yaml logs -f
docker compose -f deploy/docker/compose.yaml exec supragnosis supragnosis identity   # the node_id peers must admit
docker compose -f deploy/docker/compose.yaml down                                    # keeps the volume
docker volume rm supragnosis-state                                                   # retires the node identity
```

The healthcheck asks the viewer socket for the workspace list, because `serve` being alive is not
the same as the store being readable, and a hub whose store failed to open should not report healthy.

The root filesystem is read-only and all capabilities are dropped, mirroring the hardening in the
systemd unit - a container can enforce more of it than an unprivileged user manager can. The only
writable paths are the state volume and a tmpfs at `/tmp`, which is itself a check on the claim that
all durable state lives in one place: if something needed a second location, the container would
fail rather than quietly write somewhere a backup does not cover.

`supragnosis identity` prints the node_id and **creates `node.key` if it does not exist yet** - the
one time that file is written (F14). Running it on a fresh volume is the normal way to learn the id
a peer has to admit; running it against the wrong volume mints an identity you did not want.

## As an MCP server, not a hub

The same image runs the server on stdio, which is what an MCP client launches and what the
[MCP Registry](https://modelcontextprotocol.io/registry/about) entry in `server.json` describes:

```bash
docker run -i --rm -v supragnosis-state:/var/lib/supragnosis/.supragnosis \
  ghcr.io/ashon/supragnosis:latest serve
```

`serve` overrides the image's default command, which starts the HTTP daemon a hub wants. The volume
is the same requirement as everywhere else on this page and matters more here, because a client
launches the container per session: without it, every session is a different node with an empty log.

Verified end to end rather than assumed - an `observe` in one container is found by a
`search_knowledge` in the next, and the identity survives both.
