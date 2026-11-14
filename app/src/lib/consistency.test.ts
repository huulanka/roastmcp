import { describe, expect, it } from "vitest";

import type { ConsistencyResult } from "@/api/types.ts";
import { furthest, position, unusual } from "./consistency.ts";

const spread = (values: number[]) => {
  const mean = values.reduce((a, b) => a + b, 0) / values.length;
  const sd = Math.sqrt(values.reduce((a, b) => a + (b - mean) ** 2, 0) / (values.length - 1));
  return { n: values.length, mean, sd, min: Math.min(...values), max: Math.max(...values) };
};

const drops = [206, 207, 206, 206, 213];
const result: ConsistencyResult = {
  consistency: { roasts: 5, curve_sd_c: 2.8, drop_bt_c: spread(drops), total_s: null },
  roasts: drops.map((d, i) => ({
    id: `r${i}`,
    title: "Yellow Bourbon",
    roasted_at: "2026-11-09T18:00:00Z",
    batch_no: `BRYB-00${i}`,
    figures: { drop_bt_c: d },
  })),
  unfinished: 0,
};

describe("consistency", () => {
  it("places a value between the smallest and the largest", () => {
    expect(position(209.5, spread(drops))).toBeCloseTo(0.5);
    expect(position(300, spread(drops))).toBe(1);
    expect(position(5, { n: 2, mean: 5, sd: 0, min: 5, max: 5 })).toBe(0.5);
  });

  it("finds the roast furthest from the mean", () => {
    const f = furthest(result);
    expect(f?.label).toBe("BRYB-004");
    expect(f?.key).toBe("drop_bt_c");
    expect(f?.delta).toBeCloseTo(5.4);
  });

  it("names a roast's unusual figures and none for a usual one", () => {
    expect(unusual(result, "r4").map((d) => d.key)).toEqual(["drop_bt_c"]);
    expect(unusual(result, "r0")).toEqual([]);
  });
});
