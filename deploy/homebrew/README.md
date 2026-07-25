# Homebrew distribution (formula + cask, no DMG)

This directory is the template set copied into the tap repo. Contents:

- `Formula/supragnosis-server.rb` - the server/CLI (the installed binary is still named
  `supragnosis`; only the brew token carries `-server`). Installs the release's per-platform
  tar.gz as-is, and `brew services start supragnosis-server` registers the always-on daemon
  (launchd). `serve --http` also opens the viewer socket (`~/.supragnosis/viz.sock`) by default.
- `Casks/supragnosis.rb` - the desktop shell. It owns the plain token, so
  `brew install supragnosis` resolves to this cask (no formula shares the name). Installs the
  release's signed/notarized universal `.app.zip`. The cask depends on the `supragnosis-server`
  formula, so the app finds the brew daemon binary on PATH (no bundled sidecar). The app is
  tray-resident, so the cask's `uninstall quit:` quits the old instance on upgrade and reopens it.
- `update-tap.sh` - after a release, updates the tap's version/sha256 from the release assets'
  .sha256 sidecar files.

## One-time setup

1. Create the tap repo: make `Ashon/homebrew-tap` (public) on GitHub and commit this directory's
   `Formula/`, `Casks/`, and `update-tap.sh` into it.
2. Register repo secrets (Settings > Secrets and variables > Actions) - the release.yml app job
   uses them for signing/notarization. If any is missing (precisely: no APPLE_SIGNING_IDENTITY),
   the job only verifies the build without signing.
   - `APPLE_CERTIFICATE` - base64 of the Developer ID Application certificate .p12
     (`base64 -i cert.p12 | pbcopy`)
   - `APPLE_CERTIFICATE_PASSWORD` - the .p12 password
   - `APPLE_SIGNING_IDENTITY` - e.g. `Developer ID Application: <Name> (<TEAMID>)`
   - `APPLE_ID` - the Apple ID email
   - `APPLE_PASSWORD` - an app-specific password (issued at appleid.apple.com)
   - `APPLE_TEAM_ID` - the team id
3. Register the tap auto-update secret: create a fine-grained PAT (Developer settings > Personal
   access tokens > Fine-grained; restrict the target repo to `Ashon/homebrew-tap` only, with just
   Contents: Read and write) and add it as the `TAP_PUSH_TOKEN` repo secret. Without it the
   release.yml tap job skips and you fall back to the manual procedure below.
4. From the next `v*` tag on, the release carries `Supragnosis-v<ver>-macos-universal.app.zip`.

## Per release

Automatic: pushing a `v*` tag makes the release.yml tap job run update-tap.sh after the assets
are attached, updating the tap's version/sha256 and pushing. If the job fails or
`TAP_PUSH_TOKEN` is missing, run it by hand:

```sh
git clone git@github.com:Ashon/homebrew-tap && cd homebrew-tap
../supragnosis/deploy/homebrew/update-tap.sh v0.1.11 .
git commit -am "supragnosis v0.1.11" && git push
```

## User install

```sh
brew tap ashon/tap
brew install supragnosis                # desktop app (macOS, pulls the server formula)
brew install supragnosis-server         # server/CLI only (macOS / Linux)
brew services start supragnosis-server  # always-on daemon (MCP :7373 + viewer socket)
```

## Dev-channel install (--HEAD server + supragnosis-dev cask)

**Server/CLI**: the formula's `head` spec builds the main branch from source (the rust toolchain
arrives as a build dep; default features = keyword search, identical to the release binaries).
The viewer UI is embedded in the server binary, so even the stable desktop app shell renders a
HEAD server's viewer unchanged - the server swap alone is usually the whole dev experience.

```sh
# With stable installed, swap only the formula (pass the cask dependency warning
# with --ignore-dependencies)
brew services stop supragnosis-server
brew uninstall --ignore-dependencies supragnosis-server
brew install --HEAD supragnosis-server
brew services start supragnosis-server

brew upgrade --fetch-HEAD supragnosis-server   # whenever main moves
```

**Desktop app**: casks cannot build from source (no `--HEAD`), so the dev channel is the
`supragnosis-dev` cask - it installs the rolling `dev` pre-release that
`.github/workflows/dev-app.yml` rebuilds (signed/notarized like a release) whenever `app/`
changes on main, or on manual dispatch. `version :latest` means `brew upgrade` does not track
it: refresh with reinstall.

```sh
brew uninstall --cask supragnosis        # the two casks install the same app bundle
brew install --cask supragnosis-dev
brew reinstall supragnosis-dev           # whenever the dev release rolls
```

Returning to stable is the mirror procedure (`brew uninstall --cask supragnosis-dev`, then
`brew install supragnosis` / `brew install supragnosis-server` without --HEAD).
The server's version string reads `HEAD-<sha>`, so `brew info` shows which commit you run; the
dev release page names the app's built commit.
Data-compatibility caution: if a dev build changed the schema/id formula, check the release
notes' migrate guidance before returning to stable (`~/.supragnosis/db` is shared).

## Upgrades

An upgrade is complete only after `brew upgrade` plus a daemon restart - brew upgrade does not
restart a running service (the formula caveats print the same reminder), and without the restart
the old daemon keeps running from the deleted keg path:

```sh
brew upgrade
brew services restart supragnosis-server
```

If you installed under the old tokens (formula `supragnosis`, cask `supragnosis-app`),
reinstall. Stopping the service comes before uninstall - brew uninstall does not clean up a
running service/launchd plist:

```sh
brew services stop supragnosis 2>/dev/null
brew uninstall --cask supragnosis-app 2>/dev/null; brew uninstall --formula supragnosis 2>/dev/null
brew install supragnosis
```

Note: the old formula token is NOT forwarded via `formula_renames.json` - that would let the
plain token resolve as a formula name again, and `brew install supragnosis` must resolve to the
cask, not a formula.
