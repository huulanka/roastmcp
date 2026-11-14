import type { TFunction } from "i18next";

import type { Cupping, CuppingScores, RestEntry } from "@/api/types.ts";

type T = TFunction;

export type ScoreKey = keyof CuppingScores;

/** The attributes of a tasting in the order of the form, with their English names. */
export const SCORES: { key: ScoreKey; name: string }[] = [
  { key: "fragrance_aroma", name: "Aroma" },
  { key: "flavor", name: "Flavour" },
  { key: "aftertaste", name: "Aftertaste" },
  { key: "acidity", name: "Acidity" },
  { key: "body", name: "Body" },
  { key: "sweetness", name: "Sweetness" },
  { key: "balance", name: "Balance" },
  { key: "overall", name: "Overall impression" },
];

/** The scores that were given, in form order. */
export function rated(scores: CuppingScores): { key: ScoreKey; name: string; value: number }[] {
  return SCORES.flatMap((s) => {
    const value = scores[s.key];
    return typeof value === "number" ? [{ ...s, value }] : [];
  });
}

/** Mean of the scores given, or `null` without any. */
export function meanScore(scores: CuppingScores): number | null {
  const given = rated(scores);
  if (given.length === 0) return null;
  return given.reduce((sum, s) => sum + s.value, 0) / given.length;
}

/** How good a tasting was, to rank them: the own total first, then the mean (on 0–100). */
export function rank(c: Cupping): number | null {
  if (typeof c.overall_score === "number") return c.overall_score;
  const mean = meanScore(c.scores);
  return mean === null ? null : mean * 10;
}

/** The best tasting of each roast, by `rank`. */
export function bestByRoast(cuppings: Cupping[]): Map<string, Cupping> {
  const best = new Map<string, Cupping>();
  for (const c of cuppings) {
    const r = rank(c);
    if (r === null) continue;
    const now = best.get(c.roast_id);
    const nowRank = now ? rank(now) : null;
    if (nowRank === null || r > nowRank) best.set(c.roast_id, c);
  }
  return best;
}

/** A tasting's result as short text: "86.5 points" or "Ø 7.9". */
export function scoreText(
  c: Cupping,
  t: T,
  format: (n: number, d?: number) => string,
): string | null {
  if (typeof c.overall_score === "number") {
    return t("{{score}} points", { score: format(c.overall_score, c.overall_score % 1 ? 1 : 0) });
  }
  const mean = meanScore(c.scores);
  return mean === null ? null : `Ø ${format(mean)}`;
}

/** The chip on a roast in the list: "resting · from 16 Nov", "ready · day 4", "past its best". */
export function restChip(entry: RestEntry, t: T, date: (iso: string) => string): string {
  switch (entry.state) {
    case "resting":
      return t("resting · from {{date}}", { date: date(entry.ready_from) });
    case "ready":
      return t("ready · day {{n}}", { n: entry.rest_days });
    case "past_best":
      return t("past its best");
  }
}

/** The line in a roast's header: when it is ready and how long it is at its best. */
export function restLine(entry: RestEntry, t: T, date: (iso: string) => string): string {
  switch (entry.state) {
    case "resting":
      return t("Resting · ready from {{date}}", { date: date(entry.ready_from) });
    case "ready":
      return t("Ready to drink · at its best until {{date}}", { date: date(entry.best_until) });
    case "past_best":
      return t("Past its best since {{date}}", { date: date(entry.best_until) });
  }
}

/** The ways of brewing offered for a tasting, with their English names. */
export const BREWS = [
  { key: "cupping", name: "Cupping" },
  { key: "filter", name: "Filter" },
  { key: "espresso", name: "Espresso" },
] as const;
export type Brew = (typeof BREWS)[number]["key"];

/** The stored brew method from the way and the details: "Filter · V60 15 g / 250 g". */
export function brewText(brew: Brew | null, detail: string, t: T): string {
  const name = brew ? t(BREWS.find((b) => b.key === brew)?.name ?? "") : "";
  return [name, detail.trim()].filter(Boolean).join(" · ");
}

/** The way and the details back from a stored brew method, in the person's language. */
export function parseBrew(text: string | undefined, t: T): { brew: Brew | null; detail: string } {
  if (!text) return { brew: null, detail: "" };
  const [head, ...rest] = text.split(" · ");
  const found = BREWS.find((b) => t(b.name) === head || b.name === head);
  return found ? { brew: found.key, detail: rest.join(" · ") } : { brew: null, detail: text };
}

/** Descriptors from typed text: split at commas, trimmed, without repeats (ignoring case). */
export function addDescriptors(have: string[], typed: string): string[] {
  const out = [...have];
  for (const part of typed.split(",")) {
    const d = part.trim();
    if (d && !out.some((x) => x.toLowerCase() === d.toLowerCase())) out.push(d);
  }
  return out;
}

/** Whole days from the roast to the moment of tasting, as the daemon counts them (never below 0). */
export function restDays(roastedAt: string, tastedAtIso: string): number {
  const ms = new Date(tastedAtIso).getTime() - new Date(roastedAt).getTime();
  return Math.max(0, Math.floor(ms / 86_400_000));
}

/** A date as the `yyyy-mm-dd` of a date field, in local time. */
export function dateField(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/**
 * The moment a tasting on that day is saved at: now for today, the roast's
 * own time on its day (a tasting may not be before the roast), else noon.
 */
export function tastedAt(day: string, roastedAt: string, now = new Date()): string {
  if (day === dateField(now)) return now.toISOString();
  const roasted = new Date(roastedAt);
  if (day === dateField(roasted)) return roasted.toISOString();
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d, 12).toISOString();
}
