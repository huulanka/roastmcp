# ADR 0010: macOS officially, Windows and Linux as a preview

## Status
Accepted (2026-11-08)

## Context
The community roasts on all three desktop platforms, but the author can only
test on a Mac with one roaster. Serial and Bluetooth behave differently per
platform.

## Decision
- The code stays platform-neutral; CI builds and tests on macOS, Windows and
  Linux from the first milestone.
- macOS is **officially supported**. Windows and Linux packages are published
  as a **preview** until community members confirm them with real hardware.

## Consequences
- Platform bugs surface early in CI, without promising support we cannot test.
- The preview label is removed per platform once confirmed.
