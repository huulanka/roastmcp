import { api, ApiError } from "@/api/client.ts";
import { applySnapshot } from "./live.ts";
import { toast } from "./toasts.ts";

export async function emergencyStop(): Promise<void> {
  try {
    applySnapshot(await api.emergencyStop());
  } catch (e) {
    toast("critical", `EMERGENCY STOP failed: ${e instanceof ApiError ? e.message : String(e)}`);
  }
}

export async function confirmPresence(): Promise<void> {
  try {
    applySnapshot(await api.presence());
  } catch (e) {
    toast("critical", e instanceof ApiError ? e.message : String(e));
  }
}

/** Runs a daemon call and reports a failure as a toast. */
export async function attempt(fn: () => Promise<unknown>): Promise<void> {
  try {
    await fn();
  } catch (e) {
    toast("critical", e instanceof ApiError ? e.message : String(e));
  }
}
