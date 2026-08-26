#!/bin/sh
# Put knowledge into the two-node sandbox, then sync it, so the surface has something to be about.
#
#   task sandbox:seed        # after `task sandbox:up`
#
# Why not MCP over http: the daemon's streamable-http transport keeps its session on the connection,
# and `curl` cannot hold one across three requests - a second POST is answered "expect initialize
# request". Seeding therefore goes over **stdio**, the transport built for one-shot use, in a
# throwaway container against the same volume. That needs the store's single writer to be the
# throwaway rather than the daemon, so each node is stopped for the length of its own seeding.
#
# Idempotent by content address: an observation with identical content resolves to the same id and
# dedups (P14/F2), so running this twice adds nothing. The sync round is idempotent for the same
# reason (F7).
set -eu

CF=deploy/docker/compose.sandbox.yaml
IMG=supragnosis:sandbox
dc() { docker compose -f "$CF" "$@"; }

# The three workspaces exercise the three outcomes the negotiated surface predicts:
#   shared      - the spoke shares it, the hub admits it   -> pushes and pulls
#   spoke-only  - the spoke shares it, the hub does NOT    -> refused at the door, loudly
#   hub-only    - the hub admits it, the spoke does not share it -> pulls, pushes nothing
seed() { # seed <volume> <host-label> ; JSON-RPC lines on stdin
  # The host label has to match the daemon's, or the observations this writes report `localhost`
  # while the same node's daemon reports its own name - one node speaking under two labels. `host` is
  # self-declared either way (P2); this only keeps the label consistent.
  docker run --rm -i -e "SUPRAGNOSIS_HOST=$2" -v "$1:/var/lib/supragnosis/.supragnosis" "$IMG" \
    serve 2>/dev/null | grep -c observation_id || true
}

hello() {
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"sandbox-seed","version":"0"}}}' \
                '{"jsonrpc":"2.0","method":"notifications/initialized"}'
}
call() { # call <id> <arguments-json>
  printf '{"jsonrpc":"2.0","id":%s,"method":"tools/call","params":{"name":"observe","arguments":%s}}\n' "$1" "$2"
}

echo "stopping both daemons - the throwaway container has to be the store's single writer"
dc stop spoke hub >/dev/null 2>&1

echo "seeding the spoke"
{
  hello
  call 2 '{"workspace":"shared","content":"redb replaced the Datalog store in v0.2.0 and the knowledge model did not move","entities":[{"name":"redb","type":"Tool","description":"embedded pure-Rust B-tree store"},{"name":"KnowledgeStore port","type":"Concept","description":"the trait every adapter implements"}],"relations":[{"from":"redb","type":"implements","to":"KnowledgeStore port"}],"confidence":0.9}'
  call 3 '{"workspace":"shared","content":"the negotiated surface is what a host says this node may reach","entities":[{"name":"negotiated surface","type":"Concept","description":"a peer entitlement, per link"},{"name":"ping","type":"Tool","description":"the sync health check that carries it"}],"relations":[{"from":"ping","type":"carries","to":"negotiated surface"}],"confidence":0.95}'
  call 4 '{"workspace":"spoke-only","content":"this spoke keeps a note the hub never admits","entities":[{"name":"local note","type":"Concept","description":"deliberately unshared"}],"confidence":0.5}'
  call 5 '{"workspace":"shared","content":"the spoke reads the sync protocol as three idempotent calls","entities":[{"name":"sync protocol","type":"Concept","description":"advertise, pull, push"},{"name":"version vector","type":"Concept","description":"per (origin, workspace) high-water marks"}],"relations":[{"from":"sync protocol","type":"uses","to":"version vector"}],"confidence":0.8}'
} | seed supragnosis-sandbox-spoke sandbox-spoke

echo "seeding the hub"
{
  hello
  call 2 '{"workspace":"shared","content":"federation replicates the observation log, never a projection","entities":[{"name":"observation log","type":"Concept","description":"the single source of truth"},{"name":"federation","type":"Concept","description":"log replication across hosts"}],"relations":[{"from":"federation","type":"replicates","to":"observation log"}],"confidence":0.95}'
  call 3 '{"workspace":"hub-only","content":"the hub tracks a certificate rotation no spoke shares","entities":[{"name":"TLS certificate","type":"Concept","description":"the hub self-signed cert"}],"confidence":0.6}'
  # The same two entities the spoke asserted, from the hub's side. Co-assertion is the case the
  # origin layer exists for: after a round both nodes have attested them, so those entities sit
  # inside BOTH origin outlines and the overlap is the fact on screen (F4). Without a co-asserted
  # entity the layer draws two disjoint regions and demonstrates nothing it was built for.
  call 4 '{"workspace":"shared","content":"the hub serves the sync protocol and holds the authoritative version vector","entities":[{"name":"sync protocol","type":"Concept","description":"advertise, pull, push"},{"name":"version vector","type":"Concept","description":"per (origin, workspace) high-water marks"}],"relations":[{"from":"sync protocol","type":"uses","to":"version vector"}],"confidence":0.85}'
} | seed supragnosis-sandbox-hub sandbox-hub

echo "starting the hub so it can serve the sync API"
dc start hub >/dev/null 2>&1
sleep 6

# The spoke stays stopped: `sync` refuses to share the store with a live daemon, so the round runs in
# a throwaway container on the compose network instead.
NET=$(docker network ls --format '{{.Name}}' | grep sandbox | head -1)
for ws in shared spoke-only hub-only; do
  echo "--- sync $ws ---"
  docker run --rm --network "$NET" -v supragnosis-sandbox-spoke:/var/lib/supragnosis/.supragnosis \
    "$IMG" sync --workspace "$ws" 2>&1 | grep -vE "INFO|federation identity" | head -3 || true
done

# A round that fails at the door (spoke-only, by design) stamps the log via `backfill` and never
# reaches its `reproject`, so the entity rows keep pre-stamp provenance and the graph reports no
# origin for knowledge the log attributes fine. Catch the projection up, or the sandbox displays a
# staleness as if it were a fact (F5's transient window, Prop C).
docker run --rm -v supragnosis-sandbox-spoke:/var/lib/supragnosis/.supragnosis \
  "$IMG" reproject --workspace spoke-only 2>&1 | grep -E "reprojected" || true

dc start spoke >/dev/null 2>&1
echo
echo "done. spoke http://127.0.0.1:7531   hub http://127.0.0.1:7521   (dock -> PEERS)"
echo "the spoke_only round is EXPECTED to fail with 403 - that is the surface refusing, out loud."
