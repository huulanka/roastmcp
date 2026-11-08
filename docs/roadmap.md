# Roadmap

An order, not dates. Each milestone ends with a criterion for going on.
Releases before 1.0 are `0.x`; from M2 on, RoastMCP can replace Artisan for
roasting by hand.

## M0: foundations

- Workspace with all crates, the desktop app shell, CI on macOS, Windows and
  Linux (fmt, clippy with warnings as errors, tests, frontend build),
  Conventional Commits and automated releases.
- Domain types, `.alog` reader/writer, sanitized fixtures from real roasts.
- Machine profiles and `.aset` import; the Skywalker Cubean+ profile.
- Simulator v0 (simple thermal model) and the simulated driver.

*Go on when:* CI is green on all three platforms and the simulator produces a
plausible roast curve end to end.

## M1: read along

- TC4 serial driver with serial, mock and replay transports.
- Daemon: session engine, recording, RoR, phases, event detection (CHARGE,
  turning point, DRY END, DROP), storage, roast files, `.alog` export.
- HTTP/WebSocket API; app: connect, live curve, events, recording.
- MCP: `get_status`, `get_curve`, `list_roasts`, `get_roast`.

*Go on when:* three real roasts recorded end to end with RoastMCP reading the
Cubean+, with curves that match what the roaster and Artisan showed.

## M2: control by hand, safely

- Controls (sliders, step buttons, configurable keys), alarms, sound cues.
- Safety layer complete: limits, arbitration, emergency stop with latch,
  overheat guard, link watchdog, audit log as roast events.
- The link-loss test from [safety](safety.md) done for the Cubean+.

*Go on when:* five roasts done with RoastMCP alone, no Artisan; safety tests and
simulator fault injection pass.

## M3: archive and profile development

- Import of existing `.alog` archives; archive with search, overlay and
  comparison aligned on CHARGE or FC; consistency statistics.
- Reference curve with live ΔBT/ΔRoR and projection.
- Profiles with versions; experiments; roast degree and ambient conditions.
- MCP: `compare_roasts`, profiles, experiments, notes; prompts
  `debrief_roast` and `plan_next_roast`.

*Go on when:* the author's 23 Artisan roasts are imported and an AI debrief of a
real roast is useful.

## M4: inventory, cupping, care

- Green coffee inventory with automatic debiting; cupping; rest-time reminders.
- Maintenance log and reminders; pre-roast safety checklist.
- Backups to a chosen folder with rotation and integrity check; tested restore.

*Go on when:* a restore from backup has demonstrably worked.

## M5: digital twin

- Thermal model with calibration from recorded roasts; the Cubean+ calibrated.
- What-if runs and training mode in the simulator.
- 3D view of the drum, live and simulated.

*Go on when:* the calibrated twin predicts bean temperature of held-out real
roasts within a stated error.

## M6: hearing first crack

- Microphone capture, first-crack detector, crack meter in the app,
  first-crack prediction.

*Go on when:* first crack detected within ±10 s of the hand-marked time in most
of a set of recorded roasts.

## M7: autopilot

- Controller following a profile; MCP control tools; AI lease; presence
  confirmation; confirmation of critical steps.
- Proven in the simulator with fault injection, then in supervised real roasts.

*Go on when:* three supervised roasts driven by the autopilot within the
profile's tolerance, with no safety violation.

## M8: onboarding and 1.0

- Setup wizard, guided roasting with starter profiles, glossary and
  explanations, German translation.
- Homebrew tap, Windows and Linux preview packages, user documentation.

*Go on when:* someone else installs it and roasts with it without help.

## After 1.0

1. In-app AI assistant.
2. Then, order open: enjoyment (brewing, blind cupping per SCA CVA),
   management (costs, blends), community and sensors.

## Open questions

- Behaviour of the Cubean+ when the Bluetooth link drops (test protocol in
  [safety](safety.md)).
- Whether the Cubean+ panel can be used while the computer is connected (M1
  read-along without Artisan).
- Licence of existing first-crack models; otherwise our own detector only.
