#!/usr/bin/env bash
# Writes Casks/roastmcp.rb for a released macOS bundle (ADR 0009).
set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: $0 <version> <path-to.dmg>" >&2
  exit 1
fi
VERSION="$1"
DMG="$2"
SHA="$(shasum -a 256 "$DMG" | cut -d' ' -f1)"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$ROOT/Casks"
cat > "$ROOT/Casks/roastmcp.rb" <<CASK
cask "roastmcp" do
  version "$VERSION"
  sha256 "$SHA"

  url "https://github.com/huulanka/roastmcp/releases/download/v#{version}/RoastMCP_#{version}_aarch64.dmg"
  name "RoastMCP"
  desc "Coffee roasting software with an AI interface (MCP)"
  homepage "https://github.com/huulanka/roastmcp"

  # Apple Silicon only: the bundle is built for aarch64.
  depends_on arch: :arm64
  depends_on macos: :sonoma

  app "RoastMCP.app"

  zap trash: [
    "~/Library/Application Support/com.andreasbauer.roastmcp",
    "~/Library/Logs/com.andreasbauer.roastmcp",
    "~/Library/Saved Application State/com.andreasbauer.roastmcp.savedState",
  ]

  caveats <<~EOS
    One more step, and it is not optional:

      xattr -dr com.apple.quarantine /Applications/RoastMCP.app

    This build is ad-hoc signed, not notarised, so Gatekeeper refuses it
    until the quarantine attribute is gone. Run the line again after every
    upgrade. macOS may ask again for permissions (for example the
    microphone for first-crack detection) after an upgrade.

    Your data lives in ~/Library/Application Support/com.andreasbauer.roastmcp
    and is not touched by upgrades. Set a backup folder in Settings.
  EOS
end
CASK
echo "Casks/roastmcp.rb -> $VERSION ($SHA)"
