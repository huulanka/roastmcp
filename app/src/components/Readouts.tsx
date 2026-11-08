import { useTranslation } from "react-i18next";

import type { Snapshot } from "@/api/types.ts";
import { Panel } from "@/components/Panel.tsx";
import { formatClock, formatNumber } from "@/lib/units.ts";

function Stat({
  label,
  value,
  unit,
  color,
  big,
}: {
  label: string;
  value: string;
  unit?: string;
  color?: string;
  big?: boolean;
}) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[10px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
        {label}
      </span>
      <span
        className={big ? "readout text-5xl font-semibold" : "readout text-2xl font-semibold"}
        style={{ color }}
      >
        {value}
        {unit && <span className="ml-1 text-xs font-normal text-muted-foreground">{unit}</span>}
      </span>
    </div>
  );
}

export function Readouts({ snapshot }: { snapshot: Snapshot }) {
  const { t } = useTranslation();
  const p = snapshot.phases;
  const parts = [
    { key: "drying", label: t("Drying"), s: p?.drying_s ?? null, c: "var(--phase-drying)" },
    { key: "maillard", label: t("Maillard"), s: p?.maillard_s ?? null, c: "var(--phase-maillard)" },
    {
      key: "dev",
      label: t("Development"),
      s: p?.development_s ?? null,
      c: "var(--phase-development)",
    },
  ];
  const sum = parts.reduce((a, x) => a + (x.s ?? 0), 0);

  return (
    <Panel className="flex flex-wrap items-center gap-x-8 gap-y-3 px-4 py-3">
      <Stat label="BT" value={formatNumber(snapshot.bt)} unit="°C" color="var(--series-bt)" big />
      <Stat
        label="RoR"
        value={formatNumber(snapshot.ror)}
        unit="°C/min"
        color="var(--series-ror)"
      />
      {snapshot.machine?.has_et && (
        <Stat label="ET" value={formatNumber(snapshot.et)} unit="°C" color="var(--series-et)" />
      )}
      <Stat
        label="DTR"
        value={p?.dtr_pct !== null && p?.dtr_pct !== undefined ? formatNumber(p.dtr_pct) : "–"}
        unit="%"
      />
      <div className="ml-auto flex min-w-64 flex-1 flex-col gap-2">
        <div className="flex h-1.5 overflow-hidden rounded-full bg-muted">
          {sum > 0 &&
            parts.map((x) => (
              <div
                key={x.key}
                className="h-full transition-[width] duration-300"
                style={{ width: `${((x.s ?? 0) / sum) * 100}%`, background: x.c }}
              />
            ))}
        </div>
        <div className="grid grid-cols-3 gap-3">
          {parts.map((x) => (
            <div key={x.key} className="flex flex-col">
              <span className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <span className="size-1.5 rounded-full" style={{ background: x.c }} />
                {x.label}
              </span>
              <span className="readout text-sm font-medium">
                {formatClock(x.s)}
                <span className="ml-1.5 text-[11px] font-normal text-muted-foreground">
                  {x.s !== null && sum > 0 ? `${formatNumber((x.s / sum) * 100, 0)}%` : ""}
                </span>
              </span>
            </div>
          ))}
        </div>
      </div>
    </Panel>
  );
}
