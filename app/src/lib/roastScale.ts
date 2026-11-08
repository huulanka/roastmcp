/**
 * The roast-degree scale: bean temperature mapped to roast phases and degrees.
 * Approximate defaults in one place so they can become a setting later.
 */

export interface RoastRange {
  /** English label; the UI translates it. */
  label: string;
  /** Lower bound in °C (inclusive), null = open. */
  from: number | null;
  /** Upper bound in °C (exclusive), null = open. */
  to: number | null;
}

export interface RoastScale {
  /** Shown span of the bar, °C. */
  min: number;
  max: number;
  ranges: RoastRange[];
  /** Bean colour by temperature: CSS custom property names from tokens.css. */
  colours: { bt: number; token: string }[];
}

export const ROAST_SCALE: RoastScale = {
  min: 140,
  max: 235,
  ranges: [
    { label: "Drying", from: null, to: 150 },
    { label: "Maillard", from: 150, to: 196 },
    { label: "Light", from: 196, to: 205 },
    { label: "Medium", from: 205, to: 215 },
    { label: "Medium-dark", from: 215, to: 224 },
    { label: "Dark", from: 224, to: null },
  ],
  colours: [
    { bt: 150, token: "--bean-green" },
    { bt: 168, token: "--bean-yellow" },
    { bt: 196, token: "--bean-cinnamon" },
    { bt: 212, token: "--bean-brown" },
    { bt: 228, token: "--bean-dark" },
  ],
};

/** Position of a temperature on the bar, 0–1 (clamped). */
export function scalePos(bt: number, scale: RoastScale = ROAST_SCALE): number {
  return Math.min(1, Math.max(0, (bt - scale.min) / (scale.max - scale.min)));
}

/** The range a temperature falls into. */
export function rangeAt(bt: number, scale: RoastScale = ROAST_SCALE): RoastRange {
  return (
    scale.ranges.find((r) => (r.from === null || bt >= r.from) && (r.to === null || bt < r.to)) ??
    scale.ranges[scale.ranges.length - 1]
  );
}

/** Each range with its start and width on the bar (0–1). */
export function rangeSpans(
  scale: RoastScale = ROAST_SCALE,
): { range: RoastRange; start: number; width: number }[] {
  return scale.ranges.map((range) => {
    const start = scalePos(range.from ?? scale.min, scale);
    const end = scalePos(range.to ?? scale.max, scale);
    return { range, start, width: end - start };
  });
}

/** The inner range boundaries, °C. */
export function scaleTicks(scale: RoastScale = ROAST_SCALE): number[] {
  return scale.ranges.flatMap((r) => (r.from === null ? [] : [r.from]));
}

/** CSS gradient of bean colour along the bar. */
export function scaleGradient(scale: RoastScale = ROAST_SCALE): string {
  const stops = scale.colours.map(
    (c) => `var(${c.token}) ${(scalePos(c.bt, scale) * 100).toFixed(1)}%`,
  );
  return `linear-gradient(90deg, ${stops.join(", ")})`;
}
