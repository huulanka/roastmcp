/** Types of the daemon API; the contract is docs/api.md. */

export type Actuator = "burner" | "air" | "drum" | "cooling" | "filter";
export type EventKind =
  | "charge"
  | "turning_point"
  | "dry_end"
  | "first_crack_start"
  | "first_crack_end"
  | "second_crack_start"
  | "second_crack_end"
  | "drop"
  | "cool_end";
export type EventOrigin = "human" | "auto" | "detector" | "ai" | "import";
export type RoastState =
  "disconnected" | "connected" | "recording" | "charged" | "dropped" | "cooling" | "finished";
export type RejectReason =
  | "emergency_stop_latched"
  | "ai_suspended_by_human"
  | "no_lease"
  | "presence_required"
  | "out_of_range"
  | "rate_limited"
  | "not_supported"
  | "disconnected"
  | "overheat";

export type CommandOutcome =
  | { outcome: "applied"; value: number; clamped_from?: number | null }
  | { outcome: "rejected"; reason: RejectReason }
  | { outcome: "awaiting_confirmation"; confirmation_id: string };

export interface RoastEvent {
  t: number;
  kind: EventKind;
  origin: EventOrigin;
  bt: number | null;
}
export type ActuatorValues = Partial<Record<Actuator, number>>;
export interface Point {
  t: number;
  bt: number | null;
  et: number | null;
  ror: number | null;
  actuators: ActuatorValues;
}
export interface Phases {
  drying_s: number;
  maillard_s: number | null;
  development_s: number | null;
  total_s: number;
  dtr_pct: number | null;
}
export interface SafetyStatus {
  emergency_stop: boolean;
  ai_suspended: boolean;
  lease_holder: string | null;
  lease_remaining_s: number | null;
  presence_due_in_s: number | null;
  overheat: boolean;
  connected: boolean;
  actuators: ActuatorValues;
}
export interface Alarm {
  id: number;
  at: string;
  severity: "info" | "warning" | "critical";
  kind: string;
  message: string;
}
export interface ActuatorSpec {
  actuator: Actuator;
  min: number;
  max: number;
  step: number;
  /** How people read the value when not in percent (the drum in rpm). */
  display?: DisplayScale;
}
/** `shown = percent * scale + offset`, in `unit`. */
export interface DisplayScale {
  unit: string;
  scale: number;
  offset: number;
}
export interface MachineInfo {
  id: string;
  name: string;
  simulated: boolean;
  port: string | null;
  actuators: ActuatorSpec[];
  switches: Actuator[];
  has_et: boolean;
  sampling_interval_s: number;
  max_bt_c: number;
  link_loss_tested: boolean;
  /** What the machine does when the link breaks, as advice; null until tested. */
  link_lost: string | null;
  /** A person's command on these ends AI control (ADR 0019); absent on older daemons. */
  takeover_actuators?: Actuator[];
}
export interface Recording {
  id: string;
  title: string;
  started_at: string;
  green_weight_g: number | null;
  beans: string | null;
  /** Absent on daemons without the inventory (ADR 0014). */
  lot_id?: string | null;
  plan?: RoastPlan | null;
  /** The batch number for the label; never set in the simulator. */
  batch_no?: string | null;
}
export type RoastGoal = "espresso" | "filter" | "omni" | "other";
/** What a roast aims for (ADR 0014); guidance only, never read by the safety layer. */
export interface RoastPlan {
  goal?: RoastGoal;
  reference_id?: string;
  drop_bt_c?: number;
  dtr_pct?: number;
  /** A profile version as the plan; wins over reference_id. */
  profile_version_id?: string;
}

/** One point of a profile's target curve; times are seconds since CHARGE. */
export interface TargetPoint {
  roast_time_s: number;
  bt_c: number;
  ror_c_per_min?: number | null;
}
/** The events a recipe step can follow (ADR 0019). */
export type RecipeAnchor = "dry_end" | "first_crack_start";
export type ActionTrigger =
  | { at: "time"; roast_time_s: number }
  | { at: "bt"; bt_c: number }
  | { at: "event"; event: RecipeAnchor; offset_s: number };
/** A recipe step: an actuator to a value at a time, a temperature or after an event. */
export interface PlannedAction {
  trigger: ActionTrigger;
  actuator: Actuator;
  value: number;
}
export interface EventTarget {
  bt_c?: number | null;
  roast_time_s?: number | null;
}
export type DropRule =
  | { rule: "at_bt"; bt_c: number }
  | { rule: "at_dtr"; dtr_pct: number }
  | { rule: "at_time"; roast_time_s: number };
export interface ProfileTarget {
  points: TargetPoint[];
  actions: PlannedAction[];
  events: {
    dry_end?: EventTarget | null;
    first_crack?: EventTarget | null;
    drop?: DropRule | null;
  };
  tolerance?: { bt_c: number; ror_c_per_min?: number | null };
}
export interface ProfileVersion {
  id: string;
  profile_id: string;
  version: number;
  parent_version?: number | null;
  created_at: string;
  change_note?: string | null;
  target: ProfileTarget;
}
export interface ProfileInfo {
  id: string;
  name: string;
  description?: string | null;
  machine_id?: string | null;
  intended_beans?: string | null;
  batch_size_g?: number | null;
  archived: boolean;
  created_at: string;
}
export interface ProfileSummary extends ProfileInfo {
  latest: {
    id: string;
    version: number;
    created_at: string;
    change_note?: string | null;
  } | null;
  versions: number;
}
export interface ProfileDetail extends ProfileInfo {
  /** Newest first. */
  versions: ProfileVersion[];
}
export type Process =
  "washed" | "natural" | "honey" | "pulped_natural" | "semi_washed" | "wet_hulled" | "other";
export type HoneyGrade = "white" | "yellow" | "red" | "black";
export type Fermentation =
  "anaerobic" | "carbonic_maceration" | "thermal_shock" | "extended" | "inoculated" | "infused";
export type Drying = "raised_beds" | "patio" | "greenhouse" | "mechanical";
export type Decaf =
  "swiss_water" | "mountain_water" | "ethyl_acetate" | "co2" | "methylene_chloride";
export type Species = "arabica" | "robusta" | "liberica" | "excelsa";
export type Packaging = "grainpro" | "vacuum" | "jute" | "ecotact" | "other";
export type Certification = "organic" | "fairtrade" | "rainforest_alliance";
/** What an importer's offer sheet states about a lot (ADR 0014); all optional. */
export interface LotDetails {
  origin_country?: string;
  region?: string;
  producer?: string;
  altitude?: { min_m: number; max_m?: number };
  species?: Species;
  varieties?: string[];
  process?: Process;
  honey?: HoneyGrade;
  fermentation?: Fermentation[];
  fermentation_hours?: number;
  drying?: Drying;
  decaf?: Decaf;
  process_detail?: string;
  harvest?: string;
  arrival_date?: string;
  grade?: string;
  screen_size?: string;
  moisture_pct?: number;
  water_activity?: number;
  density_g_per_l?: number;
  cupping_score?: number;
  flavor_notes?: string;
  supplier?: string;
  supplier_lot?: string;
  price_per_kg?: { amount: number; currency: string };
  packaging?: Packaging;
  storage_location?: string;
  certifications?: Certification[];
  notes?: string;
}
/** A green coffee lot with its stock (grams; negative when more was roasted than bought). */
export interface Lot extends LotDetails {
  id: string;
  name: string;
  archived: boolean;
  /** Start of its roasts' batch numbers, e.g. "BRYB". */
  batch_code?: string | null;
  created_at: string;
  purchase_date?: string;
  stock_g: number;
}
export interface StockMovement {
  id: string;
  lot_id: string;
  at: string;
  /** Signed grams. */
  grams: number;
  kind: "purchase" | "roast_debit" | "adjustment";
  roast_id?: string;
  note?: string;
}
/** The reference roast as seen from the live roast (same time since CHARGE). */
export interface ReferenceStatus {
  id: string;
  title: string;
  /** An earlier roast, or a profile's plan with its recipe. */
  kind?: "roast" | "profile";
  /** Its bean temperature at CHARGE (0:00): where to preheat to. */
  charge_bt?: number | null;
  bt_now: number | null;
  /** Live BT minus the reference BT now. */
  delta_bt: number | null;
  ror_now: number | null;
  /** Live RoR minus the reference RoR now. */
  delta_ror?: number | null;
  events: RoastEvent[];
  /** A profile's next recipe step the machine does not have yet. */
  next_step?: NextStep | null;
}
/** The next recipe step(s), from the daemon's recipe runner (ADR 0019). */
export interface NextStep {
  /** Roast time it is due; null while its event has neither come nor is expected. */
  at_s: number | null;
  /** Seconds until then (negative: due since); null before CHARGE. */
  in_s: number | null;
  /** `at_s` is an estimate: the event or temperature has not come yet. */
  projected?: boolean;
  /** "1:30", "FC +0:30", "185 °C". */
  label?: string;
  /** The daemon sets it itself (an AI holds the lease); otherwise a cue for the person. */
  automatic?: boolean;
  set: { step?: number; actuator: Actuator; value: number; current: number | null }[];
}
export type RecipeStepState = "waiting" | "due" | "done" | "passed" | "skipped" | "replaced";
/** A profile's recipe in the running roast (ADR 0019). */
export interface RecipeStatus {
  automatic: boolean;
  adjusted: boolean;
  steps: {
    step: number;
    actuator: Actuator;
    value: number;
    planned_value?: number;
    trigger: ActionTrigger;
    label: string;
    at_s: number | null;
    projected: boolean;
    state: RecipeStepState;
    adjusted: boolean;
  }[];
}
export interface Projection {
  /** BT expected 60 s from now. */
  bt_60s: number | null;
  next: null | { kind: "first_crack_start" | "drop"; bt: number; eta_s: number | null };
}
export interface Preheat {
  target_c: number;
  state: "heating" | "holding";
}
/** The autopilot while it is switched on (ADR 0020). */
export interface Autopilot {
  /** Before CHARGE; in control; waiting for a presence confirmation. */
  state: "armed" | "active" | "waiting";
  /** Burner points its corrections moved away from the recipe. */
  offset: number;
  corrections: number;
}
/** `GET /api/reference`: the reference curve, `t` in seconds since its CHARGE. */
export interface ReferenceCurve {
  id: string;
  title: string;
  kind?: "roast" | "profile";
  points: { t: number; bt: number | null; ror: number | null }[];
  events: RoastEvent[];
  /** A profile's recipe. */
  actions?: PlannedAction[];
}
export interface Snapshot {
  version: string;
  machine: MachineInfo | null;
  connection: "disconnected" | "connecting" | "connected" | "error";
  connection_error: string | null;
  state: RoastState;
  recording: Recording | null;
  t: number | null;
  charge_t: number | null;
  bt: number | null;
  et: number | null;
  ror: number | null;
  events: RoastEvent[];
  phases: Phases | null;
  safety: SafetyStatus;
  alarms: Alarm[];
  /** Length of the control log; changes whenever an action was logged. Absent on older daemons. */
  control_seq?: number;
  /** Absent on daemons without reference support. */
  reference?: ReferenceStatus | null;
  projection?: Projection | null;
  /** Absent on daemons without preheat support. */
  preheat?: Preheat | null;
  /** An AI asked for control and waits for "I'm at the roaster". */
  control_request?: string | null;
  /** The profile's recipe in this roast (ADR 0019). */
  recipe?: RecipeStatus | null;
  /** The autopilot while it is on; absent on daemons without it. */
  autopilot?: Autopilot | null;
}
export interface Note {
  t: number;
  text: string;
  origin: string;
}
export type CommandSource = "human" | "safety" | "autopilot" | "ai";
export interface ControlLogEntry {
  /** Seconds since the recording started (same base as `Point.t`), if one was running. */
  t?: number | null;
  at: string;
  /** The actuator's value before the request, if known. */
  previous?: number | null;
  request: { actuator: Actuator; value: number; source: CommandSource; reason?: string };
  outcome: CommandOutcome;
}
export interface Live {
  points: Point[];
  events: RoastEvent[];
  notes: Note[];
  control_log: ControlLogEntry[];
}
export interface RoastSummary {
  id: string;
  title: string;
  roasted_at: string;
  total_s: number | null;
  drop_bt: number | null;
  green_weight_g: number | null;
  roasted_weight_g: number | null;
  simulated?: boolean;
  beans?: string | null;
  lot_id?: string | null;
  first_crack_s?: number | null;
  dtr_pct?: number | null;
  goal?: RoastGoal | null;
  /** Batch number on the label, e.g. "BRYB-002" (imports: from the file name). */
  batch_no?: string | null;
  /** The profile version the roast followed. */
  profile_version_id?: string | null;
}
export interface RoastSample {
  t: number;
  bt: number | null;
  et?: number | null;
  ror?: number | null;
  actuators?: ActuatorValues;
}
/** Where the database is backed up to, and how the last backup went. */
export interface BackupStatus {
  dir: string | null;
  keep: number;
  last: { at: string; ok: boolean; path?: string; bytes?: number; error?: string } | null;
}
/** Measured roast colour (lower is darker on the Agtron scale). */
export interface RoastDegree {
  scale: "agtron" | "lighttells" | "other";
  whole_bean?: number | null;
  ground?: number | null;
}
export interface Roast {
  id: string;
  roast_degree?: RoastDegree | null;
  title?: string;
  roasted_at?: string;
  machine?: unknown;
  sampling_interval_s?: number;
  batch?: {
    green_weight_g?: number | null;
    roasted_weight_g?: number | null;
    beans?: string | null;
    batch_no?: string | null;
  };
  plan?: RoastPlan | null;
  green_lot_id?: string | null;
  origin?: { kind: string; file_name?: string };
  samples: RoastSample[];
  events?: RoastEvent[];
  control_log?: unknown[];
  notes?: { t: number; text: string; origin?: string }[];
}
/** Why a maintenance task is due. */
export interface DueReason {
  by: "roasts" | "days";
  since: number;
  interval: number;
}
/** A recurring maintenance task with how long ago it was done. */
export interface MaintenanceTask {
  id: string;
  name: string;
  description?: string;
  machine_id?: string;
  interval_roasts?: number;
  interval_days?: number;
  last_done_at?: string;
  archived: boolean;
  created_at: string;
  /** Roasts on a real machine since it was last done (or added). */
  roasts_since: number;
  /** Whole days since it was last done (or added). */
  days_since: number;
  due: DueReason | null;
}
export interface MaintenanceLogEntry {
  id: string;
  task_id: string;
  task_name: string;
  done_at: string;
  note?: string;
}
/** One thing to check before a roast; `key` names a built-in item. */
export interface ChecklistItem {
  key?: string;
  text: string;
}
/** The checklist before a roast: the person's items and the maintenance due. */
export interface Checklist {
  items: ChecklistItem[];
  due: MaintenanceTask[];
}
/** Attribute scores of a tasting, each 0–10; left out when not rated. */
export interface CuppingScores {
  fragrance_aroma?: number;
  flavor?: number;
  aftertaste?: number;
  acidity?: number;
  body?: number;
  sweetness?: number;
  balance?: number;
  overall?: number;
}
export interface Cupping {
  id: string;
  roast_id: string;
  cupped_at: string;
  rest_days: number;
  scores: CuppingScores;
  /** The total on the person's own scale, e.g. SCA 0–100. */
  overall_score?: number;
  descriptors: string[];
  notes?: string;
  brew_method?: string;
  /** Mean of the attribute scores given. */
  mean_score?: number | null;
  roast_title?: string;
  batch_no?: string | null;
  lot_id?: string | null;
}
/** What a tasting is saved with; scores and descriptors replace the stored ones. */
export interface CuppingInput {
  cupped_at?: string;
  rest_days?: number;
  scores?: CuppingScores;
  overall_score?: number;
  descriptors?: string[];
  notes?: string;
  brew_method?: string;
}
export type RestState = "resting" | "ready" | "past_best";
/** One roast in the rest overview: how long it rests and its window. */
export interface RestEntry {
  roast_id: string;
  title: string;
  batch_no?: string | null;
  beans?: string | null;
  roasted_at: string;
  level: "light" | "medium" | "dark";
  level_from: "colour" | "default";
  rest_days: number;
  ready_from: string;
  best_until: string;
  state: RestState;
  cuppings: number;
}
/** The figures of one roast, times in seconds since CHARGE. */
export interface RoastFigures {
  charge_bt_c?: number | null;
  turning_point_s?: number | null;
  turning_point_bt_c?: number | null;
  dry_end_s?: number | null;
  dry_end_bt_c?: number | null;
  first_crack_s?: number | null;
  first_crack_bt_c?: number | null;
  total_s?: number | null;
  drop_bt_c?: number | null;
  development_s?: number | null;
  dtr_pct?: number | null;
  weight_loss_pct?: number | null;
  colour_whole?: number | null;
  colour_ground?: number | null;
}
export interface Spread {
  n: number;
  mean: number;
  sd: number;
  min: number;
  max: number;
}
export type SpreadKey =
  | "charge_bt_c"
  | "dry_end_s"
  | "first_crack_s"
  | "first_crack_bt_c"
  | "total_s"
  | "drop_bt_c"
  | "development_s"
  | "dtr_pct"
  | "weight_loss_pct";
export interface ConsistencyResult {
  consistency: { roasts: number; curve_sd_c: number | null } & Partial<
    Record<SpreadKey, Spread | null>
  >;
  roasts: {
    id: string;
    title: string;
    roasted_at: string;
    batch_no?: string | null;
    figures: RoastFigures;
  }[];
  unfinished: number;
}
export type Alignment = "charge" | "first_crack";
/** Two saved roasts side by side. */
export interface Comparison {
  align: Alignment;
  a: RoastFigures;
  b: RoastFigures;
  delta: RoastFigures;
  curve: {
    t: number;
    a_bt: number | null;
    b_bt: number | null;
    delta_bt: number | null;
    a_ror: number | null;
    b_ror: number | null;
  }[];
  /** The largest BT difference after both turning points, °C. */
  max_delta_bt: number | null;
}
export type WsMessage =
  { type: "tick"; snapshot: Snapshot; point: Point | null } | { type: "reset" };
