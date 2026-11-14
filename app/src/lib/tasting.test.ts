import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import type { Cupping, RestEntry } from "@/api/types.ts";
import {
  addDescriptors,
  bestByRoast,
  brewText,
  dateField,
  meanScore,
  parseBrew,
  rated,
  restChip,
  restDays,
  restLine,
  scoreText,
  tastedAt,
} from "./tasting.ts";
import { formatNumber } from "./units.ts";

/** Fills the placeholders like i18next, without translating. */
const t = ((key: string, options?: Record<string, unknown>): string =>
  key.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
    String(options?.[name]),
  )) as unknown as TFunction;

const cupping = (o: Partial<Cupping>): Cupping => ({
  id: "c",
  roast_id: "r",
  cupped_at: "2026-11-13T10:00:00Z",
  rest_days: 4,
  scores: {},
  descriptors: [],
  ...o,
});

describe("tasting scores", () => {
  it("averages only what was rated", () => {
    const scores = { fragrance_aroma: 8, flavor: 8.5, acidity: 7 };
    expect(rated(scores).map((s) => s.key)).toEqual(["fragrance_aroma", "flavor", "acidity"]);
    expect(meanScore(scores)).toBeCloseTo(7.833, 3);
    expect(meanScore({})).toBeNull();
  });

  it("shows the own total, else the mean", () => {
    expect(scoreText(cupping({ overall_score: 86.5 }), t, formatNumber)).toBe("86.5 points");
    expect(scoreText(cupping({ overall_score: 84 }), t, formatNumber)).toBe("84 points");
    expect(scoreText(cupping({ scores: { flavor: 8, body: 7 } }), t, formatNumber)).toBe("Ø 7.5");
    expect(scoreText(cupping({}), t, formatNumber)).toBeNull();
  });

  it("keeps the best tasting of each roast", () => {
    const best = bestByRoast([
      cupping({ id: "a", roast_id: "r1", overall_score: 84 }),
      cupping({ id: "b", roast_id: "r1", overall_score: 86.5 }),
      cupping({ id: "c", roast_id: "r2", scores: { flavor: 8 } }),
      cupping({ id: "d", roast_id: "r3" }),
    ]);
    expect(best.get("r1")?.id).toBe("b");
    expect(best.get("r2")?.id).toBe("c");
    expect(best.has("r3")).toBe(false);
  });
});

describe("rest", () => {
  const entry = (o: Partial<RestEntry>): RestEntry => ({
    roast_id: "r",
    title: "Yellow Bourbon",
    roasted_at: "2026-11-09T18:04:00Z",
    level: "medium",
    level_from: "default",
    rest_days: 4,
    ready_from: "2026-11-13T18:04:00Z",
    best_until: "2026-11-23T18:04:00Z",
    state: "ready",
    cuppings: 0,
    ...o,
  });
  const date = (iso: string) => iso.slice(5, 10);

  it("says where a roast is in its window", () => {
    expect(restChip(entry({ state: "resting" }), t, date)).toBe("resting · from 11-13");
    expect(restChip(entry({}), t, date)).toBe("ready · day 4");
    expect(restChip(entry({ state: "past_best" }), t, date)).toBe("past its best");
    expect(restLine(entry({}), t, date)).toBe("Ready to drink · at its best until 11-23");
  });

  it("counts whole days from the roast to the tasting", () => {
    const roasted = "2026-11-09T18:04:00Z";
    expect(restDays(roasted, "2026-11-14T11:18:00Z")).toBe(4);
    expect(restDays(roasted, "2026-11-13T18:04:00Z")).toBe(4);
    expect(restDays(roasted, "2026-11-09T19:00:00Z")).toBe(0);
    expect(restDays(roasted, "2026-11-01T12:00:00Z")).toBe(0);
  });

  it("never dates a tasting before its roast", () => {
    const roasted = new Date(2026, 10, 9, 19, 4);
    const now = new Date(2026, 10, 14, 9, 30);
    expect(tastedAt(dateField(now), roasted.toISOString(), now)).toBe(now.toISOString());
    expect(tastedAt("2026-11-09", roasted.toISOString(), now)).toBe(roasted.toISOString());
    expect(tastedAt("2026-11-12", roasted.toISOString(), now)).toBe(
      new Date(2026, 10, 12, 12).toISOString(),
    );
  });
});

describe("brew method and descriptors", () => {
  it("joins the way and the details and reads them back", () => {
    expect(brewText("filter", " V60 15 g / 250 g ", t)).toBe("Filter · V60 15 g / 250 g");
    expect(brewText(null, "Aeropress", t)).toBe("Aeropress");
    expect(parseBrew("Filter · V60 15 g / 250 g", t)).toEqual({
      brew: "filter",
      detail: "V60 15 g / 250 g",
    });
    expect(parseBrew("Aeropress", t)).toEqual({ brew: null, detail: "Aeropress" });
    expect(parseBrew(undefined, t)).toEqual({ brew: null, detail: "" });
  });

  it("adds descriptors from typed text without repeats", () => {
    expect(addDescriptors(["Haselnuss"], "Orange, haselnuss ,  ,Kakao")).toEqual([
      "Haselnuss",
      "Orange",
      "Kakao",
    ]);
  });
});
