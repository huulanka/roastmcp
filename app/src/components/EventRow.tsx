import { useState } from "react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { EventKind, Snapshot } from "@/api/types.ts";
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
import { EVENT_NAME, EVENT_ORDER } from "@/lib/chartData.ts";
import { isRecording, primaryLabel, type PrimaryAction } from "@/lib/roast.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { attempt } from "@/state/actions.ts";
import { applySnapshot } from "@/state/live.ts";

/** The one big button for the next step; Enter does the same. */
export function PrimaryButton({
  action,
  onRun,
}: {
  action: PrimaryAction | null;
  onRun: () => void;
}) {
  const { t } = useTranslation();
  if (!action) return null;
  return (
    <Button size="lg" onClick={onRun} className="min-w-52 justify-between">
      {primaryLabel(t, action)}
      <kbd
        aria-hidden="true"
        className="rounded-sm bg-brand-foreground/15 px-2 py-0.5 font-sans text-[13px] font-medium"
      >
        ↵
      </kbd>
    </Button>
  );
}

/**
 * Every roast event in order. A set event shows its time and temperature
 * (auto-detected ones say so) and can be re-marked or removed; an unset one is
 * marked now.
 */
export function EventSegments({ snapshot }: { snapshot: Snapshot }) {
  const { t } = useTranslation();
  const [asking, setAsking] = useState<EventKind | null>(null);
  const active = isRecording(snapshot.state);
  const base = snapshot.charge_t ?? snapshot.events.find((e) => e.kind === "charge")?.t ?? null;
  const mark = (kind: EventKind) => attempt(() => api.markEvent(kind));
  const askingName = asking ? t(EVENT_NAME[asking]) : "";

  return (
    <>
      <div
        role="group"
        aria-label={t("Events")}
        className="grid grid-cols-[repeat(auto-fit,minmax(5.75rem,1fr))] gap-1 rounded-xl border border-card-border bg-surface p-1"
      >
        {EVENT_ORDER.map((kind) => {
          const ev = snapshot.events.find((e) => e.kind === kind);
          const enabled = active || (kind === "charge" && snapshot.state === "connected");
          const auto = ev && (ev.origin === "auto" || ev.origin === "detector");
          const name = t(EVENT_NAME[kind]);
          const when = ev
            ? [
                formatClock(ev.t - (base ?? ev.t)),
                ev.bt !== null ? `${formatNumber(ev.bt, 0)}°` : null,
              ]
                .filter(Boolean)
                .join(" · ")
            : null;
          return (
            <button
              key={kind}
              type="button"
              disabled={!enabled}
              aria-pressed={!!ev}
              aria-label={
                ev
                  ? t("{{name}} at {{when}}: change", { name, when })
                  : t("Mark {{name}} now", { name })
              }
              onClick={() => (ev ? setAsking(kind) : void mark(kind))}
              className={cn(
                "flex h-13 min-w-0 cursor-pointer flex-col justify-center gap-0.5 rounded-lg px-2 text-left transition-colors duration-150 outline-none hover:bg-surface-raised focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-default disabled:opacity-45 disabled:hover:bg-transparent",
              )}
            >
              <span className="truncate text-[13px] text-muted-foreground">
                {name}
                {auto && ` · ${t("auto")}`}
              </span>
              <span
                className={cn(
                  "truncate tabular text-[13px]",
                  ev ? "font-medium text-foreground" : "text-muted-foreground",
                )}
              >
                {when ?? "–"}
              </span>
            </button>
          );
        })}
      </div>
      <AlertDialog open={asking !== null} onOpenChange={(o) => !o && setAsking(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("{{label}} is already set", { label: askingName })}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("Mark it again at the current time, or remove the mark?")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              variant="outline"
              onClick={() =>
                asking && void attempt(async () => applySnapshot(await api.deleteEvent(asking)))
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
    </>
  );
}
