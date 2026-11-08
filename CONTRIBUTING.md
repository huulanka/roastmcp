# Contributing to RoastMCP

Thank you for helping. RoastMCP controls a heat source, so contributions are
held to a few extra rules on top of the usual ones.

## Ground rules

- Read [docs/safety.md](docs/safety.md) before touching anything that can
  change an actuator. Pull requests that weaken the safety layer are declined.
- **No code from Artisan.** Artisan is AGPLv3; RoastMCP is Apache-2.0. Learn
  from it, do not copy or port it ([ADR 0007](docs/adr/0007-license-and-clean-room.md)).
- By contributing you agree that your contribution is licensed under the
  Apache License 2.0.

## Adding a roaster

Most TC4-family roasters need no Rust at all:

1. Create `machines/<vendor>-<model>.toml` (start from an existing profile, or
   import your Artisan settings: `roastmcp aset import <file.aset>`).
2. Record a protocol transcript of a short session (`roastmcp record`) and add
   it under `fixtures/transcripts/`.
3. Fill in the safe state, limits and the link-loss test result
   ([test protocol](docs/safety.md#what-the-hardware-does-when-the-link-drops)).
4. Open a pull request titled `feat(machines): <vendor> <model>`.

You can also contribute sanitized roast logs of your machine
(`roastmcp alog sanitize`), which improve import tests and twin calibration.

## Development

See [CLAUDE.md](CLAUDE.md) for the commands. Before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && npm run build
```

## Pull requests

- One topic per pull request; tests for new behaviour.
- The title follows [Conventional Commits](https://www.conventionalcommits.org)
  (`feat:`, `fix:`, `docs:` …); it becomes the squash commit and drives the
  release version.
- Significant design changes come with an ADR in `docs/adr/`.
