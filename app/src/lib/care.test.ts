import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import type { MaintenanceTask } from "@/api/types.ts";
import { checklistText, intervalText, parseInterval, remaining, sinceText } from "./care.ts";

/** Fills the placeholders like i18next, without translating. */
const t = ((key: string, options?: Record<string, unknown>): string =>
  key.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
    String(options?.[name]),
  )) as unknown as TFunction;

const task = (o: Partial<MaintenanceTask>): MaintenanceTask => ({
  id: "t",
  name: "Empty the chaff collector",
  archived: false,
  created_at: "2026-11-01T12:00:00Z",
  roasts_since: 0,
  days_since: 0,
  due: null,
  ...o,
});

describe("maintenance intervals", () => {
  it("names how often a task comes due", () => {
    expect(intervalText({ interval_roasts: 1 }, t)).toBe("after every roast");
    expect(intervalText({ interval_roasts: 25 }, t)).toBe("every 25 roasts");
    expect(intervalText({ interval_days: 180 }, t)).toBe("every 180 days");
    expect(intervalText({ interval_roasts: 25, interval_days: 90 }, t)).toBe(
      "every 25 roasts or 90 days",
    );
    expect(intervalText({}, t)).toBe("no interval, only logged");
  });

  it("counts roasts for a roast interval and days for a day interval", () => {
    const date = (iso: string) => iso.slice(0, 10);
    expect(sinceText(task({ interval_roasts: 5, roasts_since: 1 }), t, date)).toBe(
      "1 roast since 2026-11-01",
    );
    expect(
      sinceText(
        task({ interval_roasts: 5, roasts_since: 3, last_done_at: "2026-11-09T19:40:00Z" }),
        t,
        date,
      ),
    ).toBe("3 roasts since 2026-11-09");
    expect(sinceText(task({ interval_days: 180, days_since: 39 }), t, date)).toBe(
      "39 days since 2026-11-01",
    );
  });

  it("shows the nearer interval of a task that is not due", () => {
    expect(remaining(task({ interval_roasts: 40, roasts_since: 18 }), t)).toEqual({
      text: "in 22 roasts",
      share: 0.45,
      count: "18 / 40",
    });
    // 20 of 25 roasts are nearer than 30 of 90 days.
    expect(
      remaining(
        task({ interval_roasts: 25, interval_days: 90, roasts_since: 20, days_since: 30 }),
        t,
      )?.text,
    ).toBe("in 5 roasts");
    expect(remaining(task({ interval_days: 180, days_since: 179 }), t)?.text).toBe("in 1 day");
    expect(remaining(task({}), t)).toBeNull();
    expect(
      remaining(
        task({
          interval_roasts: 1,
          roasts_since: 1,
          due: { by: "roasts", since: 1, interval: 1 },
        }),
        t,
      ),
    ).toBeNull();
  });

  it("reads an interval from a text field", () => {
    expect(parseInterval(" 25 ")).toBe(25);
    expect(parseInterval("")).toBeNull();
    expect(parseInterval("0")).toBeNull();
    expect(parseInterval("2.5")).toBeNull();
    expect(parseInterval("-3")).toBeNull();
  });
});

describe("checklist items", () => {
  it("translates built-in items and leaves the person's own alone", () => {
    const translate = ((key: string) =>
      key === "The chaff collector is empty" ? "Leer" : key) as unknown as TFunction;
    expect(checklistText({ key: "chaff", text: "The chaff collector is empty" }, translate)).toBe(
      "Leer",
    );
    expect(checklistText({ text: "The chaff collector is empty" }, translate)).toBe(
      "The chaff collector is empty",
    );
  });
});
