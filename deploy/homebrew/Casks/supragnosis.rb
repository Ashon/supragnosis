# Desktop shell cask (signed + notarized universal .app zip from GitHub Releases - no DMG).
# Owns the plain `supragnosis` token: `brew install supragnosis` resolves here (no formula
# shares the name) and pulls the server formula via depends_on.
# Lives in the tap repo as Casks/supragnosis.rb; update-tap.sh rewrites version/sha256.
# The shell attaches to (or spawns) the daemon from the supragnosis-server formula found on
# PATH, so the app bundle carries no sidecar binary.
cask "supragnosis" do
  version "0.1.9"
  sha256 "REPLACE_SHA256_APP_ZIP"

  url "https://github.com/Ashon/supragnosis/releases/download/v#{version}/Supragnosis-v#{version}-macos-universal.app.zip"
  name "Supragnosis"
  desc "Desktop shell for the supragnosis knowledge daemon"
  homepage "https://supragnosis.dev/"

  depends_on formula: "supragnosis-server"

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
