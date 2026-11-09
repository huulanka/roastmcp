# Architecture

## Overview

```
                ┌──────────────────────── roastmcp daemon ("roastd") ────────────────────────┐
                │                                                                            │
 roaster ◄────► │ driver ──► session engine ──► safety layer ──► driver                     │
 (serial / BT)  │   ▲           │  curves, RoR, phases,   ▲ arbitration, limits, lease,      │
 simulator ◄──► │   │           │  event detection        │ presence, overheat, e-stop       │
                │   │           ▼                         │                                  │
                │   │       controller (autopilot) ───────┘                                  │
                │   │           ▲ targets                                                    │
 microphone ──► │ first-crack detector                                                       │
                │                                                                            │
                │ store: SQLite + one JSON file per roast + backups                          │
                │ API: HTTP + WebSocket on 127.0.0.1 (token)   MCP: Streamable HTTP /mcp      │
                └───────────▲───────────────────────────────────────▲────────────────────────┘
                            │                                       │
                 desktop app (Tauri v2,                    MCP clients: Claude Code,
                 React + TypeScript)                       Claude Desktop (via `roastmcp mcp`
                                                           stdio bridge), any other client
```

Everything that touches the roaster, the data or the safety rules lives in one
long-running process, the **daemon**. The desktop app and every AI client are
clients of the daemon. If the app crashes or the AI disconnects, the roast is
still under the daemon's control and its safety rules ([ADR 0002](adr/0002-separate-core-process.md)).

## Repository layout

```
Cargo.toml              workspace
crates/
  roast-core            domain types, units, curves, RoR, phases, event detection, metrics
  roast-alog            Artisan .alog reader/writer, fixture sanitizer
  roast-machine         machine profiles (TOML), .aset import
  roast-driver          RoasterDriver trait, TC4 serial driver, transports (serial, Bluetooth direct, mock, replay)
  roast-bluetooth       Bluetooth Classic RFCOMM links on macOS via IOBluetooth (ADR 0013); the only crate with unsafe code
  roast-twin            thermal model (digital twin), simulated driver, calibration
  roast-safety          arbitration, limits, rate limits, AI lease, presence, overheat, e-stop
  roast-control         autopilot controller (profile follower)
  roast-audio           microphone capture, first-crack detector
  roast-store           SQLite schema and repositories, roast files, backups
  roast-api             API and event types shared by daemon, app and MCP (exported to TypeScript)
  roastd                the daemon: session engine, HTTP/WebSocket API, MCP server
  roastmcp              the command line: `roastmcp daemon | mcp | import | sim | aset`
app/                    desktop app: Vite + React + TypeScript, src-tauri/ (Tauri v2)
machines/               machine profiles shipped with the app (community contributions welcome)
fixtures/               sanitized real roast logs used by tests
docs/                   documentation, ADRs, research
```

## The daemon

- Started by the app (bundled as a sidecar binary, `roastmcp daemon`) or by
  hand. One instance per user, enforced by a lock file in the data directory.
- Listens on `127.0.0.1` only. A random token is generated at start and written,
  together with the port and PID, to `daemon.json` in the data directory
  (permissions `0600`). Every HTTP, WebSocket and MCP request needs the token.
- Keeps running while a roast is active even if every client disconnects. When
  idle (no roast, no clients) it exits after a configurable time.

Data directory: `~/Library/Application Support/com.andreasbauer.roastmcp/` on
macOS, the platform equivalents elsewhere (`directories` crate).

### Session engine

One sampling loop per connected machine, at the machine profile's interval
(TC4 default 1.0 s):

1. Read a sample from the driver (bean temperature, optional environment
   temperature, actual actuator values if the machine reports them).
2. Append to the curve; compute rate of rise (robust smoothing, see below),
   phase, projections, deviation from the reference curve.
3. Run event detection (CHARGE, turning point, DRY END by temperature, DROP)
   and the first-crack detector's latest verdict.
4. Run the safety checks (overheat, presence, lease, link health).
5. If the autopilot is active, let the controller propose commands.
6. Pass every proposed command through the safety layer to the driver.
7. Publish sample, events, alarms and state over the WebSocket.

**Roast state machine:** `Disconnected → Connected (monitoring/preheat) →
Recording → Charged → Dropped → Cooling → Finished`. Events are marked on a
recording; leaving `Finished` persists the roast.

**Rate of rise** is computed as the slope of a least-squares line over a sliding
window (default 30 s) of a lightly filtered bean temperature, reported per
minute. Spikes are rejected before fitting (median filter + plausibility
bound). Parameters are configurable per machine profile.

### Commands and the safety layer

Every change to an actuator is a `CommandRequest { actuator, value, source,
reason }` with `source ∈ {Human, Safety, Autopilot, Ai}`. The safety layer
decides, in this order:

1. **Latched states.** After an emergency stop, only `Human` and `Safety`
   commands are accepted until a human resets.
2. **Arbitration.** `Human` and `Safety` always win. A `Human` command while the
   AI or the autopilot is in control suspends AI control until a human resumes
   it.
3. **Limits.** Range per actuator; maximum step and maximum rate of change,
   with tighter limits for `Ai` and `Autopilot` than for `Human`. A request
   beyond the step limit is clamped, not rejected, and the clamp is reported.
4. **Lease and presence** for `Ai` and `Autopilot` sources (see
   [safety](safety.md)).

Accepted commands go to the driver; every request, accepted or not, goes into
the audit log and appears as a control event on the roast curve.

## Drivers

```rust
trait RoasterDriver {
    fn capabilities(&self) -> &Capabilities;        // channels, actuators, ranges, events
    async fn connect(&mut self) -> Result<()>;
    async fn read(&mut self) -> Result<RawSample>;   // one sampling step
    async fn apply(&mut self, cmd: &ActuatorCommand) -> Result<()>;
    async fn safe_state(&mut self) -> Result<()>;    // machine-specific safe state
    async fn disconnect(&mut self) -> Result<()>;
}
```

- **TC4 serial driver**, configured entirely by a machine profile: transport
  (serial path, baud rate), initialisation sequence (`CHAN;…`, `UNITS;C`,
  `FILT;…`), the `READ` response layout (which field is bean temperature,
  environment temperature, burner, air…), command templates (`OT1,{}`), ranges
  and the safe state. A profile can be imported from an Artisan `.aset`.
- **Transports** behind a trait: real serial port (`serialport` crate; Bluetooth
  SPP appears as a serial port on macOS, Windows and Linux), an in-memory mock,
  and a replay transport that plays back a recorded protocol transcript.
  Tests never need hardware.
- **Simulated driver** backed by the digital twin.

Machine profiles live in `machines/` as TOML, one file per machine, and are the
main way the community adds roasters ([ADR 0006](adr/0006-machine-profiles.md)).

## Storage

SQLite on the local disk, owned exclusively by the daemon (`sqlx`, migrations in
`crates/roast-store/migrations`) ([ADR 0003](adr/0003-local-sqlite-with-backups.md)).

Main tables: `machines`, `green_lots`, `stock_movements`, `profiles`,
`profile_versions`, `experiments`, `roasts`, `roast_curves` (columnar arrays per
roast), `roast_events`, `control_log` (audit), `cuppings`, `maintenance_tasks`,
`maintenance_log`, `settings`.

Each finished roast is also written as an open, documented JSON file
(`roasts/<year>/<date>_<time>_<slug>.roast.json`) and can be exported as
`.alog` ([ADR 0008](adr/0008-open-roast-files-and-alog.md)).

**Backups:** after every roast and daily, the database is copied with SQLite's
online backup (`VACUUM INTO`) together with new roast files into the backup
folder (for example a NAS share), with rotation (14 daily, 8 weekly, 12
monthly). Every backup is opened and checked (`PRAGMA integrity_check`) after
writing. Restore is a documented, tested command.

## API

- HTTP JSON under `/api/v1/…`: status, machines, connect/disconnect, recording,
  events, commands, emergency stop, presence, autopilot, and CRUD for archive,
  profiles, experiments, inventory, cupping, maintenance, settings.
- WebSocket `/api/v1/live`: samples, events, alarms, state changes, command
  results.
- Types are defined once in `roast-api` and exported to TypeScript (`ts-rs`)
  for the app.

## MCP

Served by the daemon over Streamable HTTP at `/mcp` (token required) using the
official Rust SDK (`rmcp`). `roastmcp mcp` is a stdio bridge for clients that
only speak stdio (Claude Desktop) ([ADR 0005](adr/0005-mcp-first.md)).

Initial tools:

| Tool | Purpose |
|---|---|
| `get_status` | Machine, connection, roast state, latest values, alarms, who is in control |
| `get_curve` | Current roast curve (downsampled), events, phases, deviation from reference |
| `mark_event` | CHARGE, DRY END, FC/SC start/end, DROP (subject to safety rules) |
| `set_burner` / `set_air` / `set_drum` | Actuator changes through the safety layer, with a reason |
| `emergency_stop` | Immediate safe state |
| `start_autopilot` / `adjust_autopilot` / `stop_autopilot` | Run a profile; change targets |
| `list_roasts` / `get_roast` / `compare_roasts` | Archive access and analysis |
| `list_profiles` / `get_profile` / `create_profile` / `add_profile_version` | Profile development: milestones, recipe, a new version per change |
| `set_reference` | A roast or a profile as the plan line; `get_status` then shows the deviation and the next recipe step |
| `record_experiment` / `add_roast_note` | Hypotheses and observations |
| `list_lots` / `add_lot` / `add_purchase` | Green coffee inventory (ADR 0014) |
| `start_recording` | The start dialog: lot, green weight, goal, reference, targets |
| `get_cuppings` | Tasting data |

Prompts: `debrief_roast`, `plan_next_roast`. Every AI tool call renews the AI
lease; read-only tools never require one.

## Preheat

`roast-control`'s `PreheatController` brings the empty drum to a target by
step, wait and learn ([ADR 0015](adr/0015-preheat-step-wait-learn.md)): full
heat, one step to the holding power before the target, then rare corrections
after a settle time, outside a dead band. The daemon feeds it each reading and
sends its decisions as source `Autopilot`; what it learns per machine and air
setting is kept in `control.json`.

## Profiles

A profile ([ADR 0017](adr/0017-profile-designer.md)) is a versioned plan: a
target curve, dry end / first crack / drop targets and a recipe of burner and
air steps, kept in `profiles.json`. The app's designer starts one from a roast
and warps its curve when a milestone moves; a roast that follows a version
shows its curve as the plan line, and the app announces the next recipe step
for the person to set.

## Autopilot

Two levels ([ADR 0004](adr/0004-ai-sets-targets-controller-executes.md)):

- The **controller** (`roast-control`) runs inside the daemon on every sample.
  It follows a target profile (bean temperature and rate of rise over time,
  plus planned air and drum changes), using feed-forward from the digital twin
  and a PI correction on the rate-of-rise error. Its output goes through the
  safety layer with source `Autopilot`.
- The **AI** chooses and adjusts the target profile, comments, and decides on
  critical steps, which always need a human confirmation (start, DROP, large
  changes). Machines that drop by hand get a "drop now" cue instead.

## Digital twin

A lumped thermal model (`roast-twin`): heat source → drum/air → beans, with
losses to ambient, moisture evaporation during drying, the exothermic phase
after first crack and a first-order probe lag. Inputs are burner, air and drum
values and batch size; parameters are fitted per machine from recorded roasts
(least squares, Nelder–Mead via `argmin`). Uses: simulated driver, projections,
autopilot feed-forward and testing, training mode, what-if runs.

The **3D view** (three.js through React Three Fiber) is driven by the same data,
live or simulated: drum rotation from drum speed, bean colour from bean
temperature and development, heater glow from burner, airflow from air.

## First-crack detection

`roast-audio` captures a microphone (`cpal`), resamples to 16 kHz mono and runs a
detector on short windows: a baseline transient detector (spectral flux in the
crack band against the stationary noise of fan and drum) and, where licensing
permits, an optional ONNX classifier (`ort`). A crack is confirmed after several
positive windows; the app shows a live "crack meter" so the signal is visible,
not only audible. First-crack prediction uses the twin and the reference curve.

## Desktop app

Tauri v2 with Vite, React, TypeScript, Tailwind CSS and shadcn/ui; uPlot for the
live curves; React Three Fiber for 3D; i18next for English and German; TanStack
Query for API state. The app talks to the daemon directly over HTTP and
WebSocket; its Rust side starts the daemon sidecar and hands the token to the
web view.

How the shell works (`app/src-tauri`):

- On start it reuses a daemon that answers on the address in `daemon.json`, or
  starts the bundled `roastmcp daemon` (port 7878, the UI from the bundle's
  resources, output to `logs/daemon.log`). A daemon it started and that dies is
  started again.
- The window first shows a local splash page, then loads the UI the daemon
  serves (`http://127.0.0.1:<port>/#token=…`), so the app and a browser tab run
  the same code. The web view needs no Tauri IPC.
- A global shortcut (⌘⇧⎋ on macOS, Ctrl+Alt+Esc elsewhere) sends the
  emergency stop from Rust straight to the daemon, independent of the web view
  and of which app has focus.
- Quitting (⌘Q, closing the window) asks first when a roast is recorded or the
  burner is on. Quitting stops a daemon the app started through
  `POST /api/shutdown`, which puts the roaster into the safe state and saves
  the recording; ways macOS quits without asking (Dock, logout) do the same.
  A daemon started by hand keeps running.
