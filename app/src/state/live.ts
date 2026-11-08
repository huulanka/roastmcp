import { useSyncExternalStore } from "react";

import { api, ApiError, getToken, wsUrl } from "@/api/client.ts";
import type { Point, Snapshot, WsMessage } from "@/api/types.ts";

export interface LiveState {
  snapshot: Snapshot | null;
  points: Point[];
  /** WebSocket is open. */
  online: boolean;
  /** At least one successful exchange happened since the last failure. */
  reachable: boolean;
  authError: boolean;
}

const MAX_POINTS = 8000;
let state: LiveState = {
  snapshot: null,
  points: [],
  online: false,
  reachable: true,
  authError: false,
};
const listeners = new Set<() => void>();

function set(patch: Partial<LiveState>): void {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

export function useLive(): LiveState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

/** Replace the snapshot from a REST response (connect, disconnect, ...). */
export function applySnapshot(snapshot: Snapshot): void {
  set({ snapshot });
}

function appendPoint(points: Point[], p: Point): Point[] {
  const last = points[points.length - 1];
  if (last && p.t <= last.t) return points;
  const next = points.length >= MAX_POINTS ? points.slice(1) : points.slice();
  next.push(p);
  return next;
}

let syncing = false;
let pending: Point[] = [];

async function sync(): Promise<void> {
  syncing = true;
  pending = [];
  try {
    const [snapshot, live] = await Promise.all([api.status(), api.live()]);
    let points = live.points;
    for (const p of pending) points = appendPoint(points, p);
    set({ snapshot, points, reachable: true, authError: false });
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) set({ authError: true });
    else if (e instanceof ApiError && e.status === 0) set({ reachable: false });
  } finally {
    syncing = false;
    pending = [];
  }
}

export function startLive(): () => void {
  let ws: WebSocket | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;

  const open = () => {
    if (stopped) return;
    ws = new WebSocket(wsUrl());
    ws.onopen = () => {
      set({ online: true, reachable: true });
      void sync();
    };
    ws.onmessage = (ev) => {
      let msg: WsMessage;
      try {
        msg = JSON.parse(String(ev.data)) as WsMessage;
      } catch {
        return;
      }
      if (msg.type === "reset") {
        void sync();
      } else if (msg.type === "tick") {
        const patch: Partial<LiveState> = { snapshot: msg.snapshot };
        if (msg.point) {
          if (syncing) pending.push(msg.point);
          else patch.points = appendPoint(state.points, msg.point);
        }
        set(patch);
      }
    };
    ws.onclose = () => {
      set({ online: false, reachable: false });
      if (!stopped) timer = setTimeout(open, 1500);
    };
    ws.onerror = () => ws?.close();
  };
  open();
  return () => {
    stopped = true;
    clearTimeout(timer);
    ws?.close();
  };
}

export function hasToken(): boolean {
  return getToken() !== null;
}
