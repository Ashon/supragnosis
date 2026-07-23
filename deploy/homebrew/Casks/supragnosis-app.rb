# Desktop shell cask (signed + notarized universal .app zip from GitHub Releases - no DMG).
# Lives in the tap repo as Casks/supragnosis-app.rb; update-tap.sh rewrites version/sha256.
# The shell attaches to (or spawns) the daemon from the supragnosis formula found on PATH,
# so the app bundle carries no sidecar binary.
cask "supragnosis-app" do
  version "0.1.9"
  sha256 "REPLACE_SHA256_APP_ZIP"

  url "https://github.com/Ashon/supragnosis/releases/download/v#{version}/Supragnosis-v#{version}-macos-universal.app.zip"
  name "Supragnosis"
  desc "Desktop shell for the supragnosis knowledge daemon"
  homepage "https://supragnosis.dev/"

  depends_on formula: "supragnosis"

  app "Supragnosis.app"

  zap trash: [
    "~/Library/Caches/dev.supragnosis.desktop",
    "~/Library/WebKit/dev.supragnosis.desktop",
  ]
end
