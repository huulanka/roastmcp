#!/usr/bin/env bash
# Sets the release version everywhere it is written down. Called by
# semantic-release (see .releaserc.json); never bump versions by hand.
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "usage: $0 <version>   (e.g. 0.2.0, no leading v)" >&2
  exit 1
fi
VERSION="$1"
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: '$VERSION' is not a plain semver (X.Y.Z)" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# perl -pi works the same on macOS and Linux, unlike sed -i.
set_toml_version() {
  perl -pi -e "s/^version = \"\d+\.\d+\.\d+\"/version = \"$VERSION\"/" "$1"
  echo "  $1"
}
set_json_version() {
  perl -pi -e "s/\"version\": \"\d+\.\d+\.\d+\"/\"version\": \"$VERSION\"/" "$1"
  echo "  $1"
}

echo "Setting version to $VERSION in:"
set_toml_version Cargo.toml
[ -f app/src-tauri/Cargo.toml ] && set_toml_version app/src-tauri/Cargo.toml
[ -f app/package.json ] && set_json_version app/package.json
[ -f app/src-tauri/tauri.conf.json ] && set_json_version app/src-tauri/tauri.conf.json

cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null
[ -f app/src-tauri/Cargo.toml ] && (cd app/src-tauri && (cargo update --workspace --offline >/dev/null 2>&1 || cargo update --workspace >/dev/null))
[ -f app/package.json ] && (cd app && npm install --package-lock-only --no-audit --no-fund >/dev/null)
echo "done"
