#!/usr/bin/env bash
# Writes a golden store (docs/compatibility.md Section 5) with a RELEASED supragnosis binary.
#   make.sh BIN FORMAT
# e.g. make.sh /opt/homebrew/bin/supragnosis 2  ->  format-2.redb.gz and format-2.json beside this file
#
# A golden store is only worth anything if a release wrote it, so this never builds the tree: BIN
# is the last release of FORMAT, installed. Everything happens under a temporary HOME - two nodes,
# a hub and a spoke, on loopback - so no live store, daemon or token is read or touched.
#
# The spoke's store is the fixture. It ends up holding:
#   - its own observations in two workspaces, with entities, relations and a description;
#   - a type definition and an open proposal;
#   - its shared observations stamped and signed by itself (sync backfill on push);
#   - an observation pulled from the hub, stamped and signed by the hub.
# format-N.json records what the test needs and cannot derive: the release that wrote the store,
# and the two nodes' public keys, so every signature in it can be verified by a later build.
set -euo pipefail

BIN="${1:?usage: make.sh BIN FORMAT}"
FORMAT="${2:?usage: make.sh BIN FORMAT}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d)"
HUB_PID=""
cleanup() {
  [ -n "$HUB_PID" ] && kill "$HUB_PID" 2>/dev/null && wait "$HUB_PID" 2>/dev/null
  rm -rf "$WORK"
}
trap cleanup EXIT

VERSION="$("$BIN" --version | awk '{print $NF}')"
HUB="$WORK/hub"
SPOKE="$WORK/spoke"
mkdir -p "$HUB/.supragnosis" "$SPOKE/.supragnosis"
chmod 700 "$HUB/.supragnosis" "$SPOKE/.supragnosis"
TOKEN=golden-spoke-token
PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
MCP_PORT="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"

field() { sed -n "s/^$2: *//p" "$1" | head -1; }
HOME="$HUB" "$BIN" identity > "$WORK/hub.id"
HOME="$SPOKE" "$BIN" identity --hash-token "$TOKEN" > "$WORK/spoke.id"
HUB_ID=$(field "$WORK/hub.id" node_id)
HUB_KEY=$(field "$WORK/hub.id" public_key)
SPOKE_ID=$(field "$WORK/spoke.id" node_id)
SPOKE_KEY=$(field "$WORK/spoke.id" public_key)
SPOKE_HASH=$(field "$WORK/spoke.id" bearer_hash)

cat > "$HUB/.supragnosis/supragnosis.toml" <<EOF
host_label = "golden-hub"

[sync]
share_workspaces = ["shared"]

[sync.origin_keys]
$SPOKE_ID = "$SPOKE_KEY"

[server]
listen = "127.0.0.1:$PORT"

[[server.allowlist]]
node_id = "$SPOKE_ID"
public_key_hex = "$SPOKE_KEY"
bearer_hash = "$SPOKE_HASH"
shared_workspaces = ["shared"]
EOF

cat > "$SPOKE/.supragnosis/supragnosis.toml" <<EOF
host_label = "golden-spoke"

[sync]
share_workspaces = ["shared"]

[[sync.server]]
url = "http://127.0.0.1:$PORT"
auth_token = "$TOKEN"

[sync.origin_keys]
$HUB_ID = "$HUB_KEY"
EOF
chmod 600 "$HUB/.supragnosis/supragnosis.toml" "$SPOKE/.supragnosis/supragnosis.toml"

# One stdio MCP session per node: each call waits for its answer, and any tool error stops the run.
session() {
  local home="$1" calls="$2"
  HOME="$home" python3 - "$BIN" "$calls" <<'PY'
import json, subprocess, sys
binary, calls = sys.argv[1], json.loads(sys.argv[2])
p = subprocess.Popen([binary, "serve", "--embed", "hashing", "--host", "golden"],
                     stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
def send(msg):
    p.stdin.write(json.dumps(msg) + "\n"); p.stdin.flush()
def answer(i):
    while True:
        line = json.loads(p.stdout.readline())
        if line.get("id") == i:
            return line
send({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
    "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "golden", "version": "0"}}})
answer(0)
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
for i, (name, args) in enumerate(calls, start=1):
    send({"jsonrpc": "2.0", "id": i, "method": "tools/call", "params": {"name": name, "arguments": args}})
    r = answer(i)
    if "error" in r or r["result"].get("isError"):
        sys.exit(f"{name} failed: {json.dumps(r)[:400]}")
p.stdin.close(); p.wait(timeout=30)
PY
}

session "$SPOKE" '[
  ["observe", {"workspace": "shared",
    "content": "supragnosis keeps its log in redb, an embedded key-value store written in Rust.",
    "entities": [{"name": "supragnosis", "type": "Software", "description": "A local-first knowledge server."},
                 {"name": "redb", "type": "Technology"}],
    "relations": [{"from": "supragnosis", "type": "uses", "to": "redb"}]}],
  ["observe", {"workspace": "shared", "source_ref": "golden:2", "confidence": 0.8,
    "content": "redb is also written redb-db in some notes.",
    "entities": [{"name": "redb-db", "type": "Technology"}]}],
  ["observe", {"workspace": "private",
    "content": "The spoke keeps this workspace to itself.",
    "entities": [{"name": "spoke notes", "type": "Concept"}]}],
  ["define_type", {"workspace": "shared",
    "defs": [{"target": "entity", "name": "Technology", "description": "A tool or library software is built with."}]}],
  ["propose", {"workspace": "shared", "kind": "entity_merge", "targets": ["redb", "redb-db"], "into": "redb",
    "rationale": "Two spellings of one store."}]
]'

session "$HUB" '[
  ["observe", {"workspace": "shared",
    "content": "The hub serves federation on its sync listener.",
    "entities": [{"name": "hub", "type": "Concept"}],
    "relations": [{"from": "hub", "type": "serves", "to": "supragnosis"}]}]
]'

HOME="$HUB" "$BIN" serve --http "127.0.0.1:$MCP_PORT" --viz "$WORK/hub.sock" --embed hashing \
  > "$WORK/hub.log" 2>&1 &
HUB_PID=$!
for _ in $(seq 1 50); do
  python3 -c "import socket; socket.create_connection(('127.0.0.1',$PORT),0.2)" 2>/dev/null && break
  sleep 0.2
done

HOME="$SPOKE" SUPRAGNOSIS_EMBED=hashing "$BIN" sync --workspace shared

gzip -9 -n -c "$SPOKE/.supragnosis/redb/knowledge.redb" > "$HERE/format-$FORMAT.redb.gz"
cat > "$HERE/format-$FORMAT.json" <<EOF
{
  "format": $FORMAT,
  "written_by": "$VERSION",
  "embedder": "hashing",
  "origin_keys": {
    "$SPOKE_ID": "$SPOKE_KEY",
    "$HUB_ID": "$HUB_KEY"
  }
}
EOF
echo "wrote format-$FORMAT.redb.gz and format-$FORMAT.json (by supragnosis $VERSION)"
