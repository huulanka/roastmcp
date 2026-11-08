import { describe, expect, it } from "vitest";

import { computeRor } from "./ror.ts";

describe("computeRor", () => {
  it("is null until enough span, then the slope per minute", () => {
    const ts = Array.from({ length: 41 }, (_, i) => i);
    const bt = ts.map((t) => 100 + t * 0.2); // 12 °C/min
    const ror = computeRor(ts, bt);
    expect(ror[3]).toBeNull();
    expect(ror[40]).toBeCloseTo(12, 5);
  });
});
