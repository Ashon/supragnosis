# Homebrew distribution (formula + cask, no DMG)

This directory is the template set copied into the tap repo. Contents:

- `Formula/supragnosis-server.rb` - the server/CLI (the installed binary is still named
  `supragnosis`; only the brew token carries `-server`). Installs the release's per-platform
  tar.gz as-is. It deliberately has no `service do` block: the always-on daemon has one manager,
  the canonical LaunchAgent that `supragnosis service install` (or the app's Start at Login)
  generates - a brew services job beside it would be a second owner of a single-writer store
  (docs/daemon-lifecycle.md). The formula's caveats say how to install it and how to migrate.
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
2. From the next `v*` tag on, the release carries `Supragnosis-v<ver>-macos-universal.app.zip`.

## Per release

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
supragnosis service install             # always-on daemon (MCP :7373 + viewer socket), now and at login
                                        # - or Start at Login in the app's tray menu
```

## Dev-channel install (--HEAD server + supragnosis-dev cask)

**Server/CLI**: the formula's `head` spec builds the main branch from source (the rust toolchain
arrives as a build dep; default features = keyword search, identical to the release binaries).
The viewer UI is embedded in the server binary, so even the stable desktop app shell renders a
HEAD server's viewer unchanged - the server swap alone is usually the whole dev experience.

```sh
# With stable installed, swap only the formula (pass the cask dependency warning
# with --ignore-dependencies)
supragnosis stop
brew uninstall --ignore-dependencies supragnosis-server
brew install --HEAD supragnosis-server
supragnosis restart        # reloads the login job; it names the opt link, so it runs the new keg

brew upgrade --fetch-HEAD supragnosis-server && supragnosis restart   # whenever main moves
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
notes' migrate guidance before returning to stable (`~/.supragnosis/redb` is shared).

## Upgrades

An upgrade is complete only after `brew upgrade` plus a daemon restart - brew upgrade does not
restart a running daemon (the formula caveats print the same reminder), and without the restart the
old daemon keeps running from the deleted keg path. `supragnosis status` and the app's tray line say
so when it happens ("running 0.4.0, this binary 0.4.2"):

```sh
brew upgrade
supragnosis restart     # restarts whichever single manager runs it (login job, brew services, start)
```

Coming from `brew services start supragnosis-server` (before the formula dropped its service
block): the old job keeps running and `supragnosis status` names it. Move to the login job once:

```sh
supragnosis service install --take-over   # retires the brew services job, installs the login job
```

If you installed under the old tokens (formula `supragnosis`, cask `supragnosis-app`),
reinstall. Stopping the old service comes before uninstall - brew uninstall does not clean up a
running service/launchd plist (`supragnosis status` names the old job if one is still loaded):

```sh
brew services stop supragnosis 2>/dev/null
brew uninstall --cask supragnosis-app 2>/dev/null; brew uninstall --formula supragnosis 2>/dev/null
brew install supragnosis
```

Note: the old formula token is NOT forwarded via `formula_renames.json` - that would let the
plain token resolve as a formula name again, and `brew install supragnosis` must resolve to the
cask, not a formula.
