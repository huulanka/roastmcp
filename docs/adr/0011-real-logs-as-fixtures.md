# ADR 0011: Sanitized real roast logs as test fixtures

## Status
Accepted (2026-11-08)

## Context
The `.alog` importer must handle what Artisan really writes, across versions,
and the digital twin must be calibrated against real roasts. Synthetic files
miss real-world quirks. The author's logs contain personal traces (titles,
notes, UUIDs, file paths).

## Decision
- A small set (4–5) of the author's real logs, covering Artisan 4.x and 5.0 and
  different coffees, is committed under `fixtures/alog/` after **sanitizing**:
  curves, events, weights and machine data are kept; titles, notes, UUIDs,
  operator, organisation and paths are replaced.
- Sanitizing is done by a reproducible tool (`roast-alog`), not by hand.
- Further real logs are used only locally and are never committed.

## Consequences
- Import and twin calibration are tested against reality in CI.
- Contributors can add sanitized logs of their machines the same way.
