import { useTranslation } from "react-i18next";

import type { Snapshot } from "@/api/types.ts";
import { isRecording, liveDtr, type RorAlert } from "@/lib/roast.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { roastClock } from "@/state/helpers.ts";

function Stat({
  label,
  value,
  sub,
  className,
}: {
  label: string;
  value: string;
  sub: string;
  className?: string;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span className="text-[13px] text-muted-foreground">{label}</span>
      <span className={`readout text-readout ${className ?? ""}`}>{value}</span>
      <span className="truncate text-[13px] text-muted-foreground">{sub}</span>
    </div>
  );
}

/** The big bean temperature and the three numbers that matter next to it. */
export function Readouts({
  snapshot,
  rorDelta,
  alert,
}: {
  snapshot: Snapshot;
  /** RoR now minus RoR 30 s ago, °C/min. */
  rorDelta: number | null;
  alert: RorAlert | null;
}) {
  const { t } = useTranslation();
  const hasCharge = snapshot.charge_t !== null;
  const running = isRecording(snapshot.state);
  const clock = running ? roastClock(snapshot) : null;
  const dev = liveDtr(snapshot.events, snapshot.charge_t, snapshot.t);
  const delta = snapshot.reference?.delta_bt ?? null;
  const deltaText =
    delta === null
      ? ""
      : `${delta > 0 ? "+" : delta < 0 ? "−" : "±"}${formatNumber(Math.abs(delta))}`;
  const dRor =
    rorDelta === null
      ? "°C/min"
      : `${rorDelta > 0.05 ? "↑" : rorDelta < -0.05 ? "↓" : "→"} ${rorDelta > 0 ? "+" : rorDelta < 0 ? "−" : "±"}${formatNumber(Math.abs(rorDelta))} / 30 s`;

  return (
    <section aria-label={t("Readings")} className="flex flex-col">
      <div className="flex items-baseline justify-between text-[13px] text-muted-foreground">
        <span className="flex items-center gap-2">
          <span className="h-[3px] w-4 rounded-pill bg-brand" aria-hidden="true" />
          {t("Bean")}
        </span>
        {snapshot.machine?.has_et && (
          <span className="tabular">
            {t("Environment")} {formatNumber(snapshot.et)}°
          </span>
        )}
      </div>
      <div className="mt-1 flex items-baseline gap-1.5" aria-live="off">
        <span className="readout text-hero text-brand-text">{formatNumber(snapshot.bt)}</span>
        <span className="text-xl text-muted-foreground">°C</span>
        {/* Beside the temperature, not below: the controls need the height. */}
        {delta !== null && (
          <span
            className="ml-auto flex flex-col items-end"
            aria-label={t("{{delta}}° to reference", { delta: deltaText })}
          >
            <span
              className={cn(
                "tabular text-xl font-medium",
                Math.abs(delta) > 5 ? "text-brand-text" : "text-foreground",
              )}
            >
              {deltaText}°
            </span>
            <span className="text-[13px] text-muted-foreground">{t("to reference")}</span>
          </span>
        )}
      </div>
      <div className="mt-4 grid grid-cols-3 gap-3">
        <Stat
          label={
            alert === "crash" ? t("RoR · crash") : alert === "flick" ? t("RoR · flick") : "RoR"
          }
          value={formatNumber(snapshot.ror)}
          className={alert ? "text-warning-text" : "text-ror-text"}
          sub={dRor}
        />
        <Stat
          label={t("Time")}
          value={formatClock(clock)}
          sub={hasCharge ? t("since charge") : running ? t("recording") : t("not recording")}
        />
        <Stat
          label="DTR"
          value={dev ? `${formatNumber(dev.dtr, 0)} %` : "–"}
          className={dev ? "text-brand-text" : undefined}
          sub={
            dev ? t("{{time}} since crack", { time: formatClock(dev.devS) }) : t("from first crack")
          }
        />
      </div>
    </section>
  );
}
