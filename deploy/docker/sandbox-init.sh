#!/bin/sh
# One-shot setup for the two-node sandbox (compose.sandbox.yaml).
#
# It exists because a federation pair cannot be described in a compose file alone: a hub's allowlist
# names the spoke's `node_id` and the blake3 of its bearer token, and both of those only exist after
# the spoke has generated a key. That is the out-of-band exchange federation.md Section 11 defers,
# and a sandbox is allowed to shortcut it by holding both sides at once - which is precisely what a
# real deployment must not do, and why this script lives here rather than in the product.
#
# Idempotent: it writes nothing that already exists, so `up` after `up` is a no-op and a node keeps
# the identity its peer was told about (F14).
set -eu

HUB_HOME=/hub
SPOKE_HOME=/spoke
HUB_STATE="$HUB_HOME/.supragnosis"
SPOKE_STATE="$SPOKE_HOME/.supragnosis"

# The buckets are the point of the sandbox, so the two sides are configured to DISAGREE on purpose.
# hub admits: shared, hub-only     spoke shares: shared, spoke-only
#   -> both = shared          (in sync)
#   -> local_only = spoke-only  (the spoke lists it, the hub does not admit it - a setup error)
#   -> peer_only = hub-only     (the hub would admit it, the spoke does not share it)
# Symmetric agreement would show one bucket and prove nothing.
HUB_ADMITS='"shared", "hub-only"'
SPOKE_SHARES='"shared", "spoke-only"'
SPOKE_TOKEN=sandbox-spoke-token

id_field() { sed -n "s/^$2: *//p" "$1" | head -1; }

# --- identities -------------------------------------------------------------
# `identity` generates node.key on first call and only reads it afterwards, so this is both the
# generator and the reader. HOME is what decides which key it touches, which is the same mechanism
# that keeps the two nodes apart inside one container here and in separate containers at runtime.
HOME="$HUB_HOME" supragnosis identity > /tmp/hub.id
HOME="$SPOKE_HOME" supragnosis identity --hash-token "$SPOKE_TOKEN" > /tmp/spoke.id

HUB_ID=$(id_field /tmp/hub.id node_id)
HUB_KEY=$(id_field /tmp/hub.id public_key)
SPOKE_ID=$(id_field /tmp/spoke.id node_id)
SPOKE_KEY=$(id_field /tmp/spoke.id public_key)
SPOKE_HASH=$(id_field /tmp/spoke.id bearer_hash)

echo "hub   $HUB_ID"
echo "spoke $SPOKE_ID"

# --- consistency guard ------------------------------------------------------
# A partial teardown leaves one volume and removes the other, so a node comes back with a new
# identity while its peer's allowlist still names the old one. That does NOT fail at the door:
# admission is by bearer hash, and the token here is fixed, so `ping` keeps answering and the panel
# keeps looking healthy. What breaks is the half that verifies content - a pushed event is checked
# against the `public_key_hex` in the entry (F6) - so the node is half-working in a way nothing
# announces. Refuse instead, and say which command fixes it.
if [ -f "$HUB_STATE/supragnosis.toml" ] \
  && ! grep -q "\"$SPOKE_ID\"" "$HUB_STATE/supragnosis.toml"; then
  echo "the hub's allowlist does not name this spoke ($SPOKE_ID)." >&2
  echo "one volume outlived the other, so the two nodes disagree about who the spoke is." >&2
  echo "wire auth is by bearer token and would still pass, while pushed events would not." >&2
  echo "fix: docker compose -f deploy/docker/compose.sandbox.yaml down -v --remove-orphans" >&2
  exit 1
fi

# --- hub config -------------------------------------------------------------
# `origin_keys` carries the spoke's public key so pushed events verify (F6). Without it every
# inbound event is rejected as UnknownOrigin and the counters read as a transport problem when they
# are a configuration one - a mistake worth pre-empting in a sandbox people will copy from.
if [ ! -f "$HUB_STATE/supragnosis.toml" ]; then
  cat > "$HUB_STATE/supragnosis.toml" <<EOF
host_label = "sandbox-hub"

[sync]
share_workspaces = ["shared", "hub-only"]

[sync.origin_keys]
$SPOKE_ID = "$SPOKE_KEY"

[server]
listen = "0.0.0.0:7420"
tls_cert = "/var/lib/supragnosis/.supragnosis/tls/cert.pem"
tls_key = "/var/lib/supragnosis/.supragnosis/tls/key.pem"

[[server.allowlist]]
node_id = "$SPOKE_ID"
public_key_hex = "$SPOKE_KEY"
bearer_hash = "$SPOKE_HASH"
shared_workspaces = [$HUB_ADMITS]
EOF
  echo "wrote hub config"
fi

# --- spoke config -----------------------------------------------------------
# The per-server shape, which is the point of Step 1: one credential per host rather than one token
# presented to all of them. `insecure_tls` because the hub's certificate is self-signed here -
# content authenticity stays with the event signatures either way (F6).
if [ ! -f "$SPOKE_STATE/supragnosis.toml" ]; then
  cat > "$SPOKE_STATE/supragnosis.toml" <<EOF
host_label = "sandbox-spoke"

[sync]
share_workspaces = [$SPOKE_SHARES]
insecure_tls = true

[[sync.server]]
url = "https://hub:7420"
auth_token = "$SPOKE_TOKEN"

[sync.origin_keys]
$HUB_ID = "$HUB_KEY"
EOF
  echo "wrote spoke config"
fi

chmod 600 "$HUB_STATE/supragnosis.toml" "$SPOKE_STATE/supragnosis.toml"
echo "sandbox init done"
