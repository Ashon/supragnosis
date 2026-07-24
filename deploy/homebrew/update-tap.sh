#!/usr/bin/env bash
# Rewrites the tap's formula/cask to a released version, pulling sha256 sidecar files
# from the GitHub release. Run inside a checkout of the tap repo:
#   update-tap.sh v0.1.10 [path-to-tap-checkout]
set -euo pipefail

TAG="${1:?usage: update-tap.sh vX.Y.Z [tap-dir]}"
TAP_DIR="${2:-.}"
VERSION="${TAG#v}"
BASE="https://github.com/Ashon/supragnosis/releases/download/${TAG}"

sha_of() { # asset name -> sha256 (the release publishes <asset>.sha256 sidecars)
  curl -fsSL "${BASE}/$1.sha256" | awk '{print $1}'
}

FORMULA="${TAP_DIR}/Formula/supragnosis-server.rb"
CASK="${TAP_DIR}/Casks/supragnosis.rb"

arm=$(sha_of "supragnosis-${TAG}-aarch64-apple-darwin.tar.gz")
x86=$(sha_of "supragnosis-${TAG}-x86_64-apple-darwin.tar.gz")
lin=$(sha_of "supragnosis-${TAG}-x86_64-unknown-linux-gnu.tar.gz")
app=$(sha_of "Supragnosis-${TAG}-macos-universal.app.zip")

# version line, then each sha256 by position: formula has 3 (arm, x86, linux), cask has 1.
sed -i '' -E "s/^(  version \")[^\"]+(\")/\\1${VERSION}\\2/" "$FORMULA" "$CASK"
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

echo "tap updated to ${TAG}:"
grep -H "version \"" "$FORMULA" "$CASK"
