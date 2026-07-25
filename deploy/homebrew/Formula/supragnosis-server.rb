# supragnosis server/CLI formula (prebuilt release binaries; keyword + hashing search -
# build from source with --features fastembed for local semantic search).
# Lives in the tap repo as Formula/supragnosis-server.rb; update-tap.sh rewrites version/sha256
# per release from this template. Installs the plain `supragnosis` binary - only the brew token
# carries the -server suffix (the desktop-app cask owns the plain `supragnosis` token).
class SupragnosisServer < Formula
  desc "Embedded MCP server that grows an ontology from working knowledge"
  homepage "https://supragnosis.dev/"
  version "0.1.9"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/Ashon/supragnosis/releases/download/v#{version}/supragnosis-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_SHA256_AARCH64_APPLE_DARWIN"
    else
      url "https://github.com/Ashon/supragnosis/releases/download/v#{version}/supragnosis-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_SHA256_X86_64_APPLE_DARWIN"
    end
  end

  on_linux do
    url "https://github.com/Ashon/supragnosis/releases/download/v#{version}/supragnosis-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "REPLACE_SHA256_X86_64_UNKNOWN_LINUX_GNU"
  end

  # Dev channel: `brew install --HEAD supragnosis-server` builds current main from source
  # (rust toolchain pulled as a build dep; default features = keyword search, same as the
  # release binaries). Refresh an installed HEAD with `brew upgrade --fetch-HEAD`.
  head do
    url "https://github.com/Ashon/supragnosis.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args(path: "crates/supragnosis-cli")
    else
      bin.install "supragnosis"
    end
  end

  # brew services start supragnosis-server
  # `serve --http` also brings up the viewer unix socket at ~/.supragnosis/viz.sock by
  # default, which is what the desktop app (cask supragnosis) attaches to.
  service do
    run [opt_bin/"supragnosis", "serve", "--http", "127.0.0.1:7373"]
    keep_alive true
    log_path var/"log/supragnosis.log"
    error_log_path var/"log/supragnosis.err.log"
  end

  test do
    assert_match "supragnosis", shell_output("#{bin}/supragnosis --help")
  end
end
