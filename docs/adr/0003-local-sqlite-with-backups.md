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
