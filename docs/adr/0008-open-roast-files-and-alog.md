# ADR 0008: An open JSON file per roast; Artisan .alog for import and export

## Status
Accepted (2026-11-08)

## Context
Roast data should outlive the program and be readable without it. Users arrive
with archives of Artisan `.alog` files (Python dictionary literals) and use
services that accept them (for example Roastetta).

## Decision
- The database is the working store; every finished roast is also written as a
  **`.roast.json`** file with a documented, versioned schema (metadata, curves,
  events, control log, links to lot, profile and experiment).
- **`.alog` import** for whole archives, and **`.alog` export** per roast for
  compatibility with Artisan and services that read it.
- The JSON schema is published in the repository and covered by tests.

## Consequences
- Data stays readable with any JSON tool, even if RoastMCP disappears.
- `.alog` round-trips are tested against sanitized real files from Artisan 4.x
  and 5.0.
- Two representations to keep consistent; the database is authoritative and
  files are regenerated on change.
