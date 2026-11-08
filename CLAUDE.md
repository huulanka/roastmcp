# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

RoastMCP is open-source coffee roasting software: a Rust daemon that owns the
roaster connection, the safety layer and the database; a Tauri v2 desktop app
(React + TypeScript); and an MCP server so AI clients can read, coach and —
under guardrails — control a roast. Start with `docs/architecture.md` and
`docs/safety.md`.

## Safety invariants — never break these

- Every actuator change goes through `roast-safety`. No code path may call a
  driver's `apply` for an actuator except the safety layer, and `safe_state`
  except the safety layer and the emergency stop.
- `Human` and `Safety` commands always win over `Ai` and `Autopilot`. A human
  command suspends AI control.
- The safe state is burner 0, air high, drum running — never "everything off".
- The emergency stop must not depend on the AI, MCP or the web view.
- AI/autopilot control requires a valid lease and recent presence confirmation;
  only the app UI can confirm presence, never an AI tool.
- **Never send commands to a real roaster** from tests, scripts or agents.
  Hardware is only touched with a person present who confirmed it explicitly.
  Use the simulator, the mock transport or recorded transcripts.

## Licence hygiene

Apache-2.0. Do not copy, translate or closely paraphrase code from Artisan
(AGPLv3). Reading it for protocol or file-format facts is fine; document facts
in your own words with sources in `docs/research/`.

## Layout

Cargo workspace in `crates/` (see `docs/architecture.md` for each crate), the
desktop app in `app/` (`app/src-tauri` is a workspace member), machine profiles
in `machines/`, sanitized fixtures in `fixtures/`. Never commit unsanitized
personal roast logs.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd app && npm ci && npm run build      # frontend
cd app && npm run tauri dev             # desktop app in development
cargo run -p roastmcp -- daemon --simulator   # daemon with the simulator
```

TypeScript API types are generated from `roast-api` (`cargo test -p roast-api
export_bindings`) into `app/src/api/generated/`; regenerate and commit after
changing API types.

## Conventions

- English for code, comments, docs and UI strings (German via i18n).
- Pull requests are squash-merged; the PR title becomes the commit on `main` and
  follows Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:` …).
  `feat` → minor, `fix` → patch, `BREAKING CHANGE:` → major; releases are
  automated. Commit subjects read as descriptive prose.
- Significant decisions get an ADR in `docs/adr/`; keep `docs/` in step with
  the code.
- Warnings are errors in CI (`clippy -D warnings`). No `unwrap()` in the daemon
  outside tests; errors are typed (`thiserror`) in libraries and contextual
  (`anyhow`) in binaries.
