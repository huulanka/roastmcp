import type { Roast } from "@/api/types.ts";
import { chargeTime, dropTime, rorStart } from "@/lib/chartData.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";

/** A difference with its sign ("+1.2", "−0.4"), or null when it rounds to nothing. */
export function signed(delta: number | null, decimals = 1): string | null {
  if (delta === null || !Number.isFinite(delta)) return null;
  const text = formatNumber(Math.abs(delta), decimals);
  if (Number(text) === 0) return null;
  return `${delta < 0 ? "−" : "+"}${text}`;
}

/** A time difference as a signed clock ("+0:12"), or null under a second. */
export function signedClock(delta: number | null): string | null {
  if (delta === null || !Number.isFinite(delta) || Math.abs(delta) < 0.5) return null;
  return `${delta < 0 ? "−" : "+"}${formatClock(Math.abs(delta))}`;
}

/** `a − b`, when both are known. */
export function diff(a: number | null | undefined, b: number | null | undefined): number | null {
  return typeof a === "number" && typeof b === "number" ? a - b : null;
}

/**
 * Another roast as the dashed comparison curve: its samples from CHARGE to
 * DROP on a clock that starts at its CHARGE, and from when its RoR is drawn.
 */
export function comparisonCurve(roast: Roast): {
  points: { t: number; bt: number | null; ror: number | null }[];
  rorFrom: number | null;
} {
  const events = roast.events ?? [];
  const base = chargeTime(events);
  const end = dropTime(events);
  const off = base ?? 0;
  const from = rorStart(events, base);
  let shown = from === null;
  const points = roast.samples
    .filter((s) => base === null || (s.t >= base && (end === null || s.t <= end)))
    .map((s) => {
      let ror = s.ror ?? null;
      // As on the roast's own curve: no RoR before the turning point, and
      // none while it is still negative shortly after.
      if (!shown && from !== null) {
        if (s.t < from || ror === null || (ror < 0 && s.t - from < 30)) ror = null;
        else shown = true;
      }
      return { t: s.t - off, bt: s.bt, ror };
    });
  return { points, rorFrom: from === null ? null : from - off };
}
