import { useTranslation } from "react-i18next";

import type { CommandSource, ControlLogEntry, DisplayScale, MachineInfo } from "@/api/types.ts";
import { rejectText } from "@/state/controls.ts";
import { actuatorShown, formatClock } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";

import { entryStatus, type Status } from "@/lib/actions.ts";

const STATUS_STYLE: Record<Status, string> = {
  applied: "text-foreground",
  clamped: "text-warning-text",
  rejected: "text-danger-text",
  awaiting: "text-muted-foreground",
};

function shownValue(display: DisplayScale | undefined, v: number | null | undefined): string {
  return v === null || v === undefined ? "–" : `${Math.round(actuatorShown(display, v))}`;
}

/** Newest first list of every control action of the running roast. */
export function ActionLog({
  entries,
  base,
  machine,
}: {
  entries: readonly ControlLogEntry[];
  /** CHARGE time; times are shown relative to it once the roast has started. */
  base: number | null;
  /** For actuators read in their own unit (drum rpm). */
  machine?: MachineInfo | null;
}) {
  const { t } = useTranslation();
  const source: Record<CommandSource, string> = {
    human: t("You"),
    ai: t("AI"),
    autopilot: t("Autopilot"),
    safety: t("Safety"),
  };
  const status: Record<Status, string> = {
    applied: t("Applied"),
    clamped: t("Limited"),
    rejected: t("Rejected"),
    awaiting: t("Awaiting confirmation"),
  };
  const rows = [...entries].reverse();
  return (
    <section
      aria-label={t("Actions")}
      className="flex min-h-0 flex-col gap-2 rounded-card border border-border-strong p-card"
    >
      <h2 className="text-[13px] text-muted-foreground">{t("Actions")}</h2>
      {rows.length === 0 ? (
        <p className="text-[15px] text-muted-foreground">{t("No actions yet.")}</p>
      ) : (
        <ol className="flex max-h-[60vh] flex-col gap-2 overflow-y-auto">
          {rows.map((e, i) => {
            const st = entryStatus(e);
            const display = machine?.actuators.find(
              (s) => s.actuator === e.request.actuator,
            )?.display;
            const pct = (v: number | null | undefined) => shownValue(display, v);
            const requested = e.request.value;
            const applied = e.outcome.outcome === "applied" ? e.outcome.value : null;
            const time = e.t === null || e.t === undefined ? null : e.t - (base ?? 0);
            return (
              <li key={`${e.at}-${i}`} className="flex flex-col gap-0.5 text-[14px] leading-snug">
                <div className="flex items-baseline gap-2">
                  <span className="w-12 shrink-0 tabular text-muted-foreground">
                    {time === null ? "" : formatClock(time)}
                  </span>
                  <span className="font-medium">{t(capitalize(e.request.actuator))}</span>
                  <span className={cn("tabular", STATUS_STYLE[st])}>
                    {st === "rejected"
                      ? `${pct(e.previous)} ↛ ${pct(requested)}`
                      : `${pct(e.previous)} → ${pct(applied ?? requested)}`}
                    {st === "clamped" && ` (${t("asked")} ${pct(requested)})`}
                    {display && ` ${t(display.unit)}`}
                  </span>
                  <span className="ml-auto text-[13px] text-muted-foreground">
                    {source[e.request.source]}
                  </span>
                </div>
                <div className={cn("pl-14 text-[13px]", STATUS_STYLE[st])}>
                  {status[st]}
                  {e.outcome.outcome === "rejected" && `: ${rejectText(e.outcome.reason)}`}
                  {e.request.reason && (
                    <span className="text-muted-foreground"> · {e.request.reason}</span>
                  )}
                </div>
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}

function capitalize(s: string): string {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
