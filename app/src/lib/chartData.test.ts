import { describe, expect, it } from "vitest";

import { buildChartData, chargeTime, developmentBand, mergeReference } from "./chartData.ts";

const ev = (t: number, kind: "charge" | "dry_end" | "first_crack_start" | "drop") => ({
  t,
  kind,
  origin: "human" as const,
  bt: null,
});

describe("chart data", () => {
  it("shifts x by the charge time", () => {
    const d = buildChartData(
      [
        { t: 10, bt: 100, et: null, ror: 5, actuators: { burner: 50 } },
        { t: 11, bt: 101, et: null, ror: 6, actuators: {} },
      ],
      12,
    );
    expect(d.x).toEqual([-2, -1]);
    expect(d.burner).toEqual([50, null]);
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
});
