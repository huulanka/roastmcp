import { useSyncExternalStore } from "react";

import { api } from "@/api/client.ts";

/** How many maintenance tasks are due; shown on the Care tab. */
let due = 0;
const listeners = new Set<() => void>();

function set(n: number): void {
  if (n === due) return;
  due = n;
  listeners.forEach((l) => l());
}

/** Reads the due tasks from the daemon again (on start, after a roast, after a change). */
export function refreshCare(): void {
  api
    .maintenance()
    .then((tasks) => set(tasks.filter((t) => t.due).length))
    // Not reachable right now: the tab simply shows no count.
    .catch(() => undefined);
}

/** Takes the count from tasks that were just loaded. */
export function setCareDue(n: number): void {
  set(n);
}

export function useCareDue(): number {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => due,
  );
}
