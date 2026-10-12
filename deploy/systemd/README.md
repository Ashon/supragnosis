# Running supragnosis on Linux (systemd user service)

The Linux counterpart to the macOS LaunchAgent that `supragnosis service install` generates
(docs/daemon-lifecycle.md). Written while standing up the first federation hub
(`docs/federation.md` Phase 6), so the gotchas below are the ones actually hit, not anticipated.

A **user** unit, not a system one: the store, the node key, and the config all live under
`~/.supragnosis`, so nothing here needs root - which matters, because a machine you only have
password sudo on is exactly where an always-on hub tends to live.

## Install

```sh
mkdir -p ~/.config/systemd/user
cp deploy/systemd/supragnosis.service ~/.config/systemd/user/

loginctl enable-linger "$USER"      # once: without it the service dies at logout
systemctl --user daemon-reload
systemctl --user enable --now supragnosis

systemctl --user status supragnosis
journalctl --user -u supragnosis -f
```

Check `loginctl show-user "$USER" | grep Linger` first - it may already be on.

## Getting the binary

Every release ships a Linux tarball for `x86_64-unknown-linux-gnu` and one for
`aarch64-unknown-linux-gnu`; arm64 hosts, Grace/GH200 boxes among them, take the second. Both are
built on Ubuntu 22.04, so they need glibc 2.35 or newer:

```sh
v=X.Y.Z                         # the release to install
t=aarch64-unknown-linux-gnu     # or x86_64-unknown-linux-gnu
base="https://github.com/Ashon/supragnosis/releases/download/v$v"
curl -fsSLO "$base/supragnosis-v$v-$t.tar.gz"
curl -fsSLO "$base/supragnosis-v$v-$t.tar.gz.sha256"
sha256sum -c "supragnosis-v$v-$t.tar.gz.sha256"
tar -xzf "supragnosis-v$v-$t.tar.gz"
install -m 755 "supragnosis-v$v-$t/supragnosis" ~/.local/bin/supragnosis
```

The release binary is built without `--features fastembed`, which is what a pure hub wants. The hub
relays and stores; retrieval happens on the spokes, so the ONNX runtime buys nothing there.

Build from source for an older glibc or for a feature the release leaves out:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
. "$HOME/.cargo/env"
cargo build --release --locked --bin supragnosis
install -m 755 target/release/supragnosis ~/.local/bin/supragnosis
```

The build needs a C compiler (`apt install build-essential`, or your distribution's equivalent):
the TLS stack compiles C. Nothing in it needs libclang or bindgen any more - that, and the
`zstd-sys` headers it once took, left with the Cozo store in v0.2.0.

## Two things that bite in a *user* unit

- `StartLimitIntervalSec` / `StartLimitBurst` belong in `[Unit]`. In `[Service]` systemd logs
  `Unknown key name ... ignoring` and the rate limit silently does not exist.
- `ProtectKernelTunables=`, `ProtectKernelModules=`, and `ProtectControlGroups=` imply a
  `CapabilityBoundingSet` that an unprivileged user manager cannot set. The unit dies with
  `218/CAPABILITIES` and `Failed to drop capabilities` **before the binary ever runs**. They are
  system-unit directives. The namespace- and seccomp-based hardening (`ProtectSystem`,
  `ProtectHome`, `PrivateTmp`, `RestrictNamespaces`, ...) works fine unprivileged and is what the
  shipped unit keeps.

## As a federation hub

The unit runs `serve`, and a `[server]` section in `supragnosis.toml` starts the sync API
alongside it. F10 validates at startup: a non-loopback bind demands **both** TLS and at least one
admitted node or principal, and a misconfigured section fails the daemon loudly rather than dropping
the role.

Self-signed is fine for an internal hub - event authenticity rests on the ed25519 signatures
(F6), and TLS here only buys transport privacy. Give the cert an **IP** SAN if spokes will dial
an address rather than a name:

```sh
mkdir -p ~/.supragnosis/tls && cd ~/.supragnosis/tls
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -sha256 -days 3650 -nodes \
  -keyout hub.key -out hub.crt \
  -subj "/CN=supragnosis-hub" \
  -addext "subjectAltName=IP:10.0.0.2,DNS:hub.internal"
chmod 600 hub.key
```

`~/.supragnosis/supragnosis.toml` on the **hub**. No `[sync]` section: that is the outbound share
list for a node that syncs *up* to servers, and a hub has none. What a spoke may read is decided
per-peer - `pull_handler` exports with the allowlist entry's `shared_workspaces` (6c).

```toml
host_label = "hub"

[server]
listen   = "10.0.0.2:7420"
tls_cert = "/home/you/.supragnosis/tls/hub.crt"
tls_key  = "/home/you/.supragnosis/tls/hub.key"

[[server.allowlist]]
node_id           = "<spoke: supragnosis identity>"
public_key_hex    = "<spoke: supragnosis identity>"
bearer_hash       = "<spoke: supragnosis identity --hash-token TOKEN>"
shared_workspaces = ["your-workspace"]
```

And on the **spoke**:

```toml
host_label = "laptop"

[sync]
share_workspaces = ["your-workspace"]

[[sync.server]]
url        = "https://10.0.0.2:7420"
auth_token = "TOKEN"                     # the hub stores only blake3(TOKEN)
ca         = "/etc/supragnosis/hub.pem"  # a copy of the hub's tls_cert, when it is self-signed

[sync.origin_keys]
"<hub node_id>" = "<hub public key>"
```

A self-signed hub is trusted by naming its certificate: copy the hub's `tls_cert` file to the spoke
and point `ca` at it. The spoke then trusts that certificate and no other, including one `openssl
req -x509` made with its defaults. `insecure_tls` is retired: it accepted any certificate, so it is
no longer read, and a spoke that still sets it says so in its configuration notes.

Prefer a literal IP over an mDNS `.local` name in `servers` - it keeps name resolution out of the
sync path entirely.

Then, with the spoke's daemon stopped (the store is single-process):

```sh
supragnosis sync --workspace your-workspace
# https://10.0.0.2:7420: pushed 33 pulled 0 (rejected: by server 0, locally 0)
```

With the daemon running, use the `sync_status` / `sync_pull` / `sync_push` MCP tools instead -
they share the process's SyncNode. A second round moving `pushed 0 pulled 0` is the convergence
check; `curl --unix-socket ~/.supragnosis/viz.sock http://viz/api/graph?workspace=...` on both
ends should report the same entity and relation counts.

**Multi-principal needs Phase 5.** One person across many machines is the P23 solo exception and
works today; onboarding a second principal requires the governance enforcement that has not
landed (`docs/federation.md` Section 10).

## The other Linux option

[../docker/README.md](../docker/README.md) runs the same daemon as a container. Prefer this
unit when the hub shares a machine you already administer; prefer the container when the host
is disposable and the state volume is what you back up.
