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
