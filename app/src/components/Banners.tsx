import { useEffect, useRef, useState } from "react";
import { AlertTriangle, CircleAlert, Flame, WifiOff, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog.tsx";
import { Button } from "@/components/ui/button.tsx";
import { cn } from "@/lib/utils.ts";
import { beep } from "@/state/audio.ts";
import { applySnapshot, hasToken, useLive } from "@/state/live.ts";
import { dismissToast, toast, useToasts } from "@/state/toasts.ts";

function Bar({
  tone,
  icon,
  children,
  action,
}: {
  tone: "critical" | "warning";
  icon: React.ReactNode;
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div
      role="alert"
      className={cn(
        "flex items-center gap-3 border-b px-4 py-2.5 text-sm font-medium",
        tone === "critical"
          ? "border-estop/40 bg-estop/15 text-status-critical"
          : "border-status-warning/30 bg-status-warning/10 text-status-warning",
      )}
    >
      {icon}
      <span className="flex-1">{children}</span>
      {action}
    </div>
  );
}

export function Banners() {
  const { t } = useTranslation();
  const { snapshot, reachable, authError } = useLive();
  const [confirm, setConfirm] = useState(false);
  const safety = snapshot?.safety;
  const machine = snapshot?.machine;

  return (
    <>
      {(!reachable || authError) && (
        <Bar tone="critical" icon={<WifiOff className="size-4" />}>
          {authError || !hasToken()
            ? t(
                "Missing or invalid access token. Open the URL printed by the daemon (it ends in #token=…).",
              )
            : t("Daemon not reachable. Retrying… Controls may be stale: stay at the roaster.")}
        </Bar>
      )}
      {safety?.emergency_stop && (
        <Bar
          tone="critical"
          icon={<CircleAlert className="size-5" />}
          action={
            <Button variant="outline" size="sm" onClick={() => setConfirm(true)}>
              {t("Reset")}
            </Button>
          }
        >
          <span className="text-base font-bold tracking-wide uppercase">
            {t("Emergency stop active")}
          </span>
        </Bar>
      )}
      {machine && !safety?.connected && (
        <Bar tone="critical" icon={<WifiOff className="size-4" />}>
          {t("Link to the roaster lost. Check the Bluetooth connection and stay at the roaster.")}
        </Bar>
      )}
      {safety?.overheat && (
        <Bar tone="critical" icon={<Flame className="size-4" />}>
          {t("Overheat protection active.")}
        </Bar>
      )}
      {machine && !machine.link_loss_tested && (
        <Bar tone="warning" icon={<AlertTriangle className="size-4" />}>
          {t("Link-loss behaviour of this machine is untested — stay at the roaster.")}
        </Bar>
      )}
      <AlertDialog open={confirm} onOpenChange={setConfirm}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("Reset emergency stop?")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("Only reset when the roaster is in a safe state and you are standing next to it.")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={async () => {
                try {
                  applySnapshot(await api.resetEmergencyStop());
                } catch (e) {
                  toast("critical", e instanceof ApiError ? e.message : String(e));
                }
              }}
            >
              {t("Reset")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

/** Turns new daemon alarms into toasts (deduplicated by id) and beeps for warnings. */
export function AlarmWatcher() {
  const { snapshot } = useLive();
  const seen = useRef<Set<number> | null>(null);

  useEffect(() => {
    if (!snapshot) return;
    const first = seen.current === null;
    seen.current ??= new Set();
    for (const a of snapshot.alarms) {
      if (seen.current.has(a.id)) continue;
      seen.current.add(a.id);
      const age = Date.now() - Date.parse(a.at);
      if (first && age > 30_000) continue;
      toast(
        a.severity === "critical" ? "critical" : a.severity === "warning" ? "warning" : "info",
        a.message,
      );
      if (a.severity !== "info") beep(a.severity === "critical");
    }
  }, [snapshot]);
  return null;
}

export function Toasts() {
  const toasts = useToasts();
  const tone = {
    info: "border-status-info/40",
    success: "border-status-good/40",
    warning: "border-status-warning/50",
    critical: "border-estop/60",
  } as const;
  return (
    <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-96 max-w-[calc(100vw-2rem)] flex-col gap-2">
      {toasts.map((x) => (
        <div
          key={x.id}
          role={x.kind === "critical" || x.kind === "warning" ? "alert" : "status"}
          className={cn(
            "pointer-events-auto flex animate-in items-start gap-3 rounded-xl border bg-popover/95 px-4 py-3 text-sm shadow-xl backdrop-blur duration-200 fade-in slide-in-from-bottom-2",
            tone[x.kind],
          )}
        >
          <span className="flex-1">{x.text}</span>
          <button
            onClick={() => dismissToast(x.id)}
            aria-label="Dismiss"
            className="text-muted-foreground hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </div>
      ))}
    </div>
  );
}
