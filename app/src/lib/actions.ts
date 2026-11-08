import type { ControlLogEntry } from "@/api/types.ts";

/** How a logged request ended: asked-for, limited to something else, refused, or waiting. */
export type Status = "applied" | "clamped" | "rejected" | "awaiting";

export function entryStatus(e: ControlLogEntry): Status {
  switch (e.outcome.outcome) {
    case "rejected":
      return "rejected";
    case "awaiting_confirmation":
      return "awaiting";
    default:
      return e.outcome.clamped_from !== null && e.outcome.clamped_from !== undefined
        ? "clamped"
        : "applied";
  }
}
