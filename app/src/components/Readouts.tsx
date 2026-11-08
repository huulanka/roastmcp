import { useTranslation } from "react-i18next";

import type { Snapshot } from "@/api/types.ts";
import { Panel } from "@/components/Panel.tsx";
import { formatClock, formatNumber } from "@/lib/units.ts";

function Stat({
  label,
  value,
  unit,
  color,
}: {
  label: string;
  value: string;
  unit?: string;
  color?: string;
}) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[11px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
        {label}
      </span>
      <span className="readout text-3xl font-semibold" style={{ color }}>
        {value}
        {unit && <span className="ml-1 text-sm font-normal text-muted-foreground">{unit}</span>}
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
    <Panel>
      <div className="flex flex-wrap items-end gap-x-10 gap-y-4">
        <div className="flex flex-col gap-1">
          <span className="text-[11px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
            BT
          </span>
          <span className="readout text-6xl font-semibold" style={{ color: "var(--series-bt)" }}>
            {formatNumber(snapshot.bt)}
            <span className="ml-1 text-xl font-normal text-muted-foreground">°C</span>
          </span>
        </div>
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
      </div>
      <div className="mt-5">
        <div className="flex h-2 overflow-hidden rounded-full bg-muted">
          {sum > 0 &&
            parts.map((x) => (
              <div
                key={x.key}
                className="h-full transition-[width] duration-300"
                style={{ width: `${((x.s ?? 0) / sum) * 100}%`, background: x.c }}
              />
            ))}
        </div>
        <div className="mt-3 grid grid-cols-3 gap-3 text-sm">
          {parts.map((x) => (
            <div key={x.key} className="flex flex-col">
              <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <span className="size-2 rounded-full" style={{ background: x.c }} />
                {x.label}
              </span>
              <span className="mt-1 readout text-lg font-medium">
                {formatClock(x.s)}
                <span className="ml-1.5 text-xs font-normal text-muted-foreground">
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
