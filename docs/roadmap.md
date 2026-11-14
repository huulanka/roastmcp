# Roadmap

An order, not dates. Each milestone ends with a criterion for going on.
Releases before 1.0 are `0.x`; from M2 on, RoastMCP can replace Artisan for
roasting by hand.

## Status and next steps (2026-11-08)

Done: daemon with safety layer, TC4 driver (serial/Bluetooth) and simulator;
recording with automatic CHARGE/TP/DRY END, `.roast.json` + `.alog`; MCP tools;
desktop app (Tauri) with a global emergency stop and guarded quit; reference
curve and projection; preheat to a target; `.alog` import; the roast-screen
redesign ("Röstfarbe", light and dark) in progress.

Next, in order:

1. **Hardware test, remaining** (`docs/first-roast.md`): done on 2026-11-08 were
   connect, all actuators, reported values, emergency stop, `CLDN,OF`, the
   link-loss test (results in the Cubean+ profile under `[link_loss]`) and AI
   control over MCP (lease after the person's confirmation, burner step and
   rate limits, air limit, human override, emergency stop); still
   open are the preheat changes of
   [ADR 0018](adr/0018-preheat-air-presence-calm-hold.md) (air and target
   from the plan, a presence warning, a calmer hold; first run on the machine
   on 2026-11-09), and the hardware checks for Bluetooth direct ([ADR 0013](adr/0013-bluetooth-rfcomm-on-macos.md));
   when they pass, Bluetooth direct becomes the preselected connection.
2. **UI finish and QA**: done — keys (Q/A burner, W/S air, Enter for the
   next event, Esc stop, Space for presence), RoR warning, burner and air on
   the curve, hover read-out, roast-degree scale, a curve that follows and
   zooms.
3. **Archive**: done — compare two roasts (the other one dashed, both from
   CHARGE, with the differences in duration, first crack, drop, development
   and weight loss), roasted weight and roast colour after the roast, import
   of `.alog` files from the app (file picker or drop). In the daemon and
   for AI clients: comparison lined up on CHARGE or first crack, consistency
   statistics per lot or profile, experiments, ambient conditions, and the
   prompts `debrief_roast` and `plan_next_roast`. In the app: comparison
   lined up on first crack (with development time and the largest gap) and
   consistency per bean or profile, and experiments in the Profiles tab
   (hypothesis, one change, the roasts that test it with their figures and
   tastings, the result).
4. **Store and backup**: done — SQLite database for roasts, inventory,
   profiles and settings; checked backups to a chosen folder after every
   roast and daily; `roastmcp restore`
   ([ADR 0003](adr/0003-local-sqlite-with-backups.md)).
5. **Autopilot (M7)**: built and proven in the simulator — switched on in
   the app, it runs a profile's recipe and trims the burner to keep the roast
   on the profile's curve ([ADR 0020](adr/0020-autopilot-recipe-and-trim.md));
   the simulator is fitted to the Cubean+ with beans. Open: three supervised
   roasts on the machine.
6. **Cupping and care (M4)**: in the daemon and for AI clients — cuppings
   per roast, the rest overview with windows per roast level, maintenance
   tasks with their log and what is due, and the checklist before a roast.
   In the app, the **Pflege** (Care) tab holds the maintenance tasks, their
   log and the checklist; the start dialog shows the checklist and the
   maintenance due on a first page that informs and never holds the roast
   back. The archive shows where each roast is in its rest window (with a
   "ready to drink" filter) and records tastings on two pages: the
   impression, then optional scores from 0 to 10. Open: a restore from a
   backup shown to work on the author's data.
7. M5, M6 and M8 as below; release through the Homebrew tap.

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
- The link-loss test from [safety](safety.md) done for the Cubean+ (2026-11-08).

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

- Green coffee inventory with automatic debiting (brought forward: lots,
  purchases and debits are in the daemon and MCP since ADR 0014, the
  Inventory tab in the app since 1.9); cupping; rest-time reminders.
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

- Controller following a profile: the recipe runner and a trim on the
  burner (ADR 0020); MCP control tools; AI lease; presence confirmation.
- Proven in the simulator (a weaker and a stronger machine, a colder start,
  a takeover by hand), then in supervised real roasts.

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
