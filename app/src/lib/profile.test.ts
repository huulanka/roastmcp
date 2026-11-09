import { describe, expect, it } from "vitest";

import type { Roast } from "@/api/types.ts";
import {
  TEMPLATE_MILESTONES,
  btAt,
  fromRoast,
  fromTarget,
  milestonesProblem,
  nextRecipeCue,
  phaseShares,
  recipeFromRoast,
  rorOf,
  templateCurve,
  toTarget,
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

describe("recipe cues", () => {
  const step = (t: number, actuator: "burner" | "air", value: number) => ({
    trigger: { at: "time" as const, roast_time_s: t },
    actuator,
    value,
  });
  const recipe = [
    step(0, "burner", 80),
    step(0, "air", 20),
    step(90, "burner", 85),
    step(290, "air", 30),
  ];

  it("announces what is not set yet, grouped by time", () => {
    expect(nextRecipeCue(recipe, 2, { burner: 80, air: 100 })).toEqual({
      at: 0,
      steps: [step(0, "air", 20)],
    });
    expect(nextRecipeCue(recipe, 60, { burner: 80, air: 20 })?.at).toBe(90);
  });

  it("drops a step once it is set or long past", () => {
    expect(nextRecipeCue(recipe, 100, { burner: 85, air: 20 })?.at).toBe(290);
    expect(nextRecipeCue(recipe, 130, { burner: 70, air: 20 })?.at).toBe(290);
    expect(nextRecipeCue(recipe, 400, { burner: 70, air: 30 })).toBeNull();
  });
});
