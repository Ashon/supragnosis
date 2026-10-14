# Desktop shell cask (signed + notarized Apple silicon .app zip from GitHub Releases - no DMG).
# Owns the plain `supragnosis` token: `brew install supragnosis` resolves here (no formula
# shares the name) and pulls the server formula via depends_on.
# The tap's Casks/supragnosis.rb is rendered from this template by update-tap.sh on every release.
# The shell attaches to (or spawns) the daemon from the supragnosis-server formula found on
# PATH, so the app bundle carries no sidecar binary.
cask "supragnosis" do
  version "0.1.9"
  sha256 "REPLACE_SHA256_APP_ZIP"

  url "https://github.com/Ashon/supragnosis/releases/download/v#{version}/Supragnosis-v#{version}-macos-arm64.app.zip"
  name "Supragnosis"
  desc "Desktop shell for the supragnosis knowledge daemon"
  homepage "https://supragnosis.dev/"

  # Both casks install Supragnosis.app, and brew checks only the conflicts the cask being installed
  # declares, so each side names the other: brew then refuses with the conflict instead of failing
  # on the app bundle already in /Applications.
  conflicts_with cask: "supragnosis-dev"
  depends_on arch: :arm64 # no Intel Mac build, of the app or of the server it depends on
  depends_on formula: "supragnosis-server"
  depends_on :macos

  app "Supragnosis.app"

  # Tray-resident app: brew never quits a running instance on uninstall/upgrade, which
  # leaves the old process serving from a deleted bundle. quit is upgrade-aware (brew
  # reopens the app after the swap).
  uninstall quit: "dev.supragnosis.desktop"

  zap trash: [
    "~/Library/Caches/dev.supragnosis.desktop",
    "~/Library/WebKit/dev.supragnosis.desktop",
  ]
end
