import type { EventKind, Point, RoastEvent } from "@/api/types.ts";
import { computeRor } from "./ror.ts";

/** A value per x; `undefined` marks a point that only exists in another series (uPlot spans it). */
export type Series = (number | null | undefined)[];

export interface ChartData {
  /** Seconds relative to CHARGE (or recording start). */
  x: number[];
  bt: Series;
  ror: Series;
  burner: Series;
  air: Series;
  /** Reference roast, aligned on the same x (seconds since CHARGE). */
  refBt?: Series;
  refRor?: Series;
}
export interface ChartMarker {
  x: number;
  label: string;
}
export interface ChartBand {
  from: number;
  to: number;
}

/** English event names; the UI translates them. */
export const EVENT_NAME: Record<EventKind, string> = {
  charge: "Charge",
  turning_point: "Turning point",
  dry_end: "Dry end",
  first_crack_start: "First crack",
  first_crack_end: "First crack end",
  second_crack_start: "Second crack",
  second_crack_end: "Second crack end",
  drop: "Drop",
  cool_end: "Cooling end",
};

/** Roast order of the events. */
export const EVENT_ORDER: readonly EventKind[] = [
  "charge",
  "turning_point",
  "dry_end",
  "first_crack_start",
  "first_crack_end",
  "second_crack_start",
  "second_crack_end",
  "drop",
];

export function chargeTime(events: readonly RoastEvent[]): number | null {
  return events.find((e) => e.kind === "charge")?.t ?? null;
}

/**
 * Chart data relative to CHARGE (`base`). With a CHARGE the curve starts there
 * and ends at DROP (`endT`): the preheat before and the cooling after (Artisan
 * records both) stay in the file but are not part of the roast curve.
 * The rate of rise is shown from `rorFromT` on (the turning point), once it has
 * turned positive: between CHARGE and then the probe is only cooling down to
 * the beans, and the trailing RoR still carries that fall for a few seconds
 * after the turning point.
 */
export function buildChartData(
  points: readonly Point[],
  base: number | null,
  rorFromT: number | null = null,
  endT: number | null = null,
): ChartData {
  const off = base ?? 0;
  const kept =
    base === null ? points : points.filter((p) => p.t >= base && (endT === null || p.t <= endT));
  const x = kept.map((p) => p.t - off);
  const bt = kept.map((p) => p.bt);
  const hasRor = kept.some((p) => p.ror !== null && p.ror !== undefined);
  const raw = hasRor ? kept.map((p) => p.ror ?? null) : computeRor(x, bt);
  let shown = rorFromT === null;
  const ror = raw.map((v, i) => {
    if (shown || rorFromT === null) return v;
    const t = kept[i].t;
    if (t < rorFromT || v === null || v === undefined) return null;
    if (v < 0 && t - rorFromT < ROR_SETTLE_S) return null;
    shown = true;
    return v;
  });
  return {
    x,
    bt,
    ror,
    burner: kept.map((p) => p.actuators?.burner ?? null),
    air: kept.map((p) => p.actuators?.air ?? null),
  };
}

/** After the turning point, a negative RoR is still hidden for at most this long, s. */
const ROR_SETTLE_S = 30;

/** The end of the roast curve: DROP, if there is one. */
export function dropTime(events: readonly RoastEvent[]): number | null {
  return events.find((e) => e.kind === "drop")?.t ?? null;
}

/**
 * From when the rate of rise is drawn: the turning point, or 90 s after CHARGE
 * while none is known yet; everything without a CHARGE (preheat).
 */
export function rorStart(events: readonly RoastEvent[], base: number | null): number | null {
  if (base === null) return null;
  return events.find((e) => e.kind === "turning_point")?.t ?? base + 90;
}

/** The usual RoR axis, °C/min. */
export const ROR_AXIS: readonly [number, number] = [0, 30];
/** The RoR axis never grows beyond this (the daemon drops faster changes as implausible). */
const ROR_LIMIT = 60;

/**
 * The RoR axis for the drawn values: 0-30 °C/min as usual, widened in steps of
 * 5 when the curve leaves it (a negative RoR after cutting the heat, a crash,
 * a fast preheat), never beyond ±60.
 */
export function rorRange(min: number | null, max: number | null): [number, number] {
  const [lo0, hi0] = ROR_AXIS;
  const lo =
    min !== null && Number.isFinite(min) && min < lo0 ? Math.floor((min - 1) / 5) * 5 : lo0;
  const hi = max !== null && Number.isFinite(max) && max > hi0 ? Math.ceil((max + 1) / 5) * 5 : hi0;
  return [Math.max(-ROR_LIMIT, lo), Math.min(ROR_LIMIT, hi)];
}

/** Labels of the RoR axis strictly inside `[lo, hi]`: every 5, or every 10 on a wide axis. */
export function rorTicks(lo: number, hi: number): number[] {
  const step = hi - lo > 40 ? 10 : 5;
  const out: number[] = [];
  for (let v = Math.ceil(lo / step) * step; v < hi; v += step) if (v > lo) out.push(v);
  return out;
}

export function buildMarkers(
  events: readonly RoastEvent[],
  base: number | null,
  label: (kind: EventKind) => string = (k) => EVENT_NAME[k],
): ChartMarker[] {
  const off = base ?? 0;
  return events.map((e) => ({ x: e.t - off, label: label(e.kind) }));
}

/** The development phase (first crack to drop, or to now), relative to CHARGE. */
export function developmentBand(
  events: readonly RoastEvent[],
  base: number | null,
  latestT: number | null,
): ChartBand | null {
  if (base === null) return null;
  const fc = events.find((e) => e.kind === "first_crack_start")?.t;
  const end = events.find((e) => e.kind === "drop")?.t ?? latestT ?? undefined;
  if (fc === undefined || end === undefined || end <= fc) return null;
  return { from: fc - base, to: end - base };
}

/**
 * Adds a reference curve (`t` in seconds since its CHARGE) to live data whose x
 * is already relative to the live CHARGE: both share one x axis, so the two
 * sorted x lists are merged and each side gets `undefined` where only the
 * other has a sample. The reference's RoR is drawn from `refRorFrom` on (its
 * turning point, seconds since its CHARGE), like the live one.
 */
export function mergeReference(
  data: ChartData,
  ref: readonly { t: number; bt: number | null; ror: number | null }[] | null,
  refRorFrom: number | null = null,
): ChartData {
  if (!ref || ref.length === 0) return data;
  const out: Required<ChartData> = {
    x: [],
    bt: [],
    ror: [],
    burner: [],
    air: [],
    refBt: [],
    refRor: [],
  };
  let i = 0;
  let j = 0;
  while (i < data.x.length || j < ref.length) {
    const lx = i < data.x.length ? data.x[i] : Infinity;
    const rx = j < ref.length ? ref[j].t : Infinity;
    const x = Math.min(lx, rx);
    const live = lx === x;
    const other = rx === x;
    out.x.push(x);
    out.bt.push(live ? data.bt[i] : undefined);
    out.ror.push(live ? data.ror[i] : undefined);
    out.burner.push(live ? data.burner[i] : undefined);
    out.air.push(live ? data.air[i] : undefined);
    out.refBt.push(other ? ref[j].bt : undefined);
    out.refRor.push(
      other ? (refRorFrom !== null && ref[j].t < refRorFrom ? null : ref[j].ror) : undefined,
    );
    if (live) i++;
    if (other) j++;
  }
  return out;
}
