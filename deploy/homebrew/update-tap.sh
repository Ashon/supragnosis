#!/usr/bin/env bash
# Renders the tap's formula and casks for a released version: copies them from the templates next
# to this script, then fills in the version and the sha256 sums from the release's sidecar files.
# Run from the checkout of the tag being released, against a checkout of the tap repo:
#   update-tap.sh v0.1.10 [path-to-tap-checkout] [dir-of-bottle-json]
#
# The third argument is where the release's `brew bottle --json` outputs are (the release.yml bottle
# job). Each becomes a line of the formula's bottle block. Without any, the formula has no bottle,
# and Homebrew treats it as a source build: it then demands an up-to-date Xcode or Command Line
# Tools, though the formula only copies a prebuilt binary.
#
# The templates are the source and the tap is output. This script used to edit the tap's own copy
# in place - version and sha256 only - so a change to a template's structure never reached users:
# v0.4.3 dropped the formula's `service do` block, and the tap would have kept it, along with the
# `brew services start` advice that put a second owner on the store (docs/daemon-lifecycle.md).
set -euo pipefail

TAG="${1:?usage: update-tap.sh vX.Y.Z [tap-dir] [bottle-json-dir]}"
TAP_DIR="${2:-.}"
BOTTLE_DIR="${3:-}"
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
# -i.bak (attached suffix) works under both BSD and GNU sed - the tap may be rendered on Linux.
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

# The bottle block: one line per bottle the release built, all served from the release's assets. A
# bottle for another version or from another place is refused rather than written - it would send
# every `brew install` to a file that is not this release's.
python3 - "$FORMULA" "$VERSION" "$BASE" "$BOTTLE_DIR" <<'EOF'
import glob, json, os, sys
path, version, base, bottle_dir = sys.argv[1:]
lines = []
for j in sorted(glob.glob(os.path.join(bottle_dir, "*.bottle.json"))) if bottle_dir else []:
    for name, entry in json.load(open(j)).items():
        got = entry["formula"]["pkg_version"]
        if got != version:
            sys.exit(f"update-tap.sh: {j} bottles {name} {got}, not {version}")
        bottle = entry["bottle"]
        if bottle["root_url"].rstrip("/") != base:
            sys.exit(f"update-tap.sh: {j} serves from {bottle['root_url']}, not {base}")
        cellar = bottle["cellar"]
        cellar = f":{cellar}" if cellar.startswith("any") else f'"{cellar}"'
        for tag, spec in bottle["tags"].items():
            lines.append(f'    sha256 cellar: {cellar}, {tag}: "{spec["sha256"]}"')
src = open(path).read()
if lines:
    block = "  bottle do\n" + f'    root_url "{base}"\n' + "\n".join(lines) + "\n  end"
    src = src.replace("  # BOTTLE_BLOCK", block)
    print(f"update-tap.sh: {len(lines)} bottle(s) for {version}")
else:
    src = src.replace("  # BOTTLE_BLOCK\n", "")
    print("update-tap.sh: no bottles - Homebrew will install the formula as a source build")
open(path, "w").write(src)
EOF

# A template that grows a sha256 line, or a version line in another shape, would otherwise ship a
# placeholder or the template's own stale version to every `brew upgrade`.
if grep -nE 'REPLACE_|BOTTLE_BLOCK' "$FORMULA" "$CASK"; then
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
