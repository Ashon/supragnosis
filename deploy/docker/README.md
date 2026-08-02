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
