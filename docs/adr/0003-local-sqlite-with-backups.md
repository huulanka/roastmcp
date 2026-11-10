# ADR 0003: SQLite on the local disk, with automatic backups elsewhere

## Status
Accepted (2026-11-08)

## Context
Data must not get lost: years of roasts, inventory and cupping notes. The
author rejected a single database file *on a NAS* for another project
(Hippocampus ADR 0002) because of lock contention and corruption risk on
network storage. RoastMCP is a desktop application that must also work for
community users without any server, and roasting must never depend on the
network.

## Decision
- The database is **SQLite on the local disk**, opened only by the daemon
  (single writer, WAL mode). Never on a network share.
- Every finished roast is additionally written as an open JSON file
  ([ADR 0008](0008-open-roast-files-and-alog.md)).
- **Backups** go to a user-chosen folder, which may be a NAS: after every roast
  and daily, using SQLite's online backup (`VACUUM INTO`), with rotation and an
  integrity check of every backup. Restore is a tested command.
- The storage layer is behind repository traits, so a server database could be
  added later without touching the domain.

## Consequences
- Zero setup for users; works offline; fast.
- The NAS holds copies, not the live database, so a network hiccup can never
  corrupt or block a roast.
- Data from several computers is not merged automatically (out of scope).

## As built (2026-11-10)

- `roast-store` uses `rusqlite` with SQLite compiled in, not `sqlx`: the
  daemon's storage calls are few and synchronous, and this adds no async
  runtime, no build-time database and no newer compiler. One connection
  behind a mutex, WAL, `synchronous = FULL`, foreign keys on.
- The schema is small and document-shaped: `roasts`, `lots`,
  `stock_movements`, `profiles`, `profile_versions`, `settings`. Every row
  holds its `roast-core` value as JSON; real columns exist only for what is
  listed, ordered or referred to. Inventory and profiles are saved whole in
  one transaction (they are small), which replaces the `.bak` files.
- No repository traits yet: the daemon talks to `Store` directly. They can
  come with a second backend.
- Roast files stay ([ADR 0008](0008-open-roast-files-and-alog.md)) and are a
  second net: on start, roast files the database does not know are taken into
  it. That is also the migration of an existing archive.
- `inventory.json`, `profiles.json` and `control.json` of versions up to 1.16
  are read once, stored, and renamed to `*.migrated`.
- Backups: `VACUUM INTO` a partial file in the backup folder, checked with
  `integrity_check` and `foreign_key_check`, then renamed to
  `roastmcp-<UTC time>.sqlite`; the newest 30 stay (plain count instead of the
  daily/weekly/monthly scheme first planned). They run after every saved
  roast and once a day (a failed one is tried again after an hour); the app
  shows the last result and marks the settings button when it failed.
- Restore is `roastmcp restore <file>`: refused while the daemon runs or when
  the file does not pass the check; the previous database is kept as
  `roastmcp.sqlite.before-restore-<time>`. Covered by a test in `roast-store`.
