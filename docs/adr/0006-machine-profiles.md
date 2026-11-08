# ADR 0006: Roasters are described by machine profiles, not code

## Status
Accepted (2026-11-08)

## Context
RoastMCP should support many roasters, not one. Many home roasters and
controller mods speak variants of the TC4 serial protocol; they differ in
channel layout, command names, ranges and quirks. Artisan users describe their
machine in `.aset` settings files. The author has one machine to test with.

## Decision
- A **machine profile** (TOML, in `machines/`) describes a machine completely:
  transport, initialisation, `READ` layout, actuators with command templates
  and ranges, extras (cooling, filter), sampling interval, safe state, limits,
  temperature limit, and the tested link-loss behaviour.
- The **TC4 driver** is generic and driven by the profile.
- An **importer** turns an Artisan `.aset` (TC4 device) into a draft profile.
- Other protocols get their own drivers behind the `RoasterDriver` trait.

## Consequences
- Adding a TC4-family roaster is a pull request with a TOML file and a recorded
  protocol transcript — no Rust needed.
- Profiles are versioned and reviewable; safety-relevant fields (safe state,
  limits) are visible in review.
- Machines outside the TC4 family need driver code.
