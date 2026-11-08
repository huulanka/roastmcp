#!/usr/bin/env bash
# Builds the `roastmcp` binary (the daemon, the MCP bridge and the CLI) and
# places it where Tauri expects an external binary: app/src-tauri/binaries/
# roastmcp-<target-triple>[.exe] (see "externalBin" in tauri.conf.json).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
PROFILE="${1:-release}"
EXT=""
case "$TRIPLE" in *windows*) EXT=".exe" ;; esac

if [ "$PROFILE" = "release" ]; then
  cargo build --release -p roastmcp
else
  cargo build -p roastmcp
fi

mkdir -p app/src-tauri/binaries
cp "target/$PROFILE/roastmcp$EXT" "app/src-tauri/binaries/roastmcp-$TRIPLE$EXT"
echo "sidecar: app/src-tauri/binaries/roastmcp-$TRIPLE$EXT"
