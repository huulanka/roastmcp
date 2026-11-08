import { useSyncExternalStore } from "react";

import { api, ApiError, getToken, wsUrl } from "@/api/client.ts";
import type { ControlLogEntry, Note, Point, Snapshot, WsMessage } from "@/api/types.ts";

export interface LiveState {
  snapshot: Snapshot | null;
  points: Point[];
  /** Notes of the running recording (AI coaching among them); refreshed every few seconds. */
  notes: Note[];
  /** Control actions of the running recording, newest last. */
  controlLog: ControlLogEntry[];
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
  notes: [],
  controlLog: [],
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
    set({
      snapshot,
      points,
      notes: live.notes ?? [],
      controlLog: live.control_log ?? [],
      reachable: true,
      authError: false,
    });
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) set({ authError: true });
    else if (e instanceof ApiError && e.status === 0) set({ reachable: false });
  } finally {
    syncing = false;
    pending = [];
  }
}

const EXTRAS_POLL_MS = 5000;

function sameTail<T>(a: readonly T[], b: readonly T[], key: (x: T) => unknown): boolean {
  return a.length === b.length && key(a[a.length - 1]) === key(b[b.length - 1]);
}

/** Notes are not pushed over the WebSocket; poll them while a recording runs. */
async function refreshExtras(): Promise<void> {
  const st = state.snapshot?.state;
  if (syncing || !state.online || !st || st === "disconnected" || st === "connected") return;
  try {
    const live = await api.live();
    const notes = live.notes ?? [];
    if (!sameTail(notes, state.notes, (n) => n?.t)) set({ notes });
  } catch {
    /* the WebSocket reports reachability */
  }
}

let loadingLog = false;

/** An action was logged: fetch the log right away instead of waiting for the next poll. */
async function refreshControlLog(seq: number): Promise<void> {
  if (loadingLog || syncing) return;
  loadingLog = true;
  try {
    const live = await api.live();
    const controlLog = live.control_log ?? [];
    if (controlLog.length !== state.controlLog.length || seq === 0) set({ controlLog });
  } catch {
    /* the WebSocket reports reachability */
  } finally {
    loadingLog = false;
  }
}

export function startLive(): () => void {
  let ws: WebSocket | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stopped = false;
  const extrasTimer = setInterval(() => void refreshExtras(), EXTRAS_POLL_MS);

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
        const seq = msg.snapshot.control_seq ?? 0;
        if (seq !== state.controlLog.length) void refreshControlLog(seq);
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
    clearInterval(extrasTimer);
    ws?.close();
  };
}

export function hasToken(): boolean {
  return getToken() !== null;
}
