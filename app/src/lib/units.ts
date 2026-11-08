import type { DisplayScale } from "@/api/types.ts";

/**
 * Units and number formatting. Everything internal is °C (roast-core
 * convention); conversion happens only here, at the display edge.
 */

export type TemperatureUnit = "C" | "F";

/** An absolute temperature in the display unit. */
export function toDisplayTemp(celsius: number, unit: TemperatureUnit): number {
  return unit === "F" ? (celsius * 9) / 5 + 32 : celsius;
}

/** A display-unit temperature back to °C. */
export function fromDisplayTemp(value: number, unit: TemperatureUnit): number {
  return unit === "F" ? ((value - 32) * 5) / 9 : value;
}

/**
 * A temperature *difference* or rate (for example rate of rise in °C/min) in
 * the display unit: scaled, never offset.
 */
export function toDisplayDelta(celsiusDelta: number, unit: TemperatureUnit): number {
  return unit === "F" ? (celsiusDelta * 9) / 5 : celsiusDelta;
}

export function unitSymbol(unit: TemperatureUnit): string {
  return unit === "F" ? "°F" : "°C";
}

/** Roast clock: `m:ss`, negative before CHARGE (`-0:30`). */
export function formatClock(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds)) return "–:––";
  const sign = seconds < 0 ? "-" : "";
  const whole = Math.floor(Math.abs(seconds));
  const m = Math.floor(whole / 60);
  const s = whole % 60;
  return `${sign}${m}:${s.toString().padStart(2, "0")}`;
}

/**
 * An actuator value as people read it: percent, or the machine's own unit
 * (drum rpm) where the profile has a display scale. Commands stay in percent.
 */
export function actuatorShown(display: DisplayScale | undefined, percent: number): number {
  return display ? percent * display.scale + display.offset : percent;
}

/** The percent value for a value in the actuator's display unit. */
export function actuatorPercent(display: DisplayScale | undefined, shown: number): number {
  return display ? (shown - display.offset) / display.scale : shown;
}

/**
 * The percent value one display step away from `percent`: one whole display
 * unit (1 rpm) or `step` percent points, always moving at least one point.
 */
export function stepPercent(
  display: DisplayScale | undefined,
  percent: number,
  steps: number,
  step: number,
): number {
  if (!display) return Math.round(percent + steps * step);
  const target = Math.round(actuatorShown(display, percent)) + steps;
  const v = Math.round(actuatorPercent(display, target));
  return v === Math.round(percent) ? v + Math.sign(steps) : v;
}

/** A number with fixed decimals, or a dash when unknown. */
export function formatNumber(value: number | null | undefined, decimals = 1): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "–";
  // Avoid "-0.0".
  const rounded = Number(value.toFixed(decimals));
  return (Object.is(rounded, -0) ? 0 : rounded).toFixed(decimals);
}

export function weightLossPct(green?: number | null, roasted?: number | null): number | null {
  if (!green || !roasted || green <= 0) return null;
  return ((green - roasted) / green) * 100;
}
