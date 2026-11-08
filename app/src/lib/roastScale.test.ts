import { describe, expect, it } from "vitest";

import {
  ROAST_SCALE,
  rangeAt,
  rangeSpans,
  scaleGradient,
  scalePos,
  scaleTicks,
} from "./roastScale.ts";

describe("roast scale", () => {
  it("maps temperatures onto the bar and clamps", () => {
    expect(scalePos(ROAST_SCALE.min)).toBe(0);
    expect(scalePos(ROAST_SCALE.max)).toBe(1);
    expect(scalePos(20)).toBe(0);
    expect(scalePos(400)).toBe(1);
  });

  it("finds the range of a temperature", () => {
    expect(rangeAt(120).label).toBe("Drying");
    expect(rangeAt(150).label).toBe("Maillard");
    expect(rangeAt(201).label).toBe("Light");
    expect(rangeAt(210).label).toBe("Medium");
    expect(rangeAt(220).label).toBe("Medium-dark");
    expect(rangeAt(240).label).toBe("Dark");
  });

  it("has contiguous ranges that fill the bar", () => {
    const spans = rangeSpans();
    expect(spans[0].start).toBe(0);
    for (let i = 1; i < spans.length; i++)
      expect(spans[i].start).toBeCloseTo(spans[i - 1].start + spans[i - 1].width);
    const last = spans[spans.length - 1];
    expect(last.start + last.width).toBeCloseTo(1);
    expect(scaleTicks()).toEqual([150, 196, 205, 215, 224]);
  });

  it("builds the colour gradient from tokens only", () => {
    expect(scaleGradient()).toMatch(/^linear-gradient\(90deg, var\(--bean-green\) /);
    expect(scaleGradient()).not.toMatch(/#/);
  });
});
