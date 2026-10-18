#!/usr/bin/env sh
# supragnosis install bootstrap: detects the platform and installs the latest
# GitHub Release binary to ~/.local/bin.
#
#   curl -fsSL https://supragnosis.dev/install.sh | sh
#
# Environment variables:
#   SUPRAGNOSIS_VERSION  tag to install (default: latest)
#   BIN_DIR              install path (default: ~/.local/bin)
#
# The prebuilt carries local semantic search (built with --features fastembed). Its model (224 MB)
# is fetched once, by `supragnosis service install` or `supragnosis embed fetch`; until then search
# is keyword only, and `supragnosis status` says so.
set -eu

REPO="Ashon/supragnosis"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
VERSION="${SUPRAGNOSIS_VERSION:-latest}"

usage() {
  cat <<'USAGE'
supragnosis install script

Detects the platform (macOS on Apple silicon, Linux x86_64/aarch64) and installs the GitHub Release binary
(with sha256 checksum verification). The default install path is ~/.local/bin.

Usage:
  curl -fsSL https://supragnosis.dev/install.sh | sh
  curl -fsSL .../install.sh | sh -s -- [options]   # pass options via pipe
  sh scripts/install.sh [options]                  # from a local file

Options:
  -h, --help          Print this help
  -v, --version TAG   Release tag to install (default: latest, e.g.: v0.1.0)
  -d, --dir DIR       Install path (default: ~/.local/bin)

Environment variables (options take precedence):
  SUPRAGNOSIS_VERSION   tag to install
  BIN_DIR               install path

Examples:
  sh scripts/install.sh --version v0.1.0
  BIN_DIR=/usr/local/bin sh scripts/install.sh

Note: semantic search needs the local embedding model (224 MB), fetched once by
  supragnosis service install, or supragnosis embed fetch. Until then search is keyword only.
USAGE
}

# Option parsing (overrides the environment-variable defaults). When run via pipe, pass with `sh -s -- --help`.
while [ $# -gt 0 ]; do
  case "$1" in
    -h|--help)     usage; exit 0 ;;
    -v|--version)  VERSION="${2:?--version requires a tag}"; shift 2 ;;
    -d|--dir)      BIN_DIR="${2:?--dir requires a path}"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; echo >&2; usage >&2; exit 2 ;;
  esac
done

# The oldest glibc the Linux binaries run on: the one they are built against, Ubuntu 24.04's
# (Debian 13 and Fedora 40 and later have it too). The release checks the binary it built against
# this line, so the number here is the number the binary asks for. Older hosts build from source
# or run the container image.
GLIBC_MIN=2.39

os="$(uname -s)"
arch="$(uname -m)"
case "${os}-${arch}" in
  Darwin-arm64)   target="aarch64-apple-darwin" ;;
  Darwin-x86_64)
    # A shell running under Rosetta on Apple silicon reports x86_64 as well, and the arm64 binary is
    # the one that machine runs. A real Intel Mac has no release binary to install.
    if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ]; then
      target="aarch64-apple-darwin"
    else
      echo "Intel Macs have no prebuilt binary: releases build macOS for Apple silicon only." >&2
      echo "Install via source build: https://github.com/${REPO} (cargo build --release)" >&2
      exit 1
    fi
    ;;
  Linux-x86_64)   target="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64)  target="aarch64-unknown-linux-gnu" ;;
  *)
    echo "Unsupported platform: ${os}-${arch}" >&2
    echo "Install via source build: https://github.com/${REPO} (cargo build --release)" >&2
    exit 1
    ;;
esac

# A Linux host older than the binary's glibc would fail at the first run with a message about a
# missing GLIBC_ version; say it here instead, with the two ways out.
if [ "${os}" = "Linux" ]; then
  have="$(ldd --version 2>/dev/null | sed -n '1s/.* \([0-9][0-9]*\.[0-9][0-9]*\)$/\1/p')"
  if [ -n "${have}" ] && [ "$(printf '%s\n%s\n' "${GLIBC_MIN}" "${have}" | sort -V | head -n1)" != "${GLIBC_MIN}" ]; then
    echo "This host has glibc ${have}; the Linux binaries need ${GLIBC_MIN} or newer (Ubuntu 24.04, Debian 13, Fedora 40 and later)." >&2
    echo "Build from source (deploy/systemd/README.md), or run the container image ghcr.io/ashon/supragnosis (deploy/docker/README.md): https://github.com/${REPO}" >&2
    exit 1
  fi
fi

# Look up the latest release tag (or use the specified version).
if [ "${VERSION}" = "latest" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -m1 '"tag_name"' | cut -d'"' -f4)"
  [ -n "${VERSION}" ] || { echo "Could not find the latest release." >&2; exit 1; }
fi

name="supragnosis-${VERSION}-${target}"
url="https://github.com/${REPO}/releases/download/${VERSION}/${name}.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT INT TERM

echo "Download: ${url}"
curl -fsSL "${url}" -o "${tmp}/pkg.tar.gz"

# Checksum verification, required. Every release publishes <asset>.sha256 beside each binary, so a
# checksum that cannot be fetched means the download is not what the release published - or the
# network in between is not to be trusted with it. Installing anyway would make the check decorative:
# it used to be skipped whenever the .sha256 could not be fetched.
if ! curl -fsSL "${url}.sha256" -o "${tmp}/pkg.sha256" || [ ! -s "${tmp}/pkg.sha256" ]; then
  echo "No checksum at ${url}.sha256 - refusing to install a binary that cannot be verified." >&2
  exit 1
fi
want="$(cut -d' ' -f1 "${tmp}/pkg.sha256")"
case "${want}" in
  *[!0-9a-f]* | "") echo "The checksum file does not hold a sha256 - refusing to install." >&2; exit 1 ;;
esac
[ "${#want}" -eq 64 ] || { echo "The checksum file does not hold a sha256 - refusing to install." >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
  got="$(sha256sum "${tmp}/pkg.tar.gz" | cut -d' ' -f1)"
elif command -v shasum >/dev/null 2>&1; then
  got="$(shasum -a 256 "${tmp}/pkg.tar.gz" | cut -d' ' -f1)"
else
  echo "Neither sha256sum nor shasum is installed, so the download cannot be verified - refusing." >&2
  exit 1
fi
if [ "${want}" != "${got}" ]; then
  echo "Checksum mismatch (expected ${want}, actual ${got})." >&2
  exit 1
fi
echo "Checksum OK"

tar -C "${tmp}" -xzf "${tmp}/pkg.tar.gz"
mkdir -p "${BIN_DIR}"
install -m 0755 "${tmp}/${name}/supragnosis" "${BIN_DIR}/supragnosis"
echo "Install complete: ${BIN_DIR}/supragnosis (${VERSION}, ${target})"

# PATH guidance.
case ":${PATH}:" in
  *":${BIN_DIR}:"*) : ;;
  *) echo "Note: ${BIN_DIR} is not in PATH. Add it to your shell config (e.g.: ~/.zshrc):"
     echo "  export PATH=\"${BIN_DIR}:\$PATH\"" ;;
esac

# Keeping the daemon running has one path on macOS: the login job `service install` generates
# (docs/daemon-lifecycle.md). It also owns restarts, so a later re-run of this script is followed by
# `supragnosis restart`. On Linux that command declines and points at the systemd user unit.
if [ "${os}" = "Darwin" ]; then
  daemon_hint="       \"${BIN_DIR}/supragnosis\" service install
       # starts it now and at every login. MCP: http://127.0.0.1:7373/mcp
       # Viewer: HTTP over unix socket ~/.supragnosis/viz.sock (the desktop app attaches to it)
       # After re-running this installer to upgrade: supragnosis restart"
else
  daemon_hint="       SUPRAGNOSIS_HTTP_ADDR=127.0.0.1:7373 SUPRAGNOSIS_VIZ_SOCK=~/.supragnosis/viz.sock \"${BIN_DIR}/supragnosis\" serve
       # MCP: http://127.0.0.1:7373/mcp   Viewer: HTTP over unix socket ~/.supragnosis/viz.sock
       # To keep it running, see the systemd user unit in the repository's deploy/systemd/."
fi

# The first run is one command now (docs/onboarding.md Section 8): `setup` shows its plan and asks
# before it acts. A binary from before `setup` existed gets the steps written out below instead.
if "${BIN_DIR}/supragnosis" setup --help >/dev/null 2>&1; then
  cat <<EOF

Next: supragnosis setup
  It keeps the daemon running, connects your AI apps and downloads the search model (224 MB),
  showing the plan and asking before it changes anything.
  Help: sh install.sh --help   |   Docs/issues: https://github.com/${REPO}
EOF
  # Under `curl | sh` standard input is this script, so the answer is read from the terminal itself,
  # and only when there is one to read from.
  if (exec </dev/tty) 2>/dev/null; then
    printf "Run it now? [Y/n] " >/dev/tty
    answer=n
    read -r answer </dev/tty || answer=n
    case "${answer}" in
      "" | y | Y | yes) exec "${BIN_DIR}/supragnosis" setup </dev/tty ;;
    esac
  fi
  exit 0
fi

cat <<EOF

Install complete. Onboarding:

  1) Run the always-on daemon + live viewer
${daemon_hint}

  2) Connect an AI app (it talks to the daemon; no token to copy)
       "${BIN_DIR}/supragnosis" connect claude-code
       # or claude-desktop, cursor, vscode, codex, gemini - \`supragnosis connect\` lists them

  - Search: semantic search needs the local model (224 MB), fetched once by \`supragnosis embed fetch\`
    (service install does it too); until then search is keyword only, and \`supragnosis status\` says so.
  - Help: sh install.sh --help   |   Docs/issues: https://github.com/${REPO}
EOF
