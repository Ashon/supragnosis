# Homebrew distribution (formula + cask, no DMG)

This directory is the source of the tap repo's formula and casks. The tap holds rendered output:
every release overwrites its copies from these templates, so a change is made here, never in the
tap. Contents:

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
- `Casks/supragnosis-dev.rb` - the rolling dev-channel cask (`version :latest`), copied as-is.
- `update-tap.sh` - after a release, renders the formula and casks into the tap: copies the
  templates, fills in the version and the sha256 sums from the release assets' .sha256 sidecar
  files, writes the formula's bottle block from the release's bottles, and fails if a placeholder
  or the template's own version survives.
- `bottle.sh` - bottles the formula for a released tag and proves the bottle pours (Bottles,
  below).

## Bottles

The formula only copies the release's prebuilt binary, but Homebrew treats a formula without a
bottle as a source build. Before installing one it requires an up-to-date Xcode or Command Line
Tools, and fails without them, even though nothing is compiled. That is how installing the desktop
app, whose cask depends on the formula, came to ask for Xcode. A bottle is poured instead, with no
such check.

- **The release builds them.** The release.yml bottle job runs `bottle.sh` on macos-14 and
  ubuntu-22.04:
  - it installs the rendered formula from a local tap with `--build-bottle`;
  - `brew bottle` makes the bottle;
  - it reinstalls from that bottle and requires that Homebrew poured it.
  The job attaches the bottle to the release and hands its JSON to the tap job, where
  `update-tap.sh` writes the `bottle do` block. A bottle that fails to build is left out, and that
  platform keeps the source-build path it has without one.
- **Which machines pour one:**
  - Apple silicon on macOS 14 or later (`arm64_sonoma`). Homebrew pours a bottle built for an older
    macOS on every newer one, which is why it is built on the oldest arm64 runner.
  - x86_64 Linux (`x86_64_linux`). On a distribution whose glibc is older than Homebrew's own
    minimum, Homebrew installs its glibc and gcc beside any formula it installs - on ubuntu-22.04
    that was twelve formulae before this bottle poured. The bottle does not change that; it only
    spares the source-build checks.
  - Intel Macs do not: there is no Intel runner to build on, since the release cross-compiles the
    Intel binary. They keep the source-build path, so an outdated Command Line Tools still stops the
    install there (`xcode-select --install`, or Software Update).
- **Pull requests check it.** `.github/workflows/homebrew.yml` runs `bottle.sh` against the latest
  release whenever this directory changes, and weekly, so a change to the formula or to Homebrew
  that breaks bottling shows up before a release depends on it.

## One-time setup

1. Create the tap repo: make `Ashon/homebrew-tap` (public) on GitHub. The first release's tap job
   (or a manual run of `update-tap.sh`, below) writes `Formula/` and `Casks/` into it.
2. From the next `v*` tag on, the release carries `Supragnosis-v<ver>-macos-universal.app.zip`.

## Per release

```sh
git clone git@github.com:Ashon/homebrew-tap && cd homebrew-tap
../supragnosis/deploy/homebrew/update-tap.sh v0.1.11 . [dir-with-the-release's-*.bottle.json]
git add Formula Casks && git commit -m "supragnosis v0.1.11" && git push
```

Without the bottle JSONs (the bottle job's artifacts) the formula is rendered without a bottle
block, which installs, but only where Xcode or the Command Line Tools are current.

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
