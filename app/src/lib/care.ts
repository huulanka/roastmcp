import type { TFunction } from "i18next";

import type { ChecklistItem, MaintenanceTask } from "@/api/types.ts";

type T = TFunction;

/** Tasks offered when none exist yet; the person takes them over and changes them. */
export const SUGGESTED_TASKS: {
  name: string;
  interval_roasts?: number;
  interval_days?: number;
}[] = [
  { name: "Empty the chaff collector", interval_roasts: 1 },
  { name: "Clean the drum and the exhaust path", interval_roasts: 25, interval_days: 90 },
  { name: "Check the bean probe", interval_days: 180 },
];

/** How often a task comes due: "after every roast", "every 25 roasts or 90 days". */
export function intervalText(
  task: Pick<MaintenanceTask, "interval_roasts" | "interval_days">,
  t: T,
): string {
  const { interval_roasts: roasts, interval_days: days } = task;
  if (roasts && days) return t("every {{roasts}} roasts or {{days}} days", { roasts, days });
  if (roasts === 1) return t("after every roast");
  if (roasts) return t("every {{roasts}} roasts", { roasts });
  if (days) return t("every {{days}} days", { days });
  return t("no interval, only logged");
}

/** What happened since the task was last done: "3 roasts since 9 Nov". */
export function sinceText(task: MaintenanceTask, t: T, date: (iso: string) => string): string {
  const from = date(task.last_done_at ?? task.created_at);
  const counted = task.interval_roasts !== undefined || task.interval_days === undefined;
  if (counted) {
    return task.roasts_since === 1
      ? t("1 roast since {{date}}", { date: from })
      : t("{{n}} roasts since {{date}}", { n: task.roasts_since, date: from });
  }
  return task.days_since === 1
    ? t("1 day since {{date}}", { date: from })
    : t("{{n}} days since {{date}}", { n: task.days_since, date: from });
}

/** How far a task that is not due is from coming due. */
export interface Remaining {
  /** "in 22 roasts", "in 141 days". */
  text: string;
  /** Share of the interval used up, 0 to 1. */
  share: number;
  /** "18 / 40". */
  count: string;
}

/**
 * The nearer of the two intervals, as text and as a share. `null` for a task
 * that is due or has no interval.
 */
export function remaining(task: MaintenanceTask, t: T): Remaining | null {
  if (task.due) return null;
  const options: (Remaining & { left: number })[] = [];
  if (task.interval_roasts) {
    const left = Math.max(0, task.interval_roasts - task.roasts_since);
    options.push({
      left: left / task.interval_roasts,
      text: left === 1 ? t("in 1 roast") : t("in {{n}} roasts", { n: left }),
      share: task.roasts_since / task.interval_roasts,
      count: `${task.roasts_since} / ${task.interval_roasts}`,
    });
  }
  if (task.interval_days) {
    const left = Math.max(0, task.interval_days - task.days_since);
    options.push({
      left: left / task.interval_days,
      text: left === 1 ? t("in 1 day") : t("in {{n}} days", { n: left }),
      share: task.days_since / task.interval_days,
      count: `${task.days_since} / ${task.interval_days}`,
    });
  }
  const nearest = options.sort((a, b) => a.left - b.left)[0];
  if (!nearest) return null;
  return {
    text: nearest.text,
    share: Math.min(1, Math.max(0, nearest.share)),
    count: nearest.count,
  };
}

/** A checklist item in the person's language: built-in items are translated. */
export function checklistText(item: ChecklistItem, t: T): string {
  return item.key ? t(item.text) : item.text;
}

/** A whole number of at least 1 from a text field, or `null`. */
export function parseInterval(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  return n >= 1 && n <= 10_000 ? n : null;
}
