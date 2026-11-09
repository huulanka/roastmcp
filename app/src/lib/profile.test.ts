import { describe, expect, it } from "vitest";

import type { Roast } from "@/api/types.ts";
import {
  TEMPLATE_MILESTONES,
  btAt,
  fromRoast,
  fromTarget,
  milestonesProblem,
  phaseShares,
  plannedTime,
  recipeFromRoast,
  rorOf,
  sortActions,
  templateCurve,
  toTarget,
  triggerLabel,
  warp,
} from "./profile.ts";

/** A roast with a smooth curve: charge at 20 s, preheat before it. */
function roast(): Roast {
  const samples = [];
  for (let t = 0; t <= 700; t += 1.5) {
    const r = t - 20;
    const bt =
      r < 0
        ? 200
        : r < 70
          ? 98 + 102 * Math.exp(-r / 15)
          : 98 + (206 - 98) * (1 - Math.exp(-(r - 70) / 300));
    const burner = r < 90 ? 80 : r < 92 ? 82.5 : r < 300 ? 85 : 70;
    const air = r < 290 ? 20 : 30;
    samples.push({ t, bt, actuators: { burner, air } });
  }
  return {
    id: "r",
    samples,
    events: [
      { t: 20, kind: "charge", origin: "auto", bt: 200 },
      { t: 300, kind: "dry_end", origin: "auto", bt: null },
      { t: 545, kind: "first_crack_start", origin: "human", bt: null },
      { t: 660, kind: "drop", origin: "auto", bt: null },
    ],
  } as Roast;
}

describe("a profile from a roast", () => {
  it("starts at CHARGE and ends at DROP, with its milestones", () => {
    const r = fromRoast(roast());
    expect(r).not.toBeNull();
    const { curve, milestones } = r!;
    expect(curve[0].t).toBe(0);
    expect(curve[curve.length - 1].t).toBe(640);
    expect(milestones.dryEnd.t).toBe(280);
    expect(milestones.firstCrack.t).toBe(525);
    expect(milestones.drop.t).toBe(640);
    expect(milestones.charge).toBeGreaterThan(150);
  });

  it("reads the recipe: one step per move, ramps merged, whole percent", () => {
    // Times to the nearest 5 s: the readings come every 1.5 s.
    const steps = recipeFromRoast(roast()).map((a) => [
      a.trigger.at === "time" ? Math.round(a.trigger.roast_time_s / 5) * 5 : null,
      a.actuator,
      a.value,
    ]);
    expect(steps).toEqual([
      [0, "burner", 80],
      [0, "air", 20],
      [90, "burner", 85],
      [290, "air", 30],
      [300, "burner", 70],
    ]);
  });
});

describe("moving milestones", () => {
  it("lands the curve on the new milestones and keeps the rest", () => {
    const { curve, milestones } = fromRoast(roast())!;
    const to = {
      ...milestones,
      firstCrack: { t: milestones.firstCrack.t + 20, bt: milestones.firstCrack.bt + 2 },
      drop: { t: milestones.drop.t + 30, bt: milestones.drop.bt + 3 },
    };
    const moved = warp(curve, milestones, to);
    expect(btAt(moved, to.firstCrack.t)).toBeCloseTo(to.firstCrack.bt, 0);
    expect(moved[moved.length - 1].t).toBe(to.drop.t);
    expect(btAt(moved, to.drop.t)).toBeCloseTo(to.drop.bt, 0);
    // Before the dry end nothing moved.
    expect(btAt(moved, 200)).toBeCloseTo(btAt(curve, 200), 5);
  });

  it("checks the order of the milestones", () => {
    expect(milestonesProblem(TEMPLATE_MILESTONES)).toBeNull();
    expect(
      milestonesProblem({ ...TEMPLATE_MILESTONES, firstCrack: { t: 290, bt: 193 } }),
    ).not.toBeNull();
    expect(milestonesProblem({ ...TEMPLATE_MILESTONES, drop: { t: 640, bt: 190 } })).not.toBeNull();
  });

  it("computes the phases", () => {
    const p = phaseShares(TEMPLATE_MILESTONES);
    expect(p.drying + p.maillard + p.development).toBeCloseTo(100);
    expect(p.development).toBeCloseTo((115 / 640) * 100);
  });
});

describe("stored plans", () => {
  it("round-trip through the saved target", () => {
    const curve = templateCurve(TEMPLATE_MILESTONES);
    const target = toTarget(curve, TEMPLATE_MILESTONES, []);
    expect(target.points[0].roast_time_s).toBe(0);
    expect(target.events.drop).toEqual({ rule: "at_time", roast_time_s: 640 });
    const back = fromTarget(target);
    expect(back.milestones.firstCrack).toEqual(TEMPLATE_MILESTONES.firstCrack);
    expect(back.milestones.drop.t).toBe(640);
    expect(back.milestones.drop.bt).toBeCloseTo(204, 0);
  });

  it("the template curve rises from the turning point through every milestone", () => {
    const curve = templateCurve(TEMPLATE_MILESTONES);
    expect(btAt(curve, 0)).toBeCloseTo(200, 0);
    expect(btAt(curve, 280)).toBeCloseTo(155, 0);
    expect(btAt(curve, 525)).toBeCloseTo(193, 0);
    const ror = rorOf(curve);
    const after = ror.filter((_, i) => curve[i].t > 120);
    expect(after.every((v) => v !== null && v > 0)).toBe(true);
  });
});

describe("steps after an event", () => {
  it("are named and sorted like the daemon does", () => {
    const fc = { at: "event", event: "first_crack_start", offset_s: 30 } as const;
    const de = { at: "event", event: "dry_end", offset_s: 0 } as const;
    expect(triggerLabel(fc)).toBe("FC +0:30");
    expect(triggerLabel(de)).toBe("DE +0:00");
    expect(triggerLabel({ at: "bt", bt_c: 185 })).toBe("185 °C");
    const sorted = sortActions([
      { trigger: fc, actuator: "burner", value: 60 },
      { trigger: de, actuator: "air", value: 40 },
      { trigger: { at: "time", roast_time_s: 90 }, actuator: "burner", value: 85 },
    ]);
    expect(sorted.map((s) => triggerLabel(s.trigger))).toEqual(["1:30", "DE +0:00", "FC +0:30"]);
  });

  it("sit at the planned event in the plan", () => {
    const m = TEMPLATE_MILESTONES;
    const curve = templateCurve(m);
    const fc = { at: "event", event: "first_crack_start", offset_s: 30 } as const;
    expect(plannedTime(fc, m, curve)).toBe(m.firstCrack.t + 30);
    expect(plannedTime({ at: "time", roast_time_s: 90 }, m, curve)).toBe(90);
    // A temperature step: where the curve reaches it after the turning point.
    const at = plannedTime({ at: "bt", bt_c: m.dryEnd.bt }, m, curve);
    expect(at).not.toBeNull();
    expect(Math.abs((at ?? 0) - m.dryEnd.t)).toBeLessThan(5);
    expect(plannedTime({ at: "bt", bt_c: 290 }, m, curve)).toBeNull();
  });
});
