import { describe, expect, it } from "vitest";

import type { ControlLogEntry } from "@/api/types.ts";
import { entryStatus } from "@/lib/actions.ts";

const base = {
  at: "2026-01-01T00:00:00Z",
  request: { actuator: "burner", value: 80, source: "ai" },
} as const;

describe("entryStatus", () => {
  it("tells applied, limited, rejected and waiting apart", () => {
    const e = (outcome: ControlLogEntry["outcome"]): ControlLogEntry => ({ ...base, outcome });
    expect(entryStatus(e({ outcome: "applied", value: 80, clamped_from: null }))).toBe("applied");
    expect(entryStatus(e({ outcome: "applied", value: 60, clamped_from: 80 }))).toBe("clamped");
    expect(entryStatus(e({ outcome: "rejected", reason: "no_lease" }))).toBe("rejected");
    expect(entryStatus(e({ outcome: "awaiting_confirmation", confirmation_id: "x" }))).toBe(
      "awaiting",
    );
  });
});
