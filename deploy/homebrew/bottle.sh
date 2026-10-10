#!/usr/bin/env bash
# Bottles the server formula for a released tag, and proves the bottle pours (README.md, Bottles).
#   bottle.sh vX.Y.Z OUT_DIR
#
# The formula only copies the release's prebuilt binary, but Homebrew treats a formula without a
# bottle as a source build: it insists on an up-to-date Xcode or Command Line Tools first, and fails
# without them. A bottle is poured instead, with no such check.
#
# Steps, all on the machine it runs on (a CI runner - it installs into that machine's Homebrew):
#   1. render the formula for TAG from this checkout's template (update-tap.sh);
#   2. install it with --build-bottle from a local tap named like the real one, since a bottle
#      records the tap it came from;
#   3. `brew bottle` it, naming the release as where the bottle will be served from;
#   4. rename the file to the name the bottle block's URL asks for (brew writes name--version);
#   5. reinstall from that bottle, served from OUT_DIR, and require that Homebrew poured it.
#
# OUT_DIR is left holding the bottle and its JSON. The release uploads the bottle and hands the
# JSON to update-tap.sh, which writes the bottle block. A pull request runs the same steps against
# the latest release and keeps nothing (.github/workflows/homebrew.yml).
set -euo pipefail

TAG="${1:?usage: bottle.sh vX.Y.Z OUT_DIR}"
OUT="${2:?usage: bottle.sh vX.Y.Z OUT_DIR}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RELEASE="https://github.com/Ashon/supragnosis/releases/download/${TAG}"
export HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 HOMEBREW_NO_ANALYTICS=1 HOMEBREW_NO_ENV_HINTS=1

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# 1. The formula for this tag. Only the formula: the desktop app is attached to the release after
#    the bottles are built.
FORMULA_ONLY=1 "${HERE}/update-tap.sh" "$TAG" "${WORK}/render" >/dev/null

# 2. A local tap named like the real one. Newer Homebrew refuses developer commands on a tap it has
#    not been told to trust; older Homebrew has no such command.
brew tap-new --no-git ashon/tap >/dev/null
brew trust ashon/tap >/dev/null 2>&1 || true
TAP_FORMULA="$(brew --repository ashon/tap)/Formula/supragnosis-server.rb"
cp "${WORK}/render/Formula/supragnosis-server.rb" "$TAP_FORMULA"
brew install --build-bottle ashon/tap/supragnosis-server

# 3 and 4. The bottle, under the name its URL will ask for.
cd "$OUT"
brew bottle --json --root-url "$RELEASE" ashon/tap/supragnosis-server
python3 - <<'EOF'
import glob, json, os
for j in glob.glob("*.bottle.json"):
    for entry in json.load(open(j)).values():
        for tag in entry["bottle"]["tags"].values():
            if tag["local_filename"] != tag["filename"]:
                os.rename(tag["local_filename"], tag["filename"])
EOF

# 5. It pours. The bottle block brew writes names the release; for this check it is pointed at
#    OUT_DIR instead, so the check needs nothing uploaded and proves the file itself.
brew uninstall supragnosis-server
brew bottle --merge --write --no-commit ./*.bottle.json
sed -i.bak "s|root_url \"${RELEASE}\"|root_url \"file://${OUT}\"|" "$TAP_FORMULA"
rm -f "${TAP_FORMULA}.bak"
brew install ashon/tap/supragnosis-server 2>&1 | tee "${WORK}/install.log"
if ! grep -q "Pouring supragnosis-server" "${WORK}/install.log"; then
  echo "bottle.sh: Homebrew did not pour the bottle - it would still demand Xcode" >&2
  exit 1
fi
"$(brew --prefix)/bin/supragnosis" --version
ls -1 "$OUT"
