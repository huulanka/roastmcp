import { describe, expect, it } from "vitest";

import { formatClock, formatNumber } from "./units.ts";

describe("formatClock", () => {
  it("formats m:ss", () => {
    expect(formatClock(0)).toBe("0:00");
    expect(formatClock(75.9)).toBe("1:15");
    expect(formatClock(600)).toBe("10:00");
  });
  it("marks negative time", () => {
    expect(formatClock(-30)).toBe("-0:30");
  });
  it("handles unknown", () => {
    expect(formatClock(null)).toBe("–:––");
    expect(formatClock(NaN)).toBe("–:––");
  });
});

describe("formatNumber", () => {
  it("rounds and avoids negative zero", () => {
    expect(formatNumber(12.345)).toBe("12.3");
    expect(formatNumber(-0.01)).toBe("0.0");
    expect(formatNumber(null)).toBe("–");
  });
});
