import type { RoastState, Snapshot } from "@/api/types.ts";
import { chargeTime } from "@/lib/chartData.ts";

export type Screen = "roast" | "archive";

export function stateLabel(t: (k: string) => string, s: RoastState): string {
  const m: Record<RoastState, string> = {
    disconnected: t("Disconnected"),
    connected: t("Ready"),
    recording: t("Recording"),
    charged: t("Roasting"),
    dropped: t("Dropped"),
    cooling: t("Cooling"),
    finished: t("Finished"),
  };
  return m[s];
}

export function roastClock(s: Snapshot | null): number | null {
  if (!s || s.t === null) return null;
  const c = s.charge_t ?? chargeTime(s.events);
  return c !== null ? s.t - c : s.t;
}
