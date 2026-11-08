import { useState } from "react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { EventKind, Snapshot } from "@/api/types.ts";
import { Panel } from "@/components/Panel.tsx";
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
import { cn } from "@/lib/utils.ts";
import { formatClock } from "@/lib/units.ts";
import { applySnapshot } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

const BUTTONS: { kind: EventKind; label: string }[] = [
  { kind: "charge", label: "CHARGE" },
  { kind: "dry_end", label: "DRY END" },
  { kind: "first_crack_start", label: "FC start" },
  { kind: "first_crack_end", label: "FC end" },
  { kind: "second_crack_start", label: "SC start" },
  { kind: "drop", label: "DROP" },
  { kind: "cool_end", label: "COOL END" },
];

export function EventButtons({ snapshot }: { snapshot: Snapshot }) {
  const { t } = useTranslation();
  const [asking, setAsking] = useState<EventKind | null>(null);
  const active = ["recording", "charged", "dropped", "cooling"].includes(snapshot.state);
  const base = snapshot.charge_t ?? snapshot.events.find((e) => e.kind === "charge")?.t ?? null;

  const guard = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    }
  };
  const mark = (kind: EventKind) => guard(async () => void (await api.markEvent(kind)));
  const label = BUTTONS.find((b) => b.kind === asking)?.label ?? "";

  return (
    <Panel className="p-2.5">
      <div className="grid grid-cols-4 gap-2 xl:grid-cols-7">
        {BUTTONS.map(({ kind, label }) => {
          const ev = snapshot.events.find((e) => e.kind === kind);
          return (
            <button
              key={kind}
              disabled={!(active || (kind === "charge" && snapshot.state === "connected"))}
              onClick={() => (ev ? setAsking(kind) : void mark(kind))}
              className={cn(
                "flex h-12 flex-col items-center justify-center rounded-xl border text-xs font-semibold tracking-wide transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:pointer-events-none disabled:opacity-40",
                ev
                  ? "border-brand/40 bg-brand-soft text-brand"
                  : "border-border bg-secondary text-secondary-foreground hover:bg-accent",
              )}
            >
              {label}
              <span className="mt-1 readout text-sm font-medium">
                {ev ? formatClock(ev.t - (base ?? ev.t)) : "–"}
              </span>
            </button>
          );
        })}
      </div>
      <AlertDialog open={asking !== null} onOpenChange={(o) => !o && setAsking(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("{{label}} is already set", { label })}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("Mark it again at the current time, or remove the mark?")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              variant="outline"
              onClick={() =>
                asking && void guard(async () => applySnapshot(await api.deleteEvent(asking)))
              }
            >
              {t("Undo mark")}
            </AlertDialogAction>
            <AlertDialogAction onClick={() => asking && void mark(asking)}>
              {t("Re-mark now")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Panel>
  );
}
