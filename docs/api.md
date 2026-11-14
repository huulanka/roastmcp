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
// ADR 0014: what a roast aims for; guidance only, the safety layer never reads it.
interface RoastPlan { goal?: "espresso" | "filter" | "omni" | "other"; reference_id?: string;
                      drop_bt_c?: number; dtr_pct?: number }
// A green coffee lot (roast-core GreenLot) with its stock: the sum of its movements.
// The offer-sheet details (LotDetails) sit next to the name; all optional.
interface Lot { id: string; name: string; purchase_date?: string; archived: boolean;
  created_at: string; stock_g: number /* negative when more was roasted than bought */;
  // origin
  origin_country?: string; region?: string; producer?: string;
  altitude?: { min_m: number; max_m?: number };
  species?: "arabica" | "robusta" | "liberica" | "excelsa"; varieties?: string[];
  // processing: base method + fermentation techniques + drying
  process?: "washed" | "natural" | "honey" | "pulped_natural" | "semi_washed" | "wet_hulled" | "other";
  honey?: "white" | "yellow" | "red" | "black";
  fermentation?: ("anaerobic" | "carbonic_maceration" | "thermal_shock" | "extended"
    | "inoculated" | "infused")[]; fermentation_hours?: number;
  drying?: "raised_beds" | "patio" | "greenhouse" | "mechanical";
  decaf?: "swiss_water" | "mountain_water" | "ethyl_acetate" | "co2" | "methylene_chloride";
  process_detail?: string;
  // quality and measurements
  harvest?: string; arrival_date?: string; grade?: string; screen_size?: string;
  moisture_pct?: number; water_activity?: number; density_g_per_l?: number;
  cupping_score?: number; flavor_notes?: string;
  // purchase and storage
  supplier?: string; supplier_lot?: string; price_per_kg?: { amount: number; currency: string };
  packaging?: "grainpro" | "vacuum" | "jute" | "ecotact" | "other"; storage_location?: string;
  certifications?: ("organic" | "fairtrade" | "rainforest_alliance")[]; notes?: string }
interface StockMovement { id: string; lot_id: string; at: string; grams: number /* signed */;
  kind: "purchase" | "roast_debit" | "adjustment"; roast_id?: string; note?: string }
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
  link_lost: string | null;   // the profile's advice, once tested
  takeover_actuators: Actuator[] }  // a person's command on these ends AI control (ADR 0019)

interface Snapshot {
  version: string;
  machine: MachineInfo | null;           // null when disconnected
  connection: "disconnected" | "connecting" | "connected" | "error";
  connection_error: string | null;
  state: RoastState;
  recording: { id: string; title: string; started_at: string;
               green_weight_g: number | null; beans: string | null;
               lot_id: string | null; plan: RoastPlan | null;
               batch_no: string | null /* never in the simulator */ } | null;
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
    kind: "roast" | "profile";           // an earlier roast, or a profile version's plan
    charge_bt: number | null;            // its BT at CHARGE (0:00): the preheat target
    bt_now: number | null;               // reference BT at the same time since CHARGE
    delta_bt: number | null;             // bt - bt_now
    ror_now: number | null;
    delta_ror: number | null;            // ror - ror_now
    events: RoastEvent[];                // t relative to the reference's CHARGE
    next_step?: {                        // a profile's next recipe step not yet applied
      at_s: number | null;               // roast time it is due; null while its event is neither come nor expected
      in_s: number | null;               // seconds until then (negative: due since; null before CHARGE)
      projected: boolean;                // at_s is an estimate (projection or plan) for a step after an event or at a BT
      label: string;                     // "1:30", "FC +0:30", "185 °C"
      automatic: boolean;                // the daemon sets it (an AI holds the lease); else a cue for the person
      set: { step: number; actuator: Actuator; value: number; current: number | null }[];
    };                                   // due for 30 s after its time unless the runner is setting it
  };
  recipe: null | {                       // a profile plan's recipe in this roast (ADR 0019)
    automatic: boolean; adjusted: boolean;
    steps: { step: number; actuator: Actuator; value: number; planned_value?: number;
             trigger: ActionTrigger; label: string; at_s: number | null; projected: boolean;
             state: "waiting" | "due" | "done" | "passed" | "skipped" | "replaced";
             adjusted: boolean }[];
  };
  projection: null | {                   // between CHARGE and DROP, once the RoR is positive
    bt_60s: number | null;
    next: null | { kind: "first_crack_start" | "drop"; bt: number; eta_s: number | null };
  };                                     // next: from the reference's event temperatures; first crack
                                         // without one in the reference at first_crack.bt
  first_crack: null | {                  // while recording: where first crack is expected (ADR 0021)
    bt: number;                          // °C it usually starts at
    low: number; high: number;           // a heard first crack is only believed in between
    roasts: number;                      // how many roasts bt comes from
    basis: "lot" | "roasts" | "typical"; // earlier roasts of the lot | of any coffee | 192 °C
  };
  preheat: null | { target_c: number; state: "heating" | "holding" };
  autopilot: null | {                    // while the autopilot is switched on (ADR 0020)
    state: "armed" | "active" | "waiting"; // before CHARGE | in control | waits for a presence confirmation
    offset: number;                      // burner points its corrections moved away from the recipe
    corrections: number;
  };
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
| `POST /api/recipe/adjust` | `{ changes: { step, shift_s?, value?, skip? }[], reason?, origin?: "human" \| "ai" }` | `Snapshot`; changes the running roast's recipe only, all or none (400 names the step that does not fit); noted with the roast |
| `POST /api/emergency-stop/reset` | | `Snapshot` |
| `POST /api/presence` | | `Snapshot` (a person confirmed being at the roaster) |
| `POST /api/ai/resume` | | `Snapshot` (a person allows AI control again) |
| `POST /api/ai/lease` | `{ holder }` | `{ holder, expires_in_s }` or 409 `{ error, reason }`; 400 for the reserved holders `preheat` and `autopilot` |
| `DELETE /api/ai/lease` | | `Snapshot` |
| `POST /api/recording/start` | `{ title?, green_weight_g?, beans?, lot_id?, plan?: RoastPlan, batch_no? }` — `plan.profile_version_id` makes a profile version the plan line (wins over `plan.reference_id`) | `Snapshot`; the lot's name becomes `beans` (and the title if none), the plan's reference becomes the reference curve; `batch_no` (e.g. `BRYB-002`) is kept with the roast, starts its file names and gives a lot without a code its code; the simulator ignores it and debits no stock; 404 for an unknown lot |
| `PATCH /api/recording` | `{ title?, green_weight_g?, roasted_weight_g?, beans?, lot_id?, plan? }` | `Snapshot` |
| `POST /api/recording/stop` | | `{ id, json_path, alog_path }` (saves the roast) |
| `POST /api/events` | `{ kind, origin? }` | `RoastEvent` (marks at the latest `t`; replaces an earlier event of the same kind) |
| `DELETE /api/events/{kind}` | | `Snapshot` (undo a mark) |
| `POST /api/notes` | `{ text }` | `Snapshot` |
| `GET /api/roasts` | | `{ id, title, roasted_at, total_s, drop_bt, green_weight_g, roasted_weight_g, batch_no, … }[]`, newest first; `batch_no` is the roast's, or for an imported Artisan file the one its name starts with (`BR011_…`) |
| `GET /api/roasts/{id}` | | the roast (`.roast.json` content); each sample also carries `ror` (°C/min), computed as the live value |
| `PATCH /api/roasts/{id}` | `{ title?, roasted_weight_g?, green_weight_g?, notes?, note_origin?, roast_degree?, ambient? }` | the roast, as `GET`; a changed green weight replaces the roast's stock debit; `notes` adds a note (`note_origin: "ai"` marks it as an AI's); `roast_degree` is `{ scale: "agtron" \| "lighttells" \| "other", whole_bean?, ground? }` and replaces the stored colour (no reading clears it); `ambient` is `{ temperature_c?, humidity_pct?, pressure_hpa? }` and replaces the stored conditions (empty clears them; 400 outside −30…60 °C, 0…100 %, 500…1100 hPa) |
| `GET /api/roasts/compare?a=<id>&b=<id>&align=charge\|first_crack&step_s=30` | | `{ align, a, b, delta, curve, max_delta_bt, roast_a, roast_b }` — two saved roasts side by side: `a` and `b` are their figures (`charge_bt_c`, `turning_point_s`, `turning_point_bt_c`, `dry_end_s`, `dry_end_bt_c`, `first_crack_s`, `first_crack_bt_c`, `total_s`, `drop_bt_c`, `development_s`, `dtr_pct`, `weight_loss_pct`, `colour_whole`, `colour_ground`; times since CHARGE), `delta` is `a − b`, `curve` has a row every `step_s` (1…600) seconds from CHARGE to DROP: `{ t, a_bt, b_bt, delta_bt, a_ror, b_ror }`. With `align=first_crack` zero is the start of first crack (negative before); `align` in the answer is `charge` if one of the roasts has no first crack. `max_delta_bt` is the largest BT difference after both turning points; `roast_a` and `roast_b` are `{ id, title, roasted_at, batch_no }`. 404 for an unknown roast |
| `GET /api/consistency?lot_id=<id>` \| `?profile_id=<id>` \| `?ids=<id>,<id>` (`&simulated=true`) | | `{ consistency, roasts, unfinished }` — how alike the roasts of a lot, of a profile (any of its versions) or of a chosen set are. `consistency` has `roasts` (how many), a spread `{ n, mean, sd, min, max }` or `null` per figure (`charge_bt_c`, `dry_end_s`, `first_crack_s`, `first_crack_bt_c`, `total_s`, `drop_bt_c`, `development_s`, `dtr_pct`, `weight_loss_pct`) and `curve_sd_c`: the standard deviation of the bean temperature across the roasts, averaged between turning point and DROP (`null` under two roasts). `roasts` lists them oldest first with their `figures` (the newest 60 at most); recordings without CHARGE or DROP are left out and counted in `unfinished`. Simulator roasts only with `simulated=true` or by id. 400 unless exactly one of the three is given |
| `GET /api/experiments` | | `Experiment[]`, newest first: `{ id, hypothesis, changed_variable: { description, structured?: { name, from, to } }, expected_outcome?, conclusion?, status: "planned" \| "running" \| "done", profile_version_id?, roast_ids, created_at }` |
| `POST /api/experiments` | `{ hypothesis, change, variable?: { name, from, to }, expected_outcome?, profile_version_id?, roast_ids? }` | `Experiment`; with roasts it is `running` at once. 400 without a hypothesis or a change, 404 for an unknown roast |
| `GET /api/experiments/{id}` / `PATCH /api/experiments/{id}` / `DELETE /api/experiments/{id}` | patch: `{ hypothesis?, change?, expected_outcome?, conclusion?, status?, add_roasts?, remove_roasts? }` | `Experiment` (`{}` after a delete). The first roast makes it `running`, a conclusion `done`, unless `status` says otherwise. A roast belongs to one experiment and names it (`experiment_id` in the roast and its file); adding it to another moves it. Deleting an experiment keeps its roasts |
| `GET /api/cuppings` (`?roast=<id>` \| `?lot=<id>`) | | `Cupping[]`, newest first: `{ id, roast_id, cupped_at, rest_days, scores: { fragrance_aroma?, flavor?, aftertaste?, acidity?, body?, sweetness?, balance?, overall? }, overall_score?, descriptors, notes?, brew_method?, mean_score, roast_title, batch_no, lot_id }` — attribute scores are 0–10, `overall_score` is the total on the person's own scale, `rest_days` the days between roast and tasting unless given, `mean_score` the mean of the scores given. 404 for an unknown roast or lot |
| `POST /api/roasts/{id}/cuppings` | `{ cupped_at?, rest_days?, scores?, overall_score?, descriptors?, notes?, brew_method? }` | `Cupping`; `cupped_at` is now if left out. 400 for a score outside 0–10 or a tasting before the roast |
| `PATCH /api/cuppings/{id}` / `DELETE /api/cuppings/{id}` | patch: the fields of the `POST`; `scores` and `descriptors` replace the stored ones, empty `notes` or `brew_method` remove them | `Cupping` (`{}` after a delete). A deleted roast takes its cuppings with it |
| `GET /api/rest` (`?simulated=true`) | | `{ roast_id, title, batch_no, beans, roasted_at, level: "light" \| "medium" \| "dark", level_from: "colour" \| "default", rest_days, ready_from, best_until, state: "resting" \| "ready" \| "past_best", cuppings }[]`, newest first — every roast that rests or is in its window, and those past their best for 14 more days. The level comes from the measured colour (Agtron or Lighttells, ground before whole bean: from 70 light, from 50 medium, below dark) and is medium without one |
| `GET /api/rest-windows` / `PUT /api/rest-windows` | `{ light, medium, dark }`, each `{ min_days, max_days }` | the same; defaults 7–21, 4–14 and 2–10 days. 400 if a window ends before it starts or after 365 days |
| `GET /api/maintenance` (`?archived=true`) | | `MaintenanceTask[]` in the order they were added: `{ id, name, description?, machine_id?, interval_roasts?, interval_days?, last_done_at?, archived, created_at, roasts_since, days_since, due }` — `due` is `null` or `{ by: "roasts" \| "days", since, interval }`; a task is due after `interval_roasts` roasts or `interval_days` days since it was last done (or added), whichever comes first. Only roasts on a real machine count, with `machine_id` only that machine's |
| `POST /api/maintenance/tasks` | `{ name, description?, machine_id?, interval_roasts?, interval_days? }` | `MaintenanceTask`; without an interval it is never due, only logged |
| `PATCH /api/maintenance/tasks/{id}` | `{ name?, description?, interval_roasts?, interval_days?, archived? }` — an interval of 0 removes it, an empty description too | `MaintenanceTask` |
| `POST /api/maintenance/tasks/{id}/done` | `{ done_at?, note? }` or nothing | `MaintenanceTask`; a log entry is written and the intervals count from `done_at` (now if left out; 400 in the future) |
| `GET /api/maintenance/log` (`?task=<id>`, `?limit=100`) | | `{ id, task_id, task_name, done_at, note? }[]`, newest first, at most 500 |
| `DELETE /api/maintenance/log/{id}` | | `MaintenanceTask` — takes an entry back; the task counts from the entry before it |
| `GET /api/checklist` | | `{ items: { key?, text }[], due: MaintenanceTask[] }` — what the person checks before every roast, and the maintenance that is due. `key` names a built-in item (`extinguisher`, `chaff`, `surroundings`, `cooling`, `stay`) so the app can translate it. The checklist informs; no command depends on it |
| `PUT /api/checklist` | `{ items: { key?, text }[] }` (at most 20) | as `GET`; replaces the items. An item keeps its `key` only with the built-in text |
| `POST /api/roasts/import?name=<file name>` | the bytes of an Artisan `.alog` (up to 16 MB) | `{ id, title, already_there }` — the roast is saved as `.roast.json` with the original file beside it; the same roast again is not imported twice |
| `DELETE /api/roasts/{id}` | | `{ credited_g, trash_path }` — the files move to `roasts/trash/` (restore by moving them back), the stock debit is taken back, the action log records it |
| `GET /api/profiles?archived=true` | | `{ …Profile, latest: { id, version, created_at, change_note } \| null, versions }[]`, newest change first (ADR 0017) |
| `POST /api/profiles` | `{ name, description?, intended_beans?, batch_size_g?, target: ProfileTarget, change_note? }` | `{ …Profile, versions: ProfileVersion[] }` with version 1; 400 for a target whose times do not rise from 0, temperatures outside 0–300 °C or recipe values outside 0–100 % |
| `GET /api/profiles/{id}` / `PATCH /api/profiles/{id}` | patch: `{ name?, description?, intended_beans?, batch_size_g?, archived? }` | `{ …Profile, versions }`, versions newest first |
| `POST /api/profiles/{id}/versions` | `{ target: ProfileTarget, change_note?, parent_version? }` | `{ …Profile, versions }` with the new version first |
| `GET /api/profile-versions/{id}` | | `{ profile, version }` — the version a roast's plan names |
| `GET /api/lots?archived=true` | | `Lot[]`, newest first; archived lots only with `archived=true` |
| `POST /api/lots` | `{ name, purchase_g?, purchase_date?, batch_code?, …offer-sheet fields of Lot }` | `Lot` (with its opening purchase); blank text is dropped, a honey grade kept only with `process: "honey"`; `batch_code` keeps letters and digits, upper case, at most 8 |
| `GET /api/lots/{id}` / `PATCH /api/lots/{id}` | patch: `{ name?, archived?, details?, batch_code?, next_roast? }`; `details` (all offer-sheet fields) replaces the lot's details whole; an empty `batch_code` removes it; `next_roast` is the person's note for the next roast of the lot (at most 500 characters, shown when the lot is picked to roast), an empty one removes it | `Lot` |
| `GET /api/lots/{id}/movements` | | `StockMovement[]`, newest first |
| `POST /api/lots/{id}/purchases` | `{ grams, date?, note? }` | `Lot` |
| `POST /api/lots/{id}/adjustments` | `{ grams /* signed */, note? }` | `Lot` |

A roast debits its green weight from its lot once, when it is saved after
CHARGE (not when stopped before). The inventory lives in the database; a
change is saved whole or not at all. If an `inventory.json` of a version up
to 1.16 cannot be read when it is taken over, every change is refused with
409 and nothing is overwritten.
| `GET /api/roasts/{id}/alog` | | Artisan `.alog` text (download) |
| `GET /api/reference` | | `null` or `{ id, title, kind, points: {t, bt, ror}[], events, actions? }` — `t` in seconds since the reference's CHARGE; `actions` is a profile's recipe |
| `PUT /api/reference` | `{ id?: string, profile_version_id?: string }` | `Snapshot` (a saved roast, or a profile version with its recipe; neither clears it; 404 for an unknown id) |
| `POST /api/preheat` | `{ target_c, air? }` | `Snapshot` — sets `air` (%) first as the person's command, then preheats to `target_c` (bean probe) and holds it (ADR 0018); without `air` the applied air stays; 400 outside 50 … `max_bt_c - 10` or air outside 0–100, 409 after CHARGE or when automation is not allowed (`reason`) |
| `DELETE /api/preheat` | | `Snapshot` — stop preheating; the burner holds |
| `POST /api/autopilot` | | `Snapshot` — the app's autopilot switch (ADR 0020): from CHARGE the daemon sets the recipe's steps and trims the burner to keep the roast on the profile's curve; before CHARGE it is armed. Counts as a presence confirmation and allows automation again, so only the app calls it. 409 without a profile as the plan, after DROP, when disconnected or while the emergency stop is latched (`reason`) |
| `DELETE /api/autopilot` | | `Snapshot` — switch the autopilot off; the actuators hold and the person has control |
| `GET /api/backup` | | `{ dir, keep, last }` — the backup folder (`null`: none), how many backups stay there, and how the last one went: `{ at, ok, path?, bytes?, error? }` or `null` |
| `PUT /api/backup` | `{ dir: string \| null, keep?: number }` | as `GET`; `dir` is a full path and is created right away (400 if that fails), `null` or empty turns backups off |
| `POST /api/backup/run` | | as `GET`, after a backup made now; 400 with the reason if none is set or it failed |
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
`daemon.json`. Tools: `get_status`, `get_curve`, `wait` (up to 30 s, then a
compact status: roast time, BT, RoR, actuators, the plan's deviation and next
recipe step, the recipe's open steps, the projection, lease and presence,
and only the events and alarms new since the last `get_status` or `wait`;
`full: true` returns the whole status), `mark_event`, `add_note`,
`request_control` (also while the preheat holds, ADR 0019), `release_control`,
`adjust_recipe` (move, change or skip steps of the running roast's recipe),
`set_burner`, `set_air`, `set_drum`, `set_filter`, `set_cooling` (on/off),
`emergency_stop`, `list_roasts`,
`get_roast`, `compare_roasts` (two roasts lined up on CHARGE or on first
crack), `get_consistency` (the spread over the roasts of a lot, a profile or
a chosen set), `add_roast_note`, the experiments (`list_experiments`,
`record_experiment`, `update_experiment`: one hypothesis, one change, the
roasts that test it, the conclusion), tasting and rest (`get_cuppings`,
`record_cupping`: scores and words as the person gives them, never read
off the curve; `get_rest_status`: which roasts rest, are ready or past
their best), the care of the roaster (`get_maintenance`,
`add_maintenance_task`, `log_maintenance`: only what the person says they
did; `get_checklist`: read-only, to go through with the person before
preheating),
`set_reference` (a roast or a profile), the inventory and start
dialog of the app (ADR 0014): `list_lots`, `add_lot`, `add_purchase`,
`start_recording` (lot by id or name, green weight, goal, reference or
profile, drop temperature, DTR), and the profiles (ADR 0017):
`list_profiles`, `get_profile` (milestones, phase shares, recipe and the
curve every 15 s per version), `create_profile` and `add_profile_version`.
The last two build the plan in the MCP server with `roast-core::design`, as
the app's profile designer does: a starting curve (a saved roast via
`from_roast`, the newest version, or a typical Cubean+ espresso roast) moved
to the milestones given, plus the recipe, and save it through `POST
/api/profiles` and `POST /api/profiles/{id}/versions`. `mark_event charge`
from the AI needs a running recording; `start_recording` answers with the
status, whose `recording.batch_no` confirms the batch number. A marked event
replaces a detected one of the same kind (alarm `event_replaced`); phases,
DTR and the plan use the marked one. At DROP the daemon turns the burner
off, switches the bean cooling on and ends the AI's lease (see
[safety.md](safety.md)). Presence can only be confirmed in the
app, and there is no preheat or autopilot tool, because starting either
confirms presence. The status shows the autopilot while it is on (state,
offset from the recipe, corrections).

The server also offers two prompts (`prompts/list`, `prompts/get`):
`debrief_roast` (argument `roast`: an id or batch number, the newest roast
without it) walks through a saved roast, its plan and the coffee's other
roasts and ends with one proposed change; `plan_next_roast` (argument
`coffee`) plans the next roast of a lot from its earlier roasts, its profile
and the open experiments, and prepares the recording.
