# Rolling dev-channel cask for the desktop shell: installs the `dev` pre-release asset that
# .github/workflows/dev-app.yml republishes from main (signed/notarized exactly like a
# release build). `version :latest` + `sha256 :no_check` because the artifact rolls under a
# single URL - `brew upgrade` does not track it, so refresh with
# `brew reinstall supragnosis-dev`. Conflicts with the stable cask (same app bundle); pair
# with `brew install --HEAD supragnosis-server` for a full dev stack (the viewer UI itself
# lives in the server binary).
cask "supragnosis-dev" do
  version :latest
  sha256 :no_check

  url "https://github.com/Ashon/supragnosis/releases/download/dev/Supragnosis-dev-macos-universal.app.zip"
  name "Supragnosis (dev channel)"
  desc "Desktop shell for the supragnosis knowledge daemon - rolling dev build"
  homepage "https://supragnosis.dev/"

  conflicts_with cask: "supragnosis"
  depends_on formula: "supragnosis-server"

  app "Supragnosis.app"

  # Tray-resident app: quit the running instance around uninstall/upgrade, same as the
  # stable cask (brew reopens it after a swap).
  uninstall quit: "dev.supragnosis.desktop"

  zap trash: [
    "~/Library/Caches/dev.supragnosis.desktop",
    "~/Library/WebKit/dev.supragnosis.desktop",
  ]
end
