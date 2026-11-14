import type {
  Actuator,
  Alignment,
  Comparison,
  ConsistencyResult,
  Cupping,
  CuppingInput,
  RestEntry,
  BackupStatus,
  Checklist,
  ChecklistItem,
  CommandOutcome,
  EventKind,
  Live,
  Lot,
  LotDetails,
  MaintenanceLogEntry,
  MaintenanceTask,
  ProfileDetail,
  ProfileInfo,
  ProfileSummary,
  ProfileTarget,
  ProfileVersion,
  ReferenceCurve,
  Roast,
  RoastDegree,
  RoastEvent,
  RoastPlan,
  RoastSummary,
  Snapshot,
  StockMovement,
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
    batch_no?: string;
  }) => request<Snapshot>("POST", "/api/recording/start", body),
  lots: (archived = false) => request<Lot[]>("GET", `/api/lots${archived ? "?archived=true" : ""}`),
  lot: (id: string) => request<Lot>("GET", `/api/lots/${encodeURIComponent(id)}`),
  lotMovements: (id: string) =>
    request<StockMovement[]>("GET", `/api/lots/${encodeURIComponent(id)}/movements`),
  addLot: (
    body: LotDetails & {
      name: string;
      purchase_g?: number;
      purchase_date?: string;
      batch_code?: string;
    },
  ) => request<Lot>("POST", "/api/lots", body),
  patchLot: (
    id: string,
    body: {
      name?: string;
      details?: LotDetails;
      archived?: boolean;
      batch_code?: string;
      /** The note for the next roast; an empty one removes it. */
      next_roast?: string;
    },
  ) => request<Lot>("PATCH", `/api/lots/${encodeURIComponent(id)}`, body),
  addPurchase: (id: string, body: { grams: number; date?: string; note?: string }) =>
    request<Lot>("POST", `/api/lots/${encodeURIComponent(id)}/purchases`, body),
  adjustLot: (id: string, body: { grams: number; note?: string }) =>
    request<Lot>("POST", `/api/lots/${encodeURIComponent(id)}/adjustments`, body),
  stopRecording: () =>
    request<{ id: string; json_path: string; alog_path: string }>("POST", "/api/recording/stop"),
  markEvent: (kind: EventKind) =>
    request<RoastEvent>("POST", "/api/events", { kind, origin: "human" }),
  deleteEvent: (kind: EventKind) => request<Snapshot>("DELETE", `/api/events/${kind}`),
  /** Preheat to `target_c` at `air` % (ADR 0018); the daemon sets the air first as your command. */
  preheat: (target_c: number, air: number | null) =>
    request<Snapshot>("POST", "/api/preheat", air === null ? { target_c } : { target_c, air }),
  stopPreheat: () => request<Snapshot>("DELETE", "/api/preheat"),
  /** The autopilot switch (ADR 0020); switching it on also confirms presence. */
  startAutopilot: () => request<Snapshot>("POST", "/api/autopilot"),
  stopAutopilot: () => request<Snapshot>("DELETE", "/api/autopilot"),
  reference: () => request<ReferenceCurve | null>("GET", "/api/reference"),
  setReference: (id: string | null) => request<Snapshot>("PUT", "/api/reference", { id }),
  roasts: () => request<RoastSummary[]>("GET", "/api/roasts"),
  roast: (id: string) => request<Roast>("GET", `/api/roasts/${encodeURIComponent(id)}`),
  patchRoast: (
    id: string,
    body: { title?: string; roasted_weight_g?: number; roast_degree?: RoastDegree },
  ) => request<Roast>("PATCH", `/api/roasts/${encodeURIComponent(id)}`, body),
  backup: () => request<BackupStatus>("GET", "/api/backup"),
  setBackup: (body: { dir: string | null; keep?: number }) =>
    request<BackupStatus>("PUT", "/api/backup", body),
  runBackup: () => request<BackupStatus>("POST", "/api/backup/run"),
  /** Uploads an Artisan `.alog` into the archive. */
  async importAlog(file: File): Promise<{ id: string; title: string; already_there: boolean }> {
    let res: Response;
    try {
      res = await fetch(`${baseUrl()}/api/roasts/import?name=${encodeURIComponent(file.name)}`, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${token ?? ""}`,
          "Content-Type": "application/octet-stream",
        },
        body: await file.arrayBuffer(),
      });
    } catch {
      throw new ApiError(0, "Daemon not reachable");
    }
    const data = (await res.json().catch(() => ({}))) as {
      error?: string;
      id: string;
      title: string;
      already_there: boolean;
    };
    if (!res.ok) throw new ApiError(res.status, data.error ?? res.statusText);
    return data;
  },
  profiles: (archived = false) =>
    request<ProfileSummary[]>("GET", `/api/profiles${archived ? "?archived=true" : ""}`),
  profile: (id: string) => request<ProfileDetail>("GET", `/api/profiles/${encodeURIComponent(id)}`),
  addProfile: (body: {
    name: string;
    intended_beans?: string;
    batch_size_g?: number;
    target: ProfileTarget;
    change_note?: string;
  }) => request<ProfileDetail>("POST", "/api/profiles", body),
  profileVersion: (id: string) =>
    request<{ profile: ProfileInfo; version: ProfileVersion }>(
      "GET",
      `/api/profile-versions/${encodeURIComponent(id)}`,
    ),
  addProfileVersion: (id: string, body: { target: ProfileTarget; change_note?: string }) =>
    request<ProfileDetail>("POST", `/api/profiles/${encodeURIComponent(id)}/versions`, body),
  patchProfile: (
    id: string,
    body: { name?: string; intended_beans?: string; batch_size_g?: number; archived?: boolean },
  ) => request<ProfileDetail>("PATCH", `/api/profiles/${encodeURIComponent(id)}`, body),
  maintenance: (archived = false) =>
    request<MaintenanceTask[]>("GET", `/api/maintenance${archived ? "?archived=true" : ""}`),
  addMaintenanceTask: (body: {
    name: string;
    description?: string;
    interval_roasts?: number;
    interval_days?: number;
  }) => request<MaintenanceTask>("POST", "/api/maintenance/tasks", body),
  /** An interval of 0 removes it; an empty description too. */
  patchMaintenanceTask: (
    id: string,
    body: {
      name?: string;
      description?: string;
      interval_roasts?: number;
      interval_days?: number;
      archived?: boolean;
    },
  ) => request<MaintenanceTask>("PATCH", `/api/maintenance/tasks/${encodeURIComponent(id)}`, body),
  maintenanceDone: (id: string, note?: string) =>
    request<MaintenanceTask>(
      "POST",
      `/api/maintenance/tasks/${encodeURIComponent(id)}/done`,
      note ? { note } : {},
    ),
  maintenanceLog: (limit = 50) =>
    request<MaintenanceLogEntry[]>("GET", `/api/maintenance/log?limit=${limit}`),
  /** Takes a log entry back; its task counts from the entry before it. */
  deleteMaintenanceLog: (id: string) =>
    request<MaintenanceTask>("DELETE", `/api/maintenance/log/${encodeURIComponent(id)}`),
  cuppings: (roastId: string) =>
    request<Cupping[]>("GET", `/api/cuppings?roast=${encodeURIComponent(roastId)}`),
  allCuppings: () => request<Cupping[]>("GET", "/api/cuppings"),
  addCupping: (roastId: string, body: CuppingInput) =>
    request<Cupping>("POST", `/api/roasts/${encodeURIComponent(roastId)}/cuppings`, body),
  /** Scores and descriptors replace the stored ones; empty notes or brew method remove them. */
  patchCupping: (id: string, body: CuppingInput) =>
    request<Cupping>("PATCH", `/api/cuppings/${encodeURIComponent(id)}`, body),
  deleteCupping: (id: string) =>
    request<Record<string, never>>("DELETE", `/api/cuppings/${encodeURIComponent(id)}`),
  /** Roasts that rest, are in their window or were past it within the last 14 days. */
  rest: () => request<RestEntry[]>("GET", "/api/rest"),
  /** How alike the roasts of a lot, a profile or a chosen set are. */
  consistency: (
    group: { lot_id: string } | { profile_id: string } | { ids: string[] },
    simulated = false,
  ) => {
    const q =
      "lot_id" in group
        ? `lot_id=${encodeURIComponent(group.lot_id)}`
        : "profile_id" in group
          ? `profile_id=${encodeURIComponent(group.profile_id)}`
          : `ids=${group.ids.map(encodeURIComponent).join(",")}`;
    return request<ConsistencyResult>(
      "GET",
      `/api/consistency?${q}${simulated ? "&simulated=true" : ""}`,
    );
  },
  compare: (a: string, b: string, align: Alignment, stepS = 5) =>
    request<Comparison>(
      "GET",
      `/api/roasts/compare?a=${encodeURIComponent(a)}&b=${encodeURIComponent(b)}&align=${align}&step_s=${stepS}`,
    ),
  checklist: () => request<Checklist>("GET", "/api/checklist"),
  setChecklist: (items: ChecklistItem[]) => request<Checklist>("PUT", "/api/checklist", { items }),
  /** Moves a roast to the trash folder and credits its green coffee back. */
  deleteRoast: (id: string) =>
    request<{ credited_g: number; trash_path: string }>(
      "DELETE",
      `/api/roasts/${encodeURIComponent(id)}`,
    ),
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
