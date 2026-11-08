import { useSyncExternalStore } from "react";

import { api, ApiError } from "@/api/client.ts";
import type { EventKind } from "@/api/types.ts";
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

let justSaved: string | null = null;
const savedListeners = new Set<() => void>();
function setJustSaved(id: string | null): void {
  justSaved = id;
  savedListeners.forEach((l) => l());
}

/** The roast the daemon just saved at the drop, until its weight dialog is closed. */
export function useJustSaved(): string | null {
  return useSyncExternalStore(
    (l) => {
      savedListeners.add(l);
      return () => savedListeners.delete(l);
    },
    () => justSaved,
  );
}

export function clearJustSaved(): void {
  setJustSaved(null);
}

/** Marks an event. The drop ends and saves the recording, so it also opens the weight dialog. */
export async function markEvent(kind: EventKind): Promise<void> {
  try {
    await api.markEvent(kind);
  } catch (e) {
    toast("critical", e instanceof ApiError ? e.message : String(e));
    return;
  }
  if (kind !== "drop") return;
  try {
    const list = await api.roasts();
    const newest = [...list].sort((a, b) => b.roasted_at.localeCompare(a.roasted_at))[0];
    if (newest) setJustSaved(newest.id);
  } catch {
    /* the roast is saved either way */
  }
}
