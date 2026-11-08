import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Phases } from "@/api/types.ts";
import { rangeAt, rangeSpans, scaleGradient, scalePos, scaleTicks } from "@/lib/roastScale.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";

function Swatch({ className }: { className: string }) {
  return <span aria-hidden="true" className={cn("w-4 shrink-0", className)} />;
}

/** Heading of the curve with the legend; the reference can be cleared from here. */
export function CurveLegend({
  reference,
  projection,
  onClearReference,
}: {
  reference: string | null;
  projection: boolean;
  onClearReference: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-1">
      <h2 className="text-[15px] font-medium text-muted-foreground">{t("Curve")}</h2>
      <ul className="flex flex-wrap items-center gap-x-5 gap-y-1 text-[13px] text-muted-foreground">
        <li className="flex items-center gap-2">
          <Swatch className="h-[3px] rounded-pill bg-brand" />
          {t("Bean")}
        </li>
        <li className="flex items-center gap-2">
          <Swatch className="h-[3px] rounded-pill bg-ror" />
          RoR
        </li>
        {reference !== null && (
          <li className="flex items-center gap-2">
            <Swatch className="border-t-2 border-dashed border-reference" />
            <span className="max-w-56 truncate">
              {t("Reference")} · {reference}
            </span>
            <button
              type="button"
              onClick={onClearReference}
              aria-label={t("Clear reference")}
              className="-my-3 grid size-touch place-items-center rounded-md text-muted-foreground hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
            >
              <X className="size-4" />
            </button>
          </li>
        )}
        {projection && (
          <li className="flex items-center gap-2">
            <Swatch className="border-t-2 border-dashed border-brand" />
            {t("Projection")}
          </li>
        )}
      </ul>
    </div>
  );
}

/** Phase durations as one quiet line. */
export function PhaseLine({ phases }: { phases: Phases | null }) {
  const { t } = useTranslation();
  const part = (label: string, s: number | null | undefined, extra?: string, accent = false) => (
    <span className="whitespace-nowrap">
      {label}{" "}
      <span className={accent && s !== null ? "text-brand-text" : "text-foreground"}>
        {formatClock(s)}
        {extra}
      </span>
    </span>
  );
  const dtr = phases?.dtr_pct;
  return (
    <p className="flex flex-wrap gap-x-6 gap-y-1 tabular text-[13px] text-muted-foreground">
      {part(t("Drying"), phases?.drying_s ?? null)}
      {part(t("Maillard"), phases?.maillard_s ?? null)}
      {part(
        t("Development"),
        phases?.development_s ?? null,
        dtr !== null && dtr !== undefined ? ` · ${formatNumber(dtr, 0)} %` : "",
        true,
      )}
    </p>
  );
}

export interface ScaleMarker {
  bt: number;
  label: string;
}

/**
 * Roast-degree scale by bean temperature: bean colour from green to dark, the
 * phase and degree ranges, the current bean temperature, and faint markers for
 * the projected next milestone and the reference's drop.
 */
export function RoastScaleBar({
  bt,
  target,
  reference,
}: {
  bt: number | null;
  target: ScaleMarker | null;
  reference: ScaleMarker | null;
}) {
  const { t } = useTranslation();
  const pos = bt !== null ? scalePos(bt) : null;
  const current = bt !== null ? rangeAt(bt) : null;
  const pct = (p: number) => `${(p * 100).toFixed(2)}%`;

  return (
    <div
      className="flex flex-col gap-1.5"
      role="group"
      aria-label={
        current && bt !== null
          ? t("Roast degree: {{range}} at {{bt}} °C", {
              range: t(current.label),
              bt: formatNumber(bt),
            })
          : t("Roast degree")
      }
    >
      <div className="relative h-4 text-[13px] leading-4">
        {rangeSpans().map(({ range, start, width }) => (
          <span
            key={range.label}
            className={cn(
              "absolute truncate px-1 text-center",
              range === current ? "font-medium text-foreground" : "text-muted-foreground",
            )}
            style={{ left: pct(start), width: pct(width) }}
          >
            {t(range.label)}
          </span>
        ))}
      </div>
      <div className="relative h-3.5">
        <div className="absolute inset-0 rounded-pill" style={{ background: scaleGradient() }} />
        {pos !== null && (
          <div
            className="absolute inset-y-0 right-0 rounded-r-pill bg-background/55"
            style={{ left: pct(pos) }}
          />
        )}
        {scaleTicks().map((tick) => (
          <div
            key={tick}
            aria-hidden="true"
            className="absolute inset-y-0 w-0.5 -translate-x-1/2 bg-background"
            style={{ left: pct(scalePos(tick)) }}
          />
        ))}
        {reference && (
          <div
            title={reference.label}
            className="absolute -inset-y-1 w-0.5 -translate-x-1/2 rounded-pill bg-reference"
            style={{ left: pct(scalePos(reference.bt)) }}
          />
        )}
        {target && (
          <div
            title={target.label}
            className="absolute -inset-y-1 w-0 -translate-x-1/2 border-l-2 border-dashed border-brand"
            style={{ left: pct(scalePos(target.bt)) }}
          />
        )}
        {pos !== null && (
          <div
            className="absolute -inset-y-1.5 w-1 -translate-x-1/2 rounded-pill bg-foreground ring-2 ring-background"
            style={{ left: pct(pos) }}
          />
        )}
      </div>
      <div className="relative h-4 tabular text-[12px] leading-4 text-muted-foreground">
        {scaleTicks().map((tick) => (
          <span
            key={tick}
            className="absolute -translate-x-1/2"
            style={{ left: pct(scalePos(tick)) }}
          >
            {tick}°
          </span>
        ))}
      </div>
    </div>
  );
}
