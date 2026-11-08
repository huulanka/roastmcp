# Daemon API (v0)

The daemon (`roastmcp daemon`) listens on `127.0.0.1` only. On start it writes
`daemon.json` (mode 0600) into the data directory
(`~/Library/Application Support/com.andreasbauer.roastmcp/` on macOS):

```json
{ "port": 7878, "token": "…", "pid": 12345, "version": "0.1.0" }
```

Every `/api` request needs `Authorization: Bearer <token>`; the WebSocket takes
`?token=<token>`. The daemon also serves the web UI at `/`; it prints
`http://127.0.0.1:7878/#token=<token>` on start, and the UI reads the token from
the URL fragment (and keeps it in `sessionStorage`).

Errors are HTTP 4xx/5xx with `{ "error": "message" }`. All temperatures are °C,
times are seconds, actuator values percent (switches 0 or 100). `t` is seconds
since the recording (or the preview) started; the roast clock shown to people
is `t - charge_t`.

## Types

```ts
type Actuator = "burner" | "air" | "drum" | "cooling" | "filter";
type EventKind = "charge" | "turning_point" | "dry_end" | "first_crack_start"
  | "first_crack_end" | "second_crack_start" | "second_crack_end" | "drop" | "cool_end";
type EventOrigin = "human" | "auto" | "detector" | "ai" | "import";
type RoastState = "disconnected" | "connected" | "recording" | "charged"
  | "dropped" | "cooling" | "finished";
type RejectReason = "emergency_stop_latched" | "ai_suspended_by_human" | "no_lease"
  | "presence_required" | "out_of_range" | "rate_limited" | "not_supported"
  | "disconnected" | "overheat";
type CommandOutcome =
  | { outcome: "applied"; value: number; clamped_from: number | null }
  | { outcome: "rejected"; reason: RejectReason }
  | { outcome: "awaiting_confirmation"; confirmation_id: string };

interface RoastEvent { t: number; kind: EventKind; origin: EventOrigin; bt: number | null }
interface Point { t: number; bt: number | null; et: number | null; ror: number | null;
                  actuators: Partial<Record<Actuator, number>> }
interface Phases { drying_s: number; maillard_s: number | null; development_s: number | null;
                   total_s: number; dtr_pct: number | null }
interface SafetyStatus {
  emergency_stop: boolean; ai_suspended: boolean;
  lease_holder: string | null; lease_remaining_s: number | null;
  presence_due_in_s: number | null;   // negative = overdue
  overheat: boolean; connected: boolean;
  actuators: Partial<Record<Actuator, number>>;  // last commanded/reported values
}
interface Alarm { id: number; at: string /* RFC 3339 */; severity: "info" | "warning" | "critical";
                  kind: string; message: string }
interface ActuatorSpec { actuator: Actuator; min: number; max: number; step: number }
interface MachineInfo { id: string; name: string; simulated: boolean; port: string | null;
  actuators: ActuatorSpec[]; switches: Actuator[]; has_et: boolean;
  sampling_interval_s: number; max_bt_c: number; link_loss_tested: boolean;
  link_lost: string | null }  // the profile's advice, once tested

interface Snapshot {
  version: string;
  machine: MachineInfo | null;           // null when disconnected
  connection: "disconnected" | "connecting" | "connected" | "error";
  connection_error: string | null;
  state: RoastState;
  recording: { id: string; title: string; started_at: string;
               green_weight_g: number | null; beans: string | null } | null;
  t: number | null;                      // time of the latest point
  charge_t: number | null;
  bt: number | null; et: number | null; ror: number | null;
  events: RoastEvent[];
  phases: Phases | null;
  safety: SafetyStatus;
  alarms: Alarm[];                       // the last 20, newest last
  control_seq: number;                   // length of control_log; reload /api/live when it changes
  reference: null | {                    // see PUT /api/reference
    id: string; title: string;
    bt_now: number | null;               // reference BT at the same time since CHARGE
    delta_bt: number | null;             // bt - bt_now
    ror_now: number | null;
    events: RoastEvent[];                // t relative to the reference's CHARGE
  };
  projection: null | {                   // between CHARGE and DROP, once the RoR is positive
    bt_60s: number | null;
    next: null | { kind: "first_crack_start" | "drop"; bt: number; eta_s: number | null };
  };                                     // next: from the reference's event temperatures
  preheat: null | { target_c: number; state: "heating" | "holding" };
  control_request: string | null;  // an AI asked for control and waits for "I'm at the roaster" (shown 120 s)
}
```

## Endpoints

| Method & path | Body | Response |
|---|---|---|
| `GET /api/status` | | `Snapshot` |
| `GET /api/live` | | `{ points: Point[], events, notes: {t,text,origin}[], control_log: { t, at, previous, request: {actuator, value, source, reason}, outcome }[] }` — every request, applied (with `clamped_from` if limited) or rejected (with a reason); the running recording, or the preview (last 10 min) when not recording |
| `GET /api/ports` | | `string[]` serial ports, then on macOS paired Bluetooth serial devices as `bluetooth:<name>` (Bluetooth direct, ADR 0013) |
| `POST /api/connect` | `{ simulator: boolean, port?: string }` | `Snapshot`; `port` is a serial port or `bluetooth:<name>` |
| `POST /api/disconnect` | | `Snapshot` |
| `POST /api/command` | `{ actuator, value, source?: "human" \| "ai", reason? }` | `{ outcome: CommandOutcome }` (source defaults to `human`) |
| `POST /api/emergency-stop` | `{ reason? }` | `Snapshot` |
| `POST /api/emergency-stop/reset` | | `Snapshot` |
| `POST /api/presence` | | `Snapshot` (a person confirmed being at the roaster) |
| `POST /api/ai/resume` | | `Snapshot` (a person allows AI control again) |
| `POST /api/ai/lease` | `{ holder }` | `{ holder, expires_in_s }` or 409 `{ error, reason }` |
| `DELETE /api/ai/lease` | | `Snapshot` |
| `POST /api/recording/start` | `{ title?, green_weight_g?, beans? }` | `Snapshot` |
| `PATCH /api/recording` | `{ title?, green_weight_g?, roasted_weight_g?, beans? }` | `Snapshot` |
| `POST /api/recording/stop` | | `{ id, json_path, alog_path }` (saves the roast) |
| `POST /api/events` | `{ kind, origin? }` | `RoastEvent` (marks at the latest `t`; replaces an earlier event of the same kind) |
| `DELETE /api/events/{kind}` | | `Snapshot` (undo a mark) |
| `POST /api/notes` | `{ text }` | `Snapshot` |
| `GET /api/roasts` | | `{ id, title, roasted_at, total_s, drop_bt, green_weight_g, roasted_weight_g }[]`, newest first |
| `GET /api/roasts/{id}` | | the roast (`.roast.json` content) |
| `PATCH /api/roasts/{id}` | `{ title?, roasted_weight_g?, notes? }` | the roast |
| `GET /api/roasts/{id}/alog` | | Artisan `.alog` text (download) |
| `GET /api/reference` | | `null` or `{ id, title, points: {t, bt, ror}[], events }` — `t` in seconds since the reference's CHARGE |
| `PUT /api/reference` | `{ id: string \| null }` | `Snapshot` (sets or clears the reference curve; 404 for an unknown id) |
| `POST /api/preheat` | `{ target_c }` | `Snapshot` — preheat to `target_c` (bean probe) and hold it; 400 outside 50 … `max_bt_c - 10`, 409 after CHARGE or when automation is not allowed (`reason`) |
| `DELETE /api/preheat` | | `Snapshot` — stop preheating; the burner holds |
| `POST /api/shutdown` | | 202; the daemon exits as on Ctrl-C (safe state if the burner is on, recording saved) |

## WebSocket `GET /api/ws?token=…`

The server sends JSON text messages:

- `{ "type": "tick", "snapshot": Snapshot, "point": Point | null }`: after
  every sample (about 1 Hz) and after every state change (`point` null then).
  `point` belongs to the curve returned by `/api/live`; on (re)connect, fetch
  `/api/live` once and then append points.
- `{ "type": "reset" }`: the curve started over (recording started or
  stopped): fetch `/api/live` again.

## MCP

`roastmcp mcp` is an MCP server on stdio that forwards to this API using
`daemon.json`. Tools: `get_status`, `get_curve`, `wait` (up to 30 s, then
status), `mark_event`, `add_note`, `request_control`, `release_control`,
`set_burner`, `set_air`, `set_drum`, `emergency_stop`. Presence can only be
confirmed in the app.
