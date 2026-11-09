import { describe, expect, it } from "vitest";

import type { RoastSummary } from "@/api/types.ts";
import {
  kg,
  lastRoastOfLot,
  parseNumber,
  planPreheat,
  planProgress,
  suggestedTargets,
} from "./plan.ts";

describe("preheat from the plan", () => {
  it("takes the CHARGE temperature and the recipe's air at CHARGE", () => {
    const at = (t: number, actuator: "burner" | "air", value: number) => ({
      trigger: { at: "time" as const, roast_time_s: t },
      actuator,
      value,
    });
    const recipe = [at(1, "burner", 80), at(1, "air", 20), at(300, "air", 40)];
    expect(planPreheat(199.6, recipe)).toEqual({ target: 200, air: 20 });
    expect(planPreheat(null, [at(300, "air", 40)])).toEqual({ target: null, air: null });
    expect(planPreheat(undefined, undefined)).toEqual({ target: null, air: null });
  });
});

const live = { t: 600, bt: 198.7, ror: 5, chargeT: 30, firstCrackT: 548 };

describe("plan progress", () => {
  it("counts down to the drop temperature at the current rate of rise", () => {
    const p = planProgress({ drop_bt_c: 204 }, live);
    expect(p.drop?.remaining).toBeCloseTo(5.3);
    // 5.3 °C at 5 °C/min ≈ 64 s
    expect(p.drop?.eta_s).toBeCloseTo(63.6, 0);
    expect(p.dtr).toBeNull();
  });

  it("has no countdown when the temperature does not rise or is reached", () => {
    expect(planProgress({ drop_bt_c: 204 }, { ...live, ror: 0.2 }).drop?.eta_s).toBeNull();
    const reached = planProgress({ drop_bt_c: 204 }, { ...live, bt: 205 }).drop;
    expect(reached?.remaining).toBeLessThan(0);
    expect(reached?.eta_s).toBeNull();
  });

  it("tracks the development time ratio from first crack", () => {
    // Roast time 570 s, first crack at 518 s: (570 - 518) / 570 ≈ 9.1 %.
    const p = planProgress({ dtr_pct: 21 }, live);
    expect(p.dtr?.now).toBeCloseTo(9.12, 1);
    // 518 / (1 - 0.21) ≈ 655.7 s after CHARGE
    expect(p.dtr?.at_s).toBeCloseTo(655.7, 0);
    const before = planProgress({ dtr_pct: 21 }, { ...live, firstCrackT: null });
    expect(before.dtr).toEqual({ target: 21, now: null, at_s: null });
  });

  it("ignores a plan without targets", () => {
    expect(planProgress({ goal: "espresso" }, live)).toEqual({ drop: null, dtr: null });
    expect(planProgress(null, live)).toEqual({ drop: null, dtr: null });
  });
});

describe("plan defaults", () => {
  const roast = (id: string, at: string, extra: Partial<RoastSummary> = {}): RoastSummary => ({
    id,
    title: id,
    roasted_at: at,
    total_s: 635,
    drop_bt: 203.8,
    green_weight_g: 200,
    roasted_weight_g: 172.7,
    lot_id: "lot",
    dtr_pct: 20.8,
    goal: "espresso",
    ...extra,
  });

  it("suggests the latest real roast of the same lot", () => {
    const roasts = [
      roast("old", "2026-09-01T10:00:00Z"),
      roast("new", "2026-09-07T10:00:00Z"),
      roast("sim", "2026-09-08T10:00:00Z", { simulated: true }),
      roast("other", "2026-09-09T10:00:00Z", { lot_id: "x" }),
      // An aborted recording (no CHARGE to DROP) is never a plan.
      roast("aborted", "2026-09-10T10:00:00Z", { total_s: null }),
    ];
    expect(lastRoastOfLot(roasts, "lot")?.id).toBe("new");
    expect(lastRoastOfLot(roasts, "lot", true)?.id).toBe("sim");
    expect(lastRoastOfLot(roasts, null)).toBeNull();
    expect(suggestedTargets(lastRoastOfLot(roasts, "lot"))).toEqual({
      goal: "espresso",
      drop: 204,
      dtr: 21,
    });
  });

  it("formats and parses amounts", () => {
    expect(kg(2973)).toBe("2.97 kg");
    expect(parseNumber("203,5")).toBe(203.5);
    expect(parseNumber(" ")).toBeNull();
  });
});
