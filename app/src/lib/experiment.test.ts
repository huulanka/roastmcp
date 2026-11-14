import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import type { Experiment } from "@/api/types.ts";
import {
  changeDescription,
  changeLine,
  roastsText,
  sortExperiments,
  variableOf,
} from "./experiment.ts";

const t = ((key: string, options?: Record<string, unknown>): string =>
  key.replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
    String(options?.[name]),
  )) as unknown as TFunction;

const exp = (o: Partial<Experiment>): Experiment => ({
  id: "e",
  hypothesis: "Charging hotter shortens drying",
  changed_variable: { description: "charge hotter" },
  status: "planned",
  roast_ids: [],
  created_at: "2026-11-09T10:00:00Z",
  ...o,
});

describe("experiments", () => {
  it("names the change by its variable, else its description", () => {
    expect(
      changeLine(
        exp({
          changed_variable: {
            description: "x",
            structured: { name: "Charge", from: "190 °C", to: "200 °C" },
          },
        }),
      ),
    ).toBe("Charge 190 °C → 200 °C");
    expect(changeLine(exp({}))).toBe("charge hotter");
  });

  it("builds the change from the variable when no description is given", () => {
    const v = variableOf(" Charge ", "190 °C", "200 °C");
    expect(v).toEqual({ name: "Charge", from: "190 °C", to: "200 °C" });
    expect(variableOf("Charge", "", "200")).toBeNull();
    expect(changeDescription("", v)).toBe("Charge: 190 °C → 200 °C");
    expect(changeDescription(" hotter ", v)).toBe("hotter");
    expect(changeDescription("", null)).toBe("");
  });

  it("counts roasts and puts running ones first", () => {
    expect(roastsText(exp({}), t)).toBe("no roast yet");
    expect(roastsText(exp({ roast_ids: ["a", "b"] }), t)).toBe("2 roasts");
    const sorted = sortExperiments([
      exp({ id: "done", status: "done" }),
      exp({ id: "old", status: "running", created_at: "2026-11-01T00:00:00Z" }),
      exp({ id: "planned" }),
      exp({ id: "new", status: "running", created_at: "2026-11-10T00:00:00Z" }),
    ]);
    expect(sorted.map((e) => e.id)).toEqual(["new", "old", "planned", "done"]);
  });
});
