# Documentation

## Design and decisions

- **[Product scope](product.md)** — What RoastMCP is for, who it is for, what 1.0 contains and what it never will
- **[Architecture](architecture.md)** — The daemon, drivers, safety layer, storage, API, MCP, autopilot, twin and app
- **[Safety](safety.md)** — The non-negotiables, the safe state, every failure and its reaction, the link-loss test
- **[Roadmap](roadmap.md)** — Milestones, each with a criterion for going on, and open questions

## Research

- **[Artisan and the Cubean+](research/artisan-and-cubean-plus.md)** — How the first machine talks, and why RoastMCP owns the connection
- **[Preheat control](research/preheat-control.md)** — The Cubean+'s first real preheat, a model of the empty drum, and Artisan's PID

## Architecture Decision Records

- [0001](adr/0001-rust-tauri-stack.md) — Rust and Tauri v2, with a React + TypeScript interface
- [0002](adr/0002-separate-core-process.md) — The roaster is owned by a separate daemon process
- [0003](adr/0003-local-sqlite-with-backups.md) — SQLite on the local disk, with automatic backups elsewhere
- [0004](adr/0004-ai-sets-targets-controller-executes.md) — The AI sets targets; a deterministic controller executes them
- [0005](adr/0005-mcp-first.md) — MCP first, in-app assistant after 1.0
- [0006](adr/0006-machine-profiles.md) — Roasters are described by machine profiles, not code
- [0007](adr/0007-license-and-clean-room.md) — Apache-2.0, and no code from Artisan
- [0008](adr/0008-open-roast-files-and-alog.md) — An open JSON file per roast; Artisan .alog for import and export
- [0009](adr/0009-distribution.md) — Homebrew tap with an ad-hoc signed app
- [0010](adr/0010-platforms.md) — macOS officially, Windows and Linux as a preview
- [0011](adr/0011-real-logs-as-fixtures.md) — Sanitized real roast logs as test fixtures
- [0012](adr/0012-guided-roast-flow.md) — A guided roast as three short steps around one live screen, not a modal wizard
- [0013](adr/0013-bluetooth-rfcomm-on-macos.md) — Bluetooth roasters over a direct RFCOMM channel on macOS
- [0014](adr/0014-green-coffee-inventory-and-roast-plan.md) — Green coffee inventory and the roast plan, the same for app and AI
- [0015](adr/0015-preheat-step-wait-learn.md) — Preheat by step, wait and learn instead of a fast PI loop
- [0016](adr/0016-batch-numbers-and-deleting-roasts.md) — Batch numbers per lot, and deleting roasts into a trash folder
- [0017](adr/0017-profile-designer.md) — Profiles from milestones and a recipe, announced while roasting
- [0018](adr/0018-preheat-air-presence-calm-hold.md) — Preheat at the roast's start air, a presence warning, a calmer hold
