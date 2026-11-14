import { useEffect, useRef, useSyncExternalStore } from "react";

import { api, ApiError } from "@/api/client.ts";
import type { EventKind, Snapshot } from "@/api/types.ts";
import { endedRoast } from "@/lib/roast.ts";
import { refreshCare } from "./care.ts";
import { applySnapshot, useLive } from "./live.ts";
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

/** Marks an event. The drop ends and saves the recording (see `SaveWatcher`). */
export async function markEvent(kind: EventKind): Promise<void> {
  try {
    await api.markEvent(kind);
  } catch (e) {
    toast("critical", e instanceof ApiError ? e.message : String(e));
  }
}

/**
 * Opens the weight dialog whenever a roast that had its CHARGE ends: a DROP
 * marked or detected by the daemon (which saves at once), or "End recording".
 */
export function SaveWatcher(): null {
  const { snapshot } = useLive();
  const prev = useRef<Snapshot | null>(null);
  useEffect(() => {
    if (!snapshot) return;
    const ended = endedRoast(prev.current, snapshot);
    if (ended) {
      setJustSaved(ended);
      // One more roast: maintenance may have come due.
      refreshCare();
    }
    prev.current = snapshot;
  }, [snapshot]);
  return null;
}
