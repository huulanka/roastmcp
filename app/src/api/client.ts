import type {
  Actuator,
  CommandOutcome,
  EventKind,
  Live,
  Lot,
  ReferenceCurve,
  Roast,
  RoastEvent,
  RoastPlan,
  RoastSummary,
  Snapshot,
} from "./types.ts";

const TOKEN_KEY = "roastmcp.token";

/** Pulls `token=…` out of a URL fragment such as `#token=abc`. */
export function parseTokenFromHash(hash: string): string | null {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const token = params.get("token");
  return token && token.length > 0 ? token : null;
}

/** Reads the token from the URL fragment (storing it and stripping it from the URL), or from sessionStorage. */
export function initToken(): string | null {
  try {
    const fromHash = parseTokenFromHash(window.location.hash);
    if (fromHash) {
      sessionStorage.setItem(TOKEN_KEY, fromHash);
      history.replaceState(null, "", window.location.pathname + window.location.search);
      return fromHash;
    }
    return sessionStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

let token: string | null = null;
export function setToken(value: string | null): void {
  token = value;
}
export function getToken(): string | null {
  return token;
}

export function baseUrl(): string {
  const env = import.meta.env.VITE_DAEMON_URL as string | undefined;
  return env && env.length > 0 ? env.replace(/\/$/, "") : window.location.origin;
}

export function wsUrl(): string {
  const url = new URL(baseUrl());
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/api/ws";
  url.search = `?token=${encodeURIComponent(token ?? "")}`;
  return url.toString();
}

export class ApiError extends Error {
  readonly status: number;
  /** 0 = the daemon could not be reached. */
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res: Response;
  try {
    res = await fetch(baseUrl() + path, {
      method,
      headers: {
        Authorization: `Bearer ${token ?? ""}`,
        ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
  } catch {
    throw new ApiError(0, "Daemon not reachable");
  }
  if (!res.ok) {
    let message = res.statusText || `HTTP ${res.status}`;
    try {
      const data = (await res.json()) as { error?: string };
      if (data.error) message = data.error;
    } catch {
      /* not JSON */
    }
    throw new ApiError(res.status, message);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

export const api = {
  status: () => request<Snapshot>("GET", "/api/status"),
  live: () => request<Live>("GET", "/api/live"),
  ports: () => request<string[]>("GET", "/api/ports"),
  connect: (simulator: boolean, port?: string) =>
    request<Snapshot>("POST", "/api/connect", { simulator, port }),
  disconnect: () => request<Snapshot>("POST", "/api/disconnect"),
  command: async (actuator: Actuator, value: number): Promise<CommandOutcome> =>
    (
      await request<{ outcome: CommandOutcome }>("POST", "/api/command", {
        actuator,
        value,
        source: "human",
      })
    ).outcome,
  emergencyStop: () => request<Snapshot>("POST", "/api/emergency-stop", { reason: "UI" }),
  resetEmergencyStop: () => request<Snapshot>("POST", "/api/emergency-stop/reset"),
  presence: () => request<Snapshot>("POST", "/api/presence"),
  resumeAi: () => request<Snapshot>("POST", "/api/ai/resume"),
  startRecording: (body: {
    title?: string;
    green_weight_g?: number;
    beans?: string;
    lot_id?: string;
    plan?: RoastPlan;
  }) => request<Snapshot>("POST", "/api/recording/start", body),
  lots: (archived = false) => request<Lot[]>("GET", `/api/lots${archived ? "?archived=true" : ""}`),
  stopRecording: () =>
    request<{ id: string; json_path: string; alog_path: string }>("POST", "/api/recording/stop"),
  markEvent: (kind: EventKind) =>
    request<RoastEvent>("POST", "/api/events", { kind, origin: "human" }),
  deleteEvent: (kind: EventKind) => request<Snapshot>("DELETE", `/api/events/${kind}`),
  preheat: (target_c: number) => request<Snapshot>("POST", "/api/preheat", { target_c }),
  stopPreheat: () => request<Snapshot>("DELETE", "/api/preheat"),
  reference: () => request<ReferenceCurve | null>("GET", "/api/reference"),
  setReference: (id: string | null) => request<Snapshot>("PUT", "/api/reference", { id }),
  roasts: () => request<RoastSummary[]>("GET", "/api/roasts"),
  roast: (id: string) => request<Roast>("GET", `/api/roasts/${encodeURIComponent(id)}`),
  patchRoast: (id: string, body: { title?: string; roasted_weight_g?: number }) =>
    request<Roast>("PATCH", `/api/roasts/${encodeURIComponent(id)}`, body),
  /** Authenticated download of text content, saved through a temporary link. */
  async download(path: string, filename: string, type = "application/json"): Promise<void> {
    let res: Response;
    try {
      res = await fetch(baseUrl() + path, { headers: { Authorization: `Bearer ${token ?? ""}` } });
    } catch {
      throw new ApiError(0, "Daemon not reachable");
    }
    if (!res.ok) throw new ApiError(res.status, res.statusText);
    const blob = new Blob([await res.text()], { type });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = filename;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
  },
};
