import type { ConsistencyResult, RoastFigures, Spread, SpreadKey } from "@/api/types.ts";

/** How a figure is shown: a clock, a temperature or a percentage. */
export type FigureKind = "clock" | "temp" | "pct";

/** The figures the consistency view lists, in order, with their English names. */
export const FIGURES: { key: SpreadKey; name: string; kind: FigureKind }[] = [
  { key: "first_crack_s", name: "To first crack", kind: "clock" },
  { key: "total_s", name: "Duration", kind: "clock" },
  { key: "drop_bt_c", name: "Drop", kind: "temp" },
  { key: "development_s", name: "Development time", kind: "clock" },
  { key: "dtr_pct", name: "Development (DTR)", kind: "pct" },
  { key: "weight_loss_pct", name: "Weight loss", kind: "pct" },
  { key: "charge_bt_c", name: "Charge", kind: "temp" },
];

/** Beyond this many standard deviations from the mean a figure counts as unusual. */
export const UNUSUAL_SD = 1.5;

/** Where a value sits between the smallest and the largest, 0 to 1 (0.5 without a range). */
export function position(value: number, spread: Spread): number {
  const range = spread.max - spread.min;
  if (range <= 0) return 0.5;
  return Math.min(1, Math.max(0, (value - spread.min) / range));
}

/** One figure that stands out: whose, which and how far from the mean. */
export interface Deviation {
  id: string;
  label: string;
  key: SpreadKey;
  /** Value minus mean. */
  delta: number;
  /** In standard deviations. */
  z: number;
}

function deviations(result: ConsistencyResult, figures: RoastFigures, id: string, label: string) {
  const out: Deviation[] = [];
  for (const f of FIGURES) {
    const spread = result.consistency[f.key];
    const value = figures[f.key];
    if (!spread || spread.n < 3 || spread.sd <= 0 || typeof value !== "number") continue;
    const delta = value - spread.mean;
    out.push({ id, label, key: f.key, delta, z: delta / spread.sd });
  }
  return out;
}

/** The roast and figure furthest from the mean, or `null` under three roasts. */
export function furthest(result: ConsistencyResult): Deviation | null {
  let best: Deviation | null = null;
  for (const r of result.roasts) {
    const label = r.batch_no ?? r.title;
    for (const d of deviations(result, r.figures, r.id, label)) {
      if (!best || Math.abs(d.z) > Math.abs(best.z)) best = d;
    }
  }
  return best;
}

/** The figures of one roast outside the usual spread, furthest first. */
export function unusual(result: ConsistencyResult, id: string): Deviation[] {
  const r = result.roasts.find((x) => x.id === id);
  if (!r) return [];
  return deviations(result, r.figures, r.id, r.batch_no ?? r.title)
    .filter((d) => Math.abs(d.z) > UNUSUAL_SD)
    .sort((a, b) => Math.abs(b.z) - Math.abs(a.z));
}
