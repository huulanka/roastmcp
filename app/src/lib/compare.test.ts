import { describe, expect, it } from "vitest";

import type { Roast } from "@/api/types.ts";
import {
  comparisonCurve,
  diff,
  firstCrackShift,
  furthestAt,
  signed,
  signedClock,
} from "./compare.ts";

describe("differences between two roasts", () => {
  it("carry their sign and vanish when they round to nothing", () => {
    expect(signed(1.24)).toBe("+1.2");
    expect(signed(-0.4)).toBe("−0.4");
    expect(signed(0.04)).toBeNull();
    expect(signed(2, 0)).toBe("+2");
    expect(signed(null)).toBeNull();
    expect(signedClock(12)).toBe("+0:12");
    expect(signedClock(-65)).toBe("−1:05");
    expect(signedClock(0.2)).toBeNull();
    expect(diff(3, 1)).toBe(2);
    expect(diff(3, null)).toBeNull();
  });
});

describe("the comparison curve", () => {
  it("runs from CHARGE to DROP on its own clock", () => {
    const roast: Roast = {
      id: "b",
      samples: [0, 100, 200, 210, 300, 400].map((t) => ({ t, bt: 100 + t / 10, ror: t - 205 })),
      events: [
        { kind: "charge", t: 100, bt: 200, origin: "human" },
        { kind: "turning_point", t: 200, bt: 90, origin: "auto" },
        { kind: "drop", t: 300, bt: 205, origin: "human" },
      ],
    } as Roast;
    const c = comparisonCurve(roast);
    expect(c.points.map((p) => p.t)).toEqual([0, 100, 110, 200]);
    // No RoR before the turning point, nor while it is still negative after.
    expect(c.points.map((p) => p.ror)).toEqual([null, null, 5, 95]);
    expect(c.rorFrom).toBe(100);
  });
});

describe("lining up on first crack", () => {
  it("moves the other roast by the difference of both first cracks", () => {
    expect(firstCrackShift(527, 545)).toBe(-18);
    expect(firstCrackShift(527, null)).toBe(0);
    const roast: Roast = {
      id: "b",
      samples: [100, 200].map((t) => ({ t, bt: 150, ror: 5 })),
      events: [{ kind: "charge", t: 100, bt: 200, origin: "human" }],
    } as Roast;
    expect(comparisonCurve(roast, -18).points.map((p) => p.t)).toEqual([-18, 82]);
  });

  it("finds where the curves are furthest apart", () => {
    const curve = [
      { t: -300, delta_bt: 1.2 },
      { t: -310, delta_bt: -4.1 },
      { t: 0, delta_bt: null },
    ];
    expect(furthestAt(curve, 4.1)).toBe(-310);
    expect(furthestAt(curve, null)).toBeNull();
  });
});
