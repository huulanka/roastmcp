import type { Lot, PlannedAction, RoastGoal, RoastPlan, RoastSummary } from "@/api/types.ts";

/**
 * The roast plan's two targets (ADR 0014) against the live roast: how far the
 * drop temperature is, and where the development time ratio stands. Times are
 * recording seconds, as in the snapshot.
 */
export interface PlanProgress {
  drop: {
    target: number;
    /** °C still to go; 0 or less once reached. */
    remaining: number;
    /** Seconds until the target at the current rate of rise, if it rises. */
    eta_s: number | null;
  } | null;
  dtr: {
    target: number;
    /** Percent now; null before first crack. */
    now: number | null;
    /** Roast time (seconds after CHARGE) at which the target ratio is reached. */
    at_s: number | null;
  } | null;
}

/** Below this rate of rise (°C/min) a countdown would be meaningless. */
const MIN_ROR_FOR_ETA = 0.5;

export function planProgress(
  plan: RoastPlan | null | undefined,
  live: {
    t: number | null;
    bt: number | null;
    ror: number | null;
    chargeT: number | null;
    firstCrackT: number | null;
  },
): PlanProgress {
  const drop =
    plan?.drop_bt_c !== undefined && plan.drop_bt_c !== null
      ? (() => {
          const target = plan.drop_bt_c;
          const remaining = live.bt === null ? target : target - live.bt;
          const eta_s =
            live.bt !== null && remaining > 0 && live.ror !== null && live.ror > MIN_ROR_FOR_ETA
              ? (remaining / live.ror) * 60
              : null;
          return { target, remaining, eta_s };
        })()
      : null;

  const dtr =
    plan?.dtr_pct !== undefined && plan.dtr_pct !== null && plan.dtr_pct > 0 && plan.dtr_pct < 100
      ? (() => {
          const target = plan.dtr_pct;
          const { t, chargeT, firstCrackT } = live;
          if (chargeT === null || firstCrackT === null || firstCrackT <= chargeT) {
            return { target, now: null, at_s: null };
          }
          const fc = firstCrackT - chargeT;
          const elapsed = t === null ? null : t - chargeT;
          const now = elapsed !== null && elapsed > fc ? ((elapsed - fc) / elapsed) * 100 : 0;
          // (T - fc) / T = d  ⇒  T = fc / (1 - d)
          const at_s = fc / (1 - target / 100);
          return { target, now, at_s };
        })()
      : null;

  return { drop, dtr };
}

/** A roast that ran from CHARGE to DROP (not an aborted recording). */
export function isComplete(r: RoastSummary): boolean {
  return typeof r.total_s === "number" && r.total_s > 0;
}

/**
 * The most recent complete roast of a lot: the default plan. On the real
 * roaster only real roasts count; on the simulator simulated ones do too.
 */
export function lastRoastOfLot(
  roasts: readonly RoastSummary[],
  lotId: string | null,
  simulated = false,
): RoastSummary | null {
  if (!lotId) return null;
  return (
    [...roasts]
      .filter((r) => r.lot_id === lotId && isComplete(r) && (simulated || !r.simulated))
      .sort((a, b) => b.roasted_at.localeCompare(a.roasted_at))[0] ?? null
  );
}

/** Plan values a reference roast suggests: its goal, drop temperature and DTR. */
export function suggestedTargets(ref: RoastSummary | null): {
  goal: RoastGoal | null;
  drop: number | null;
  dtr: number | null;
} {
  return {
    goal: ref?.goal ?? null,
    drop: typeof ref?.drop_bt === "number" ? Math.round(ref.drop_bt) : null,
    dtr: typeof ref?.dtr_pct === "number" ? Math.round(ref.dtr_pct) : null,
  };
}

/** Kilograms with two decimals, for stock. */
export function kg(grams: number): string {
  return `${(grams / 1000).toFixed(2)} kg`;
}

/** The lots offered for a roast: in use, the ones with stock first. */
export function pickableLots(lots: readonly Lot[]): Lot[] {
  return lots
    .filter((l) => !l.archived)
    .sort((a, b) => Number(b.stock_g > 0) - Number(a.stock_g > 0) || a.name.localeCompare(b.name));
}

/** Parses a number typed with a comma or a point; null when empty or invalid. */
export function parseNumber(text: string): number | null {
  const v = parseFloat(text.replace(",", ".").trim());
  return Number.isFinite(v) ? v : null;
}

/**
 * What the plan says about the preheat (ADR 0018): the target is the plan's
 * bean temperature at CHARGE, the air its recipe's air at CHARGE (a step in
 * the first 10 s counts). Null where the plan says nothing.
 */
export function planPreheat(
  chargeBt: number | null | undefined,
  actions: readonly PlannedAction[] | undefined,
): { target: number | null; air: number | null } {
  const air = (actions ?? [])
    .filter((a) => a.actuator === "air" && a.trigger.at === "time" && a.trigger.roast_time_s <= 10)
    .sort(
      (p, q) =>
        (p.trigger.at === "time" ? p.trigger.roast_time_s : 0) -
        (q.trigger.at === "time" ? q.trigger.roast_time_s : 0),
    )[0];
  return {
    target: typeof chargeBt === "number" ? Math.round(chargeBt) : null,
    air: air ? Math.round(air.value) : null,
  };
}
