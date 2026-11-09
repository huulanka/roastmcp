import { describe, expect, it } from "vitest";

import {
  buildChartData,
  chargeTime,
  developmentBand,
  dropTime,
  mergeReference,
  rorRange,
  rorStart,
  rorTicks,
} from "./chartData.ts";

const ev = (t: number, kind: "charge" | "dry_end" | "first_crack_start" | "drop") => ({
  t,
  kind,
  origin: "human" as const,
  bt: null,
});

describe("chart data", () => {
  const pt = (t: number, ror: number | null, burner?: number) => ({
    t,
    bt: 100 + t,
    et: null,
    ror,
    actuators: burner === undefined ? {} : { burner },
  });

  it("starts at CHARGE and leaves the preheat out", () => {
    const d = buildChartData([pt(10, 5, 50), pt(11, 6), pt(12, -40, 80), pt(13, -30)], 12);
    expect(d.x).toEqual([0, 1]);
    expect(d.burner).toEqual([80, null]);
    expect(d.ror).toEqual([-40, -30]);
  });

  it("ends at DROP and leaves the cooling out", () => {
    const events = [ev(12, "charge"), ev(14, "drop")];
    const d = buildChartData(
      [pt(12, 1), pt(13, 2), pt(14, 3), pt(15, -50)],
      12,
      null,
      dropTime(events),
    );
    expect(d.x).toEqual([0, 1, 2]);
  });

  it("shows the whole curve before CHARGE (preheat)", () => {
    const d = buildChartData([pt(10, -15), pt(11, -12)], null, rorStart([], null));
    expect(d.x).toEqual([10, 11]);
    expect(d.ror).toEqual([-15, -12]);
  });

  it("draws the rate of rise from the turning point", () => {
    const events = [ev(10, "charge"), { ...ev(70, "dry_end"), kind: "turning_point" as const }];
    const from = rorStart(events, 10);
    expect(from).toBe(70);
    const d = buildChartData([pt(10, null), pt(40, -30), pt(70, 0), pt(100, 18)], 10, from);
    expect(d.ror).toEqual([null, null, 0, 18]);
    // At the turning point the trailing RoR still carries the fall after CHARGE.
    const e = buildChartData([pt(70, -22), pt(75, -6), pt(80, 2), pt(85, -1)], 10, 70);
    expect(e.ror).toEqual([null, null, 2, -1]);
    // ...but not for longer than 30 s.
    expect(buildChartData([pt(70, -9), pt(101, -4)], 10, 70).ror).toEqual([null, -4]);
    // No turning point yet: 90 s after CHARGE.
    expect(rorStart([ev(10, "charge")], 10)).toBe(100);
  });

  it("finds the development phase relative to charge", () => {
    const events = [ev(10, "charge"), ev(100, "dry_end"), ev(200, "first_crack_start")];
    expect(chargeTime(events)).toBe(10);
    expect(developmentBand(events, 10, 300)).toEqual({ from: 190, to: 290 });
    expect(developmentBand([...events, ev(260, "drop")], 10, 300)).toEqual({ from: 190, to: 250 });
    expect(developmentBand(events.slice(0, 2), 10, 300)).toBeNull();
    expect(developmentBand(events, null, 300)).toBeNull();
  });
});

describe("RoR axis", () => {
  it("keeps 0-30 for a normal roast", () => {
    expect(rorRange(2, 24)).toEqual([0, 30]);
    expect(rorRange(null, null)).toEqual([0, 30]);
    expect(rorTicks(0, 30)).toEqual([5, 10, 15, 20, 25]);
  });

  it("widens for a negative or a fast rate of rise, at most ±60", () => {
    expect(rorRange(-15.2, 24)).toEqual([-20, 30]);
    expect(rorRange(-1, 24)).toEqual([-5, 30]);
    expect(rorRange(3, 33)).toEqual([0, 35]);
    expect(rorRange(-200, 200)).toEqual([-60, 60]);
    expect(rorTicks(-10, 30)).toEqual([-5, 0, 5, 10, 15, 20, 25]);
    expect(rorTicks(-20, 30)).toEqual([-10, 0, 10, 20]);
    expect(rorTicks(-30, 30)).toEqual([-20, -10, 0, 10, 20]);
  });
});

describe("reference alignment", () => {
  const live = {
    x: [-2, 0, 2, 4],
    bt: [180, 170, 160, 150],
    ror: [null, null, -5, -4],
    burner: [50, 50, 60, 60],
    air: [40, 40, 40, 40],
  };

  it("merges both x lists in order and marks missing samples undefined", () => {
    const d = mergeReference(live, [
      { t: 0, bt: 175, ror: null },
      { t: 3, bt: 158, ror: -6 },
      { t: 6, bt: 140, ror: -3 },
    ]);
    expect(d.x).toEqual([-2, 0, 2, 3, 4, 6]);
    expect(d.bt).toEqual([180, 170, 160, undefined, 150, undefined]);
    expect(d.refBt).toEqual([undefined, 175, undefined, 158, undefined, 140]);
    expect(d.refRor).toEqual([undefined, null, undefined, -6, undefined, -3]);
    expect(d.burner).toEqual([50, 50, 60, undefined, 60, undefined]);
  });

  it("leaves live data untouched without a reference", () => {
    expect(mergeReference(live, null)).toBe(live);
    expect(mergeReference(live, [])).toBe(live);
  });

  it("draws the reference ahead of the live curve", () => {
    const d = mergeReference({ x: [], bt: [], ror: [], burner: [], air: [] }, [
      { t: 0, bt: 175, ror: null },
      { t: 60, bt: 120, ror: 8 },
    ]);
    expect(d.x).toEqual([0, 60]);
    expect(d.bt).toEqual([undefined, undefined]);
  });

  it("hides the reference's RoR before its turning point", () => {
    const d = mergeReference(
      { x: [], bt: [], ror: [], burner: [], air: [] },
      [
        { t: 30, bt: 110, ror: -40 },
        { t: 80, bt: 101, ror: 4 },
      ],
      70,
    );
    expect(d.refRor).toEqual([null, 4]);
  });
});
