import { useSyncExternalStore } from "react";
import i18n from "i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Actuator, ActuatorSpec } from "@/api/types.ts";
import { stepPercent } from "@/lib/units.ts";
import { toast } from "./toasts.ts";

/** Values the user asked for that the daemon has not confirmed yet. */
let desired: Partial<Record<Actuator, number>> = {};
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export function useDesired(): Partial<Record<Actuator, number>> {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => desired,
  );
}

export function getDesired(a: Actuator): number | undefined {
  return desired[a];
}

function clear(a: Actuator, v: number): void {
  if (desired[a] === v) {
    const { [a]: _drop, ...rest } = desired;
    desired = rest;
    emit();
  }
}

export function rejectText(reason: string): string {
  const map: Record<string, string> = {
    emergency_stop_latched: "Emergency stop is active. Reset it first.",
    ai_suspended_by_human: "AI control is suspended.",
    no_lease: "No control lease.",
    presence_required: "Presence confirmation required: press “I'm at the roaster”.",
    out_of_range: "Value out of range.",
    rate_limited: "Too many changes too fast. Wait a moment.",
    not_supported: "Not supported by this machine.",
    disconnected: "Machine is not connected.",
    overheat: "Overheat protection is active.",
  };
  return i18n.t(map[reason] ?? `Rejected (${reason})`);
}

export async function setActuator(a: Actuator, value: number): Promise<void> {
  desired = { ...desired, [a]: value };
  emit();
  try {
    const res = await api.command(a, value);
    if (res.outcome === "rejected") {
      toast("warning", `${a}: ${rejectText(res.reason)}`);
    } else if (res.outcome === "applied" && typeof res.clamped_from === "number") {
      // The daemon leaves `clamped_from` out when nothing was limited.
      toast("info", i18n.t("{{a}} limited to {{v}}", { a, v: res.value }));
    }
  } catch (e) {
    toast("critical", `${a}: ${e instanceof ApiError ? e.message : String(e)}`);
  } finally {
    clear(a, value);
  }
}

/**
 * Moves an actuator by `steps`: display units where the machine has them (drum
 * rpm), otherwise `spec.step` percent points each.
 */
export function nudge(spec: ActuatorSpec, current: number, steps: number): void {
  const base = desired[spec.actuator] ?? current;
  const raw = stepPercent(spec.display, base, steps, spec.step || 1);
  const v = Math.min(spec.max, Math.max(spec.min, raw));
  if (v !== base) void setActuator(spec.actuator, v);
}
