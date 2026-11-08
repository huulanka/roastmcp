# ADR 0009: Homebrew tap with an ad-hoc signed app

## Status
Accepted (2026-11-08)

## Context
Installation should be quick on the Mac, ideally `brew install`. Since
1 September 2026 Homebrew no longer supports casks that fail Gatekeeper and has
removed `--no-quarantine`; passing Gatekeeper requires signing and notarisation
with a paid Apple Developer account. The author's Hippocampus project already
ships an ad-hoc signed app through its own tap without that account.

## Decision
- The repository is its own **Homebrew tap** (`Casks/roastmcp.rb`), like
  Hippocampus.
- The macOS app is **ad-hoc signed, not notarised**. The cask's caveats tell the
  user to run `xattr -dr com.apple.quarantine /Applications/RoastMCP.app` after
  every install or upgrade.
- Releases are built by CI and attached to GitHub Releases; Windows and Linux
  packages are attached as previews.

## Consequences
- No yearly fee; one manual command per install/upgrade.
- Not eligible for the official Homebrew cask repository.
- Permissions granted by macOS (for example microphone) may be requested again
  after an upgrade because the ad-hoc signature changes; this is documented.
