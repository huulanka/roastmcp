import type {
  EventKind,
  Note,
  RoastEvent,
  RoastState,
  SafetyStatus,
  Snapshot,
} from "@/api/types.ts";
import { EVENT_NAME, type Series } from "./chartData.ts";

/** States in which a recording runs. */
const RECORDING: readonly RoastState[] = ["recording", "charged", "dropped", "cooling"];

export function isRecording(state: RoastState): boolean {
  return RECORDING.includes(state);
}

/** What the one big button (and Enter) does next. */
export type PrimaryAction =
  { type: "start" } | { type: "mark"; kind: EventKind } | { type: "stop" };

/**
 * The next step of a roast: start a recording, then CHARGE, first crack and
 * DROP. The daemon saves the roast at DROP; "stop" only remains for a roast
 * that was dropped before this behaviour existed.
 */
export function nextAction(
  state: RoastState,
  connected: boolean,
  events: readonly RoastEvent[],
): PrimaryAction | null {
  if (!isRecording(state)) return connected ? { type: "start" } : null;
  const has = (k: EventKind) => events.some((e) => e.kind === k);
  if (!has("charge")) return { type: "mark", kind: "charge" };
  if (!has("first_crack_start") && !has("drop")) return { type: "mark", kind: "first_crack_start" };
  if (!has("drop")) return { type: "mark", kind: "drop" };
  return { type: "stop" };
}

const MARK_LABEL: Partial<Record<EventKind, string>> = {
  charge: "Mark charge",
  first_crack_start: "Mark first crack",
  drop: "Mark drop",
};

/** Label of the primary button (English key; `t` translates). */
export function primaryLabel(t: (k: string) => string, action: PrimaryAction): string {
  if (action.type === "start") return t("Start recording");
  if (action.type === "stop") return t("Save roast");
  return t(MARK_LABEL[action.kind] ?? EVENT_NAME[action.kind]);
}

/** Thresholds for rate-of-rise warnings; one place so they can become settings. */
export const ROR_RULES = {
  /** ΔRoR compares the RoR now with the RoR this many seconds ago. */
  windowS: 30,
  /** A crash: RoR fell by at least this much (°C/min) within the window… */
  crashDrop: 3,
  /** …after first crack, or in the last part of the roast from this fraction on. */
  lateFraction: 2 / 3,
  /** Roast length assumed for "late" when no reference roast gives one, seconds. */
  typicalRoastS: 600,
  /** A flick: RoR rose by at least this much (°C/min) within the window after first crack. */
  flickRise: 1,
} as const;

/** RoR now minus RoR `windowS` seconds ago (°C/min), or null without enough history. */
export function rorDelta(
  x: readonly number[],
  ror: Series,
  windowS: number = ROR_RULES.windowS,
): number | null {
  let last = -1;
  for (let i = x.length - 1; i >= 0; i--) {
    if (typeof ror[i] === "number") {
      last = i;
      break;
    }
  }
  if (last < 0) return null;
  for (let i = last - 1; i >= 0; i--) {
    if (x[last] - x[i] < windowS) continue;
    if (typeof ror[i] === "number") {
      return x[last] - x[i] > windowS * 2 ? null : (ror[last] as number) - (ror[i] as number);
    }
  }
  return null;
}

export type RorAlert = "crash" | "flick";

/** Crash or flick, judged from ΔRoR and where the roast stands. Times are seconds since CHARGE. */
export function rorAlert(input: {
  delta: number | null;
  sinceCharge: number | null;
  firstCrack: number | null;
  /** Expected roast length (e.g. the reference's drop), seconds. */
  roastS: number | null;
  dropped: boolean;
}): RorAlert | null {
  const { delta, sinceCharge, firstCrack, roastS, dropped } = input;
  if (delta === null || sinceCharge === null || dropped) return null;
  const afterFc = firstCrack !== null && sinceCharge >= firstCrack;
  const late = sinceCharge >= ROR_RULES.lateFraction * (roastS ?? ROR_RULES.typicalRoastS);
  if (delta <= -ROR_RULES.crashDrop && (afterFc || late)) return "crash";
  if (delta >= ROR_RULES.flickRise && afterFc) return "flick";
  return null;
}

/** Development time and ratio, live from the first-crack mark until DROP. */
export function liveDtr(
  events: readonly RoastEvent[],
  base: number | null,
  nowT: number | null,
): { dtr: number; devS: number } | null {
  const fc = events.find((e) => e.kind === "first_crack_start")?.t;
  const end = events.find((e) => e.kind === "drop")?.t ?? nowT;
  if (base === null || fc === undefined || end === null || end <= base || end < fc) return null;
  const devS = end - fc;
  return { dtr: (devS / (end - base)) * 100, devS };
}

/** The most recent note written by an AI. */
export function latestAiNote(notes: readonly Note[]): Note | null {
  let best: Note | null = null;
  for (const n of notes) if (n.origin === "ai" && (best === null || n.t >= best.t)) best = n;
  return best;
}

export interface PresenceView {
  /** Seconds until the next confirmation is due (negative = overdue), or null when not tracked. */
  due: number | null;
  overdue: boolean;
  /** Only an AI in control makes a missing confirmation urgent. */
  urgent: boolean;
  /** Offer the "I'm at the roaster" button. */
  ask: boolean;
}

/** Lease holder name of the daemon's own preheat automation. */
export const PREHEAT_HOLDER = "preheat";

export function presenceView(safety: SafetyStatus): PresenceView {
  const due = safety.presence_due_in_s;
  // Any lease holder (an AI or the preheat automation) needs a person nearby.
  const aiInControl = safety.lease_holder !== null;
  const overdue = due !== null && due < 0;
  return {
    due,
    overdue,
    urgent: aiInControl && due !== null && due < 30,
    ask: due !== null && (due < 60 || (aiInControl && due < 120)),
  };
}

/**
 * The gong to sound between two snapshots (ADR 0018): 60 s and 20 s before a
 * presence confirmation is due, whenever an AI or the preheat holds control.
 * A confirmation or a jump far past a mark sounds nothing.
 */
export function presenceGong(
  prevDue: number | null,
  due: number | null,
  holder: string | null,
): 60 | 20 | null {
  if (holder === null || prevDue === null || due === null) return null;
  for (const at of [20, 60] as const) {
    if (prevDue > at && due <= at && due > at - 15) return at;
  }
  return null;
}

/** Whether the space bar confirms presence now: someone holds or asks for control, or it is due soon. */
export function spaceConfirms(s: Pick<Snapshot, "safety" | "control_request">): boolean {
  if (!s.safety.connected || s.safety.emergency_stop) return false;
  return s.safety.lease_holder !== null || !!s.control_request || presenceView(s.safety).ask;
}

/** The id of a roast that just ended after its CHARGE, given two snapshots in a row. */
export function endedRoast(
  prev: Pick<Snapshot, "recording" | "state"> | null,
  next: Pick<Snapshot, "recording">,
): string | null {
  const was = prev?.recording;
  if (!was || next.recording) return null;
  return prev.state === "charged" || prev.state === "dropped" ? was.id : null;
}
