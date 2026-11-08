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

export function buildChartData(points: readonly Point[], base: number | null): ChartData {
  const off = base ?? 0;
  const x = points.map((p) => p.t - off);
  const bt = points.map((p) => p.bt);
  const hasRor = points.some((p) => p.ror !== null && p.ror !== undefined);
  const ror = hasRor ? points.map((p) => p.ror ?? null) : computeRor(x, bt);
  return {
    x,
    bt,
    ror,
    burner: points.map((p) => p.actuators?.burner ?? null),
    air: points.map((p) => p.actuators?.air ?? null),
  };
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
 * other has a sample.
 */
export function mergeReference(
  data: ChartData,
  ref: readonly { t: number; bt: number | null; ror: number | null }[] | null,
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
    out.refRor.push(other ? ref[j].ror : undefined);
    if (live) i++;
    if (other) j++;
  }
  return out;
}
