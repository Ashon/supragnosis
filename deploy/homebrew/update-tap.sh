#!/usr/bin/env bash
# Renders the tap's formula and casks for a released version: copies them from the templates next
# to this script, then fills in the version and the sha256 sums from the release's sidecar files.
# Run from the checkout of the tag being released, against a checkout of the tap repo:
#   update-tap.sh v0.1.10 [path-to-tap-checkout]
#
# The templates are the source and the tap is output. This script used to edit the tap's own copy
# in place - version and sha256 only - so a change to a template's structure never reached users:
# v0.4.3 dropped the formula's `service do` block, and the tap would have kept it, along with the
# `brew services start` advice that put a second owner on the store (docs/daemon-lifecycle.md).
set -euo pipefail

TAG="${1:?usage: update-tap.sh vX.Y.Z [tap-dir]}"
TAP_DIR="${2:-.}"
VERSION="${TAG#v}"
BASE="https://github.com/Ashon/supragnosis/releases/download/${TAG}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

sha_of() { # asset name -> sha256 (the release publishes <asset>.sha256 sidecars)
  curl -fsSL "${BASE}/$1.sha256" | awk '{print $1}'
}

FORMULA="${TAP_DIR}/Formula/supragnosis-server.rb"
CASK="${TAP_DIR}/Casks/supragnosis.rb"

# Fetch every sum before writing anything, so a missing asset leaves the tap as it was.
arm=$(sha_of "supragnosis-${TAG}-aarch64-apple-darwin.tar.gz")
x86=$(sha_of "supragnosis-${TAG}-x86_64-apple-darwin.tar.gz")
lin=$(sha_of "supragnosis-${TAG}-x86_64-unknown-linux-gnu.tar.gz")
app=$(sha_of "Supragnosis-${TAG}-macos-universal.app.zip")

# The dev cask has no version or sum (it tracks a rolling asset), so it is copied as-is.
mkdir -p "${TAP_DIR}/Formula" "${TAP_DIR}/Casks"
cp "${HERE}/Formula/supragnosis-server.rb" "$FORMULA"
cp "${HERE}/Casks/supragnosis.rb" "$CASK"
cp "${HERE}/Casks/supragnosis-dev.rb" "${TAP_DIR}/Casks/supragnosis-dev.rb"

# version line, then each sha256 by position: formula has 3 (arm, x86, linux), cask has 1.
# -i.bak (attached suffix) works under both BSD and GNU sed - the CI tap job runs on Linux.
sed -i.bak -E "s/^(  version \")[^\"]+(\")/\\1${VERSION}\\2/" "$FORMULA" "$CASK"
rm -f "${FORMULA}.bak" "${CASK}.bak"
python3 - "$FORMULA" "$arm" "$x86" "$lin" <<'EOF'
import re, sys
path, *shas = sys.argv[1:]
src = open(path).read()
it = iter(shas)
src = re.sub(r'(sha256 ")[^"]*(")', lambda m: m.group(1) + next(it) + m.group(2), src, count=3)
open(path, "w").write(src)
EOF
python3 - "$CASK" "$app" <<'EOF'
import re, sys
path, sha = sys.argv[1], sys.argv[2]
src = open(path).read()
src = re.sub(r'(sha256 ")[^"]*(")', lambda m: m.group(1) + sha + m.group(2), src, count=1)
open(path, "w").write(src)
EOF

# A template that grows a sha256 line, or a version line in another shape, would otherwise ship a
# placeholder or the template's own stale version to every `brew upgrade`.
if grep -n 'REPLACE_' "$FORMULA" "$CASK"; then
  echo "update-tap.sh: a placeholder survived rendering (above)" >&2
  exit 1
fi
for f in "$FORMULA" "$CASK"; do
  if ! grep -q "^  version \"${VERSION}\"$" "$f"; then
    echo "update-tap.sh: no version ${VERSION} in $f" >&2
    exit 1
  fi
done

echo "tap rendered for ${TAG}:"
grep -H "version \"" "$FORMULA" "$CASK"
