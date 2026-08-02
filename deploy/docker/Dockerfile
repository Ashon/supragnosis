# supragnosis as a federation hub in a container. Counterpart to deploy/systemd (Linux) and
# deploy/launchd (macOS); see deploy/docker/README.md for what a container changes and what it
# must not.
#
# Built from source rather than from a release tarball. The tarball would be faster, but this image
# is the thing a hub runs for months, and a Dockerfile that curls a binary at build time cannot be
# rebuilt to the same bits once that release is edited or replaced. Building from the repo also
# means the image can be cut from any commit, which is what an operator running a hub ahead of a
# release needs.

# The builder is pinned, but NOT to the workspace's declared MSRV (1.87). Cargo checks rust-version
# across the whole resolved graph, and the desktop shell - which this image does not build - depends
# on `oxc`, which requires 1.95. So a 1.87 builder fails at resolution over a package that would
# never be compiled. Pinned rather than `latest` so the image is reproducible; raise it deliberately.
FROM rust:1.95-bookworm AS build
WORKDIR /src

# Manifests first, so the dependency build is cached against everything but a dependency change.
# The crate sources are copied whole afterwards - a stub-src trick would have to enumerate every
# member of a nine-crate workspace and would rot the first time one is added.
COPY Cargo.toml Cargo.lock rustfmt.toml ./
COPY crates ./crates
COPY e2e ./e2e
# The desktop shell is not built here, but cargo will not load a workspace whose member manifests
# are missing - so `app` contributes its Cargo.toml and nothing else, plus a stub target to make
# that manifest a valid package. Cargo never compiles it: `app` is outside default-members and this
# build names one package. Copying the real shell would drag the whole Tauri toolchain into an image
# that does not run it.
COPY app/Cargo.toml ./app/Cargo.toml
RUN mkdir -p app/src && echo 'fn main() {}' > app/src/main.rs

# --locked: the lockfile in the repo is the one that ships. Resolving fresh here would mean the
# image and CI can disagree about a dependency, which is the kind of difference that only shows up
# in production.
#
# Default features: the prebuilt search path is keyword/hashing. `fastembed` pulls ONNX runtime and
# a model download, which belongs to a deliberate build rather than the default hub image.
RUN cargo build --release --locked -p supragnosis-cli \
    && strip target/release/supragnosis

# Bookworm slim, not Alpine: the binary is dynamically linked against glibc, and a musl target is
# not something this workspace builds or tests. ca-certificates is for the federation client half -
# a hub pulls from its peers as well as serving them, over TLS. curl is for the healthcheck, which
# asks the viewer socket a question only a working store can answer; without it the check fails
# closed and every container reads as unhealthy forever, which is worse than having no check.
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# A fixed uid, because the state volume outlives the image and its ownership has to keep matching.
# 10001 is outside the range Debian hands out to packages, so a base-image change cannot collide
# with it. HOME is what decides where the node key, the config and the store live: the daemon
# resolves all of them under $HOME/.supragnosis, so this one line is what makes a single volume
# enough.
RUN useradd --system --uid 10001 --create-home --home-dir /var/lib/supragnosis supragnosis \
    && install -d -o supragnosis -g supragnosis -m 700 /var/lib/supragnosis/.supragnosis
ENV HOME=/var/lib/supragnosis
WORKDIR /var/lib/supragnosis

COPY --from=build /src/target/release/supragnosis /usr/local/bin/supragnosis

# The state directory has to exist in the image, owned by the runtime user and 0700. A fresh named
# volume inherits the mount point's ownership from the image; without this it is created root-owned
# and the daemon cannot open its own store. 0700 is also what the viewer socket's directory wants -
# the guard that makes it reachable by one principal and no other (P17).
#
# The state directory is declared so an operator who forgets `-v` still gets persistence for the
# life of the container rather than losing the node identity at the first restart. It is NOT a
# substitute for a named volume - an anonymous volume is discarded with the container, and a new
# node.key means a new node_id, which every peer's allowlist would have to be told about (F14).
VOLUME ["/var/lib/supragnosis/.supragnosis"]

USER supragnosis

# 7420 is the federation sync API - the only surface allowed to bind non-loopback, and only with
# TLS and a non-empty allowlist (F10). 7373 (MCP) is deliberately NOT exposed: it is unauthenticated
# by design as a local trust surface (Principle 17), and publishing it from a container would put an
# unauthenticated write surface on the network. See README.md if you need it.
EXPOSE 7420

# `serve` runs in the foreground, which is what a container wants; `start` would daemonize behind a
# pidfile and the container would exit immediately. The viewer socket lands inside the state volume,
# where it is reachable by `docker exec` and by nothing else unless the operator chooses otherwise.
ENTRYPOINT ["/usr/local/bin/supragnosis"]
CMD ["serve", "--http", "127.0.0.1:7373", "--viz", "/var/lib/supragnosis/.supragnosis/viz.sock"]
