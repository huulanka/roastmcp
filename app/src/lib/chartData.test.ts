import { describe, expect, it } from "vitest";

import { buildBands, buildChartData, chargeTime } from "./chartData.ts";

const ev = (t: number, kind: "charge" | "dry_end" | "first_crack_start") => ({
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
  it("builds phase bands relative to charge", () => {
    const events = [ev(10, "charge"), ev(100, "dry_end"), ev(200, "first_crack_start")];
    expect(chargeTime(events)).toBe(10);
    const bands = buildBands(events, 10, 300);
    expect(bands.map((b) => [b.from, b.to])).toEqual([
      [0, 90],
      [90, 190],
      [190, 290],
    ]);
  });
});
