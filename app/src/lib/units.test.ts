import { describe, expect, it } from "vitest";

import { actuatorPercent, actuatorShown, formatClock, formatNumber, stepPercent } from "./units.ts";

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

describe("actuator display units", () => {
  // The Cubean+ drum: rpm = 0.65 × percent + 3 (80 % ≈ 55 rpm, 100 % ≈ 68 rpm).
  const rpm = { unit: "rpm", scale: 0.65, offset: 3 };

  it("shows percent as rpm and back", () => {
    expect(Math.round(actuatorShown(rpm, 80))).toBe(55);
    expect(Math.round(actuatorShown(rpm, 100))).toBe(68);
    expect(actuatorPercent(rpm, actuatorShown(rpm, 90))).toBeCloseTo(90);
    expect(actuatorShown(undefined, 42)).toBe(42);
  });

  it("steps one rpm at a time, always moving", () => {
    let pct = 80;
    const seen = [Math.round(actuatorShown(rpm, pct))];
    while (pct < 100) {
      pct = Math.min(100, stepPercent(rpm, pct, 1, 1));
      seen.push(Math.round(actuatorShown(rpm, pct)));
    }
    // Every whole rpm from 55 to 68 is reachable, none skipped twice.
    expect(seen[0]).toBe(55);
    expect(seen.at(-1)).toBe(68);
    for (let i = 1; i < seen.length; i++) expect(seen[i] - seen[i - 1]).toBeGreaterThan(0);
    expect(stepPercent(rpm, 100, -1, 1)).toBeLessThan(100);
  });

  it("steps percent points without a display scale", () => {
    expect(stepPercent(undefined, 40, 5, 1)).toBe(45);
    expect(stepPercent(undefined, 40, -1, 2)).toBe(38);
  });
});
