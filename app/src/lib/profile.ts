import type { Actuator, PlannedAction, ProfileTarget, Roast, TargetPoint } from "@/api/types.ts";

/**
 * The profile designer's model: a target curve shaped by milestones, and a
 * recipe of burner and air steps. A curve keeps the natural shape of the roast
 * it came from; moving a milestone stretches the curve between its
 * neighbours in time and shifts it in temperature (ADR 0017).
 */

/** A point of a curve: seconds since CHARGE and bean temperature. */
export interface CurvePoint {
  t: number;
  bt: number;
}

/** What the roaster edits: where the roast should be when. */
export interface Milestones {
  /** Bean temperature at CHARGE, °C. */
  charge: number;
  dryEnd: CurvePoint;
  firstCrack: CurvePoint;
  drop: CurvePoint;
}

/** Spacing of a stored target curve, seconds. */
const STEP_S = 2;

function lerp(a: number, b: number, f: number): number {
  return a + (b - a) * f;
}

/** Bean temperature of a curve at `t` (linear between points, clamped at the ends). */
export function btAt(curve: readonly CurvePoint[], t: number): number {
  if (curve.length === 0) return NaN;
  if (t <= curve[0].t) return curve[0].bt;
  const last = curve[curve.length - 1];
  if (t >= last.t) return last.bt;
  let i = 1;
  while (curve[i].t < t) i++;
  const a = curve[i - 1];
  const b = curve[i];
  return lerp(a.bt, b.bt, (t - a.t) / (b.t - a.t));
}

/** The turning point: the lowest temperature in the first three minutes. */
export function turningPoint(curve: readonly CurvePoint[]): CurvePoint {
  let best = curve[0] ?? { t: 0, bt: 0 };
  for (const p of curve) {
    if (p.t > 180) break;
    if (p.bt < best.bt) best = p;
  }
  return best;
}

/** Resamples to every `STEP_S` seconds from 0 to `end`. */
function resample(curve: readonly CurvePoint[], end: number): CurvePoint[] {
  const out: CurvePoint[] = [];
  for (let t = 0; t < end; t += STEP_S) out.push({ t, bt: btAt(curve, t) });
  out.push({ t: end, bt: btAt(curve, end) });
  return out;
}

/** A centred moving average over about `windowS` seconds, keeping the ends. */
function smooth(curve: readonly CurvePoint[], windowS: number): CurvePoint[] {
  const half = Math.max(1, Math.round(windowS / STEP_S / 2));
  return curve.map((p, i) => {
    if (i < half || i >= curve.length - half) return p;
    let sum = 0;
    for (let j = i - half; j <= i + half; j++) sum += curve[j].bt;
    return { t: p.t, bt: sum / (2 * half + 1) };
  });
}

/** A roast's curve from CHARGE to DROP and its milestones, or null without CHARGE. */
export function fromRoast(roast: Roast): { curve: CurvePoint[]; milestones: Milestones } | null {
  const events = roast.events ?? [];
  const charge = events.find((e) => e.kind === "charge")?.t;
  if (charge === undefined) return null;
  const raw: CurvePoint[] = roast.samples
    .filter((s) => s.t >= charge && typeof s.bt === "number")
    .map((s) => ({ t: s.t - charge, bt: s.bt as number }));
  if (raw.length < 10) return null;
  const at = (kind: string) => {
    const e = events.find((x) => x.kind === kind);
    return e ? e.t - charge : undefined;
  };
  const end = at("drop") ?? raw[raw.length - 1].t;
  const curve = smooth(resample(raw, end), 6);
  const point = (t: number | undefined, fallbackBt: number): CurvePoint => {
    if (t !== undefined) return { t, bt: btAt(curve, t) };
    // Without the event: where the curve first reaches a usual temperature.
    const hit = curve.find((p) => p.t > 90 && p.bt >= fallbackBt);
    return hit ?? { t: end * (fallbackBt < 180 ? 0.45 : 0.8), bt: fallbackBt };
  };
  return {
    curve,
    milestones: {
      charge: curve[0].bt,
      dryEnd: point(at("dry_end"), 150),
      firstCrack: point(at("first_crack_start"), 193),
      drop: { t: end, bt: btAt(curve, end) },
    },
  };
}

/** Anchors of a curve for warping: CHARGE, turning point, dry end, first crack, drop. */
function anchors(m: Milestones, tp: CurvePoint): CurvePoint[] {
  return [{ t: 0, bt: m.charge }, tp, m.dryEnd, m.firstCrack, m.drop];
}

/**
 * Moves a curve's milestones from `from` to `to`: time is stretched linearly
 * between neighbouring milestones, temperature shifted by an offset that
 * changes linearly between them. The turning point stays where it is.
 */
export function warp(curve: readonly CurvePoint[], from: Milestones, to: Milestones): CurvePoint[] {
  const tp = turningPoint(curve);
  const a = anchors(from, tp);
  const b = anchors(to, tp);
  const out: CurvePoint[] = [];
  for (const p of curve) {
    if (p.t > from.drop.t) break;
    let i = 0;
    while (i < a.length - 2 && p.t > a[i + 1].t) i++;
    const span = a[i + 1].t - a[i].t;
    const f = span > 0 ? (p.t - a[i].t) / span : 0;
    out.push({
      t: lerp(b[i].t, b[i + 1].t, f),
      bt: p.bt + lerp(b[i].bt - a[i].bt, b[i + 1].bt - a[i + 1].bt, f),
    });
  }
  return resample(out, to.drop.t);
}

/** Checks that milestones are in roast order with time between them. */
export function milestonesProblem(m: Milestones): string | null {
  const ts = [m.dryEnd.t, m.firstCrack.t, m.drop.t];
  if (!(m.dryEnd.t > 120)) return "Dry end must come after 2:00.";
  if (ts.some((t, i) => i > 0 && t <= ts[i - 1] + 30))
    return "Dry end, first crack and drop need at least 30 s between them.";
  if (!(m.dryEnd.bt < m.firstCrack.bt && m.firstCrack.bt < m.drop.bt))
    return "The temperatures must rise from dry end to first crack to drop.";
  return null;
}

/** Rate of rise, °C/min: least-squares slope over the trailing 20 s, like the daemon. */
export function rorOf(curve: readonly CurvePoint[], windowS = 20): (number | null)[] {
  return curve.map((p, i) => {
    let j = i;
    while (j > 0 && p.t - curve[j - 1].t <= windowS) j--;
    const w = curve.slice(j, i + 1);
    if (w.length < 3) return null;
    const mt = w.reduce((s, q) => s + q.t, 0) / w.length;
    const mb = w.reduce((s, q) => s + q.bt, 0) / w.length;
    let num = 0;
    let den = 0;
    for (const q of w) {
      num += (q.t - mt) * (q.bt - mb);
      den += (q.t - mt) ** 2;
    }
    return den > 0 ? (num / den) * 60 : null;
  });
}

/** Development, maillard and drying in percent of the roast time. */
export function phaseShares(m: Milestones): {
  drying: number;
  maillard: number;
  development: number;
} {
  const total = m.drop.t || 1;
  return {
    drying: (m.dryEnd.t / total) * 100,
    maillard: ((m.firstCrack.t - m.dryEnd.t) / total) * 100,
    development: ((m.drop.t - m.firstCrack.t) / total) * 100,
  };
}

const round1 = (v: number) => Math.round(v * 10) / 10;

/** The plan to save: the curve with its rate of rise, the milestones and the recipe. */
export function toTarget(
  curve: readonly CurvePoint[],
  m: Milestones,
  actions: readonly PlannedAction[],
): ProfileTarget {
  const ror = rorOf(curve);
  const points: TargetPoint[] = curve.map((p, i) => ({
    roast_time_s: round1(p.t),
    bt_c: round1(p.bt),
    ...(ror[i] !== null ? { ror_c_per_min: round1(ror[i] as number) } : {}),
  }));
  // Rounding must not make two times equal.
  const unique = points.filter((p, i) => i === 0 || p.roast_time_s > points[i - 1].roast_time_s);
  return {
    points: unique,
    actions: sortActions(actions),
    events: {
      dry_end: { roast_time_s: round1(m.dryEnd.t), bt_c: round1(m.dryEnd.bt) },
      first_crack: { roast_time_s: round1(m.firstCrack.t), bt_c: round1(m.firstCrack.bt) },
      drop: { rule: "at_time", roast_time_s: round1(m.drop.t) },
    },
  };
}

/** A stored plan back to curve and milestones. */
export function fromTarget(target: ProfileTarget): { curve: CurvePoint[]; milestones: Milestones } {
  const curve = target.points.map((p) => ({ t: p.roast_time_s, bt: p.bt_c }));
  const end =
    target.events.drop?.rule === "at_time"
      ? target.events.drop.roast_time_s
      : (curve[curve.length - 1]?.t ?? 0);
  const ev = (e: { roast_time_s?: number | null; bt_c?: number | null } | null | undefined) => {
    const t = e?.roast_time_s ?? 0;
    return { t, bt: e?.bt_c ?? btAt(curve, t) };
  };
  return {
    curve,
    milestones: {
      charge: curve[0]?.bt ?? 200,
      dryEnd: ev(target.events.dry_end),
      firstCrack: ev(target.events.first_crack),
      drop: { t: end, bt: btAt(curve, end) },
    },
  };
}

/** Time steps first in time order, then temperature steps by temperature. */
export function sortActions(actions: readonly PlannedAction[]): PlannedAction[] {
  const key = (a: PlannedAction) =>
    a.trigger.at === "time" ? a.trigger.roast_time_s : 10_000 + a.trigger.bt_c;
  return [...actions].sort((p, q) => key(p) - key(q));
}

/**
 * The recipe a roast followed: a step whenever burner or air move to a new
 * value. Changes less than `mergeS` apart count as one move (Artisan logs a
 * slider move as a short ramp), to its last value, in whole percent.
 */
export function recipeFromRoast(roast: Roast, mergeS = 8): PlannedAction[] {
  const events = roast.events ?? [];
  const charge = events.find((e) => e.kind === "charge")?.t;
  if (charge === undefined) return [];
  const drop = events.find((e) => e.kind === "drop")?.t ?? Infinity;
  const out: PlannedAction[] = [];
  for (const actuator of ["burner", "air"] as Actuator[]) {
    const changes: { t: number; v: number }[] = [];
    let last: number | null = null;
    for (const s of roast.samples) {
      if (s.t < charge - 2 || s.t > drop) continue;
      const v = s.actuators?.[actuator];
      if (typeof v !== "number") continue;
      if (last === null || Math.abs(v - last) >= 0.5)
        changes.push({ t: Math.max(0, s.t - charge), v });
      last = v;
    }
    let committed: number | null = null;
    let i = 0;
    while (i < changes.length) {
      let j = i;
      while (j + 1 < changes.length && changes[j + 1].t - changes[j].t < mergeS) j++;
      const v = Math.round(changes[j].v);
      if (v !== committed) {
        out.push({
          trigger: { at: "time", roast_time_s: Math.round(changes[i].t) },
          actuator,
          value: v,
        });
        committed = v;
      }
      i = j + 1;
    }
  }
  return sortActions(out);
}

/** A curve for an empty start: a typical 200 g roast on the Cubean+, shaped by its milestones. */
export function templateCurve(m: Milestones): CurvePoint[] {
  // Charge to turning point: the probe falls quickly to about 98 °C at 1:10.
  const tp: CurvePoint = { t: 70, bt: 98 };
  const pts: CurvePoint[] = [];
  for (let t = 0; t < tp.t; t += STEP_S) {
    const f = Math.exp(-t / 18);
    const end = Math.exp(-tp.t / 18);
    pts.push({ t, bt: tp.bt + (m.charge - tp.bt) * ((f - end) / (1 - end)) });
  }
  // Turning point to drop: a monotone cubic through the milestones, flat at the turning point.
  const knots = [tp, m.dryEnd, m.firstCrack, m.drop];
  const d = knots.map((k, i) => {
    if (i === 0) return 0;
    const prev = (k.bt - knots[i - 1].bt) / (k.t - knots[i - 1].t);
    if (i === knots.length - 1) return prev;
    const next = (knots[i + 1].bt - k.bt) / (knots[i + 1].t - k.t);
    return prev * next <= 0 ? 0 : (2 * prev * next) / (prev + next);
  });
  for (let i = 0; i < knots.length - 1; i++) {
    const a = knots[i];
    const b = knots[i + 1];
    const h = b.t - a.t;
    for (let t = a.t; t < b.t; t += STEP_S) {
      const s = (t - a.t) / h;
      const h00 = 2 * s ** 3 - 3 * s ** 2 + 1;
      const h10 = s ** 3 - 2 * s ** 2 + s;
      const h01 = -2 * s ** 3 + 3 * s ** 2;
      const h11 = s ** 3 - s ** 2;
      pts.push({ t, bt: h00 * a.bt + h10 * h * d[i] + h01 * b.bt + h11 * h * d[i + 1] });
    }
  }
  pts.push({ t: m.drop.t, bt: m.drop.bt });
  return pts;
}

/** Milestones of a typical espresso roast of 200 g on the Cubean+ (the roaster's own roasts). */
export const TEMPLATE_MILESTONES: Milestones = {
  charge: 200,
  dryEnd: { t: 280, bt: 155 },
  firstCrack: { t: 525, bt: 193 },
  drop: { t: 640, bt: 204 },
};

/** Recipe steps due at the same roast time. */
export interface RecipeCue {
  /** Seconds since CHARGE. */
  at: number;
  /** What still has to change (in the future: everything planned then). */
  steps: PlannedAction[];
}

/**
 * The recipe step to announce now: the next time step whose values the
 * machine does not have yet. A step stays "due" for `graceS` after its time;
 * one that is already set (±1 %) counts as done.
 */
export function nextRecipeCue(
  actions: readonly PlannedAction[],
  sinceCharge: number,
  current: Partial<Record<Actuator, number>>,
  graceS = 30,
): RecipeCue | null {
  const groups = new Map<number, PlannedAction[]>();
  for (const a of actions) {
    if (a.trigger.at !== "time") continue;
    const at = a.trigger.roast_time_s;
    groups.set(at, [...(groups.get(at) ?? []), a]);
  }
  for (const at of [...groups.keys()].sort((p, q) => p - q)) {
    if (at < sinceCharge - graceS) continue;
    const pending = (groups.get(at) ?? []).filter(
      (s) =>
        Math.abs((current[s.actuator] ?? Number.NaN) - s.value) >= 1 ||
        current[s.actuator] === undefined,
    );
    if (pending.length > 0) return { at, steps: pending };
  }
  return null;
}
