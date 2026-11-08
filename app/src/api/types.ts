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
  | { outcome: "applied"; value: number; clamped_from: number | null }
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
}
export interface Recording {
  id: string;
  title: string;
  started_at: string;
  green_weight_g: number | null;
  beans: string | null;
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
}
export interface Note {
  t: number;
  text: string;
  origin: string;
}
export interface Live {
  points: Point[];
  events: RoastEvent[];
  notes: Note[];
  control_log: unknown[];
}
export interface RoastSummary {
  id: string;
  title: string;
  roasted_at: string;
  total_s: number | null;
  drop_bt: number | null;
  green_weight_g: number | null;
  roasted_weight_g: number | null;
}
export interface RoastSample {
  t: number;
  bt: number | null;
  et?: number | null;
  ror?: number | null;
  actuators?: ActuatorValues;
}
export interface Roast {
  id: string;
  title?: string;
  roasted_at?: string;
  machine?: unknown;
  sampling_interval_s?: number;
  batch?: {
    green_weight_g?: number | null;
    roasted_weight_g?: number | null;
    beans?: string | null;
  };
  samples: RoastSample[];
  events?: RoastEvent[];
  control_log?: unknown[];
  notes?: { t: number; text: string; origin?: string }[];
}
export type WsMessage =
  { type: "tick"; snapshot: Snapshot; point: Point | null } | { type: "reset" };
