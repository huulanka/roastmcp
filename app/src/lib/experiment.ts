import type { TFunction } from "i18next";

import type { Experiment } from "@/api/types.ts";

type T = TFunction;

/** The change of an experiment as one line: "Charge temperature 190 → 200" or its description. */
export function changeLine(e: Experiment): string {
  const v = e.changed_variable.structured;
  return v ? `${v.name} ${v.from} → ${v.to}` : e.changed_variable.description;
}

/** The text the daemon keeps as the change: the description, else built from the variable. */
export function changeDescription(
  description: string,
  variable: { name: string; from: string; to: string } | null,
): string {
  const d = description.trim();
  if (d) return d;
  return variable ? `${variable.name}: ${variable.from} → ${variable.to}` : "";
}

/** A variable only when all three parts are given. */
export function variableOf(
  name: string,
  from: string,
  to: string,
): { name: string; from: string; to: string } | null {
  const v = { name: name.trim(), from: from.trim(), to: to.trim() };
  return v.name && v.from && v.to ? v : null;
}

/** The status as a word: planned, running, done. */
export function statusText(e: Experiment, t: T): string {
  return e.status === "planned" ? t("planned") : e.status === "running" ? t("running") : t("done");
}

/** How many roasts test it, as a short phrase. */
export function roastsText(e: Experiment, t: T): string {
  const n = e.roast_ids.length;
  return n === 0 ? t("no roast yet") : n === 1 ? t("1 roast") : t("{{n}} roasts", { n });
}

/** Running ones first, then planned, then done; newest first within each. */
export function sortExperiments(list: readonly Experiment[]): Experiment[] {
  const order = { running: 0, planned: 1, done: 2 } as const;
  return [...list].sort(
    (a, b) => order[a.status] - order[b.status] || b.created_at.localeCompare(a.created_at),
  );
}
