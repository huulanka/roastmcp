import { useEffect, useMemo, useState } from "react";
import { Download } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Roast, RoastSummary } from "@/api/types.ts";
import { inputCls } from "@/components/Dialogs.tsx";
import { Panel } from "@/components/Panel.tsx";
import { RoastChart } from "@/components/RoastChart.tsx";
import { Button } from "@/components/ui/button.tsx";
import { buildBands, buildChartData, buildMarkers, chargeTime } from "@/lib/chartData.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { weightLossPct } from "@/lib/units.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function Detail({ id, onChanged }: { id: string; onChanged: () => void }) {
  const { t } = useTranslation();
  const [roast, setRoast] = useState<Roast | null>(null);
  const [weight, setWeight] = useState("");

  useEffect(() => {
    let alive = true;
    api
      .roast(id)
      .then((r) => {
        if (!alive) return;
        setRoast(r);
        setWeight(r.batch?.roasted_weight_g?.toString() ?? "");
      })
      .catch((e) => toast("critical", errText(e)));
    return () => {
      alive = false;
    };
  }, [id]);

  const view = useMemo(() => {
    if (!roast || roast.id !== id) return null;
    const events = roast.events ?? [];
    const base = chargeTime(events);
    const points = roast.samples.map((s) => ({
      t: s.t,
      bt: s.bt,
      et: s.et ?? null,
      ror: s.ror ?? null,
      actuators: s.actuators ?? {},
    }));
    const last = points[points.length - 1]?.t ?? null;
    return {
      data: buildChartData(points, base),
      markers: buildMarkers(events, base),
      bands: buildBands(events, base, last),
    };
  }, [roast, id]);

  if (!roast || !view) return <p className="p-6 text-sm text-muted-foreground">{t("Loading…")}</p>;

  const save = async () => {
    const w = parseFloat(weight.replace(",", "."));
    if (!Number.isFinite(w)) return;
    try {
      setRoast(await api.patchRoast(id, { roasted_weight_g: w }));
      onChanged();
      toast("success", t("Saved"));
    } catch (e) {
      toast("critical", errText(e));
    }
  };
  const loss = weightLossPct(roast.batch?.green_weight_g, roast.batch?.roasted_weight_g);

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Panel className="h-[min(50vh,520px)] min-h-72 p-2 pt-3">
        <RoastChart data={view.data} markers={view.markers} bands={view.bands} />
      </Panel>
      <Panel title={roast.title || id}>
        <div className="flex flex-wrap items-end gap-x-8 gap-y-4 text-sm">
          <div>
            <div className="text-xs text-muted-foreground">{t("Green weight")}</div>
            <div className="readout text-lg font-medium">
              {formatNumber(roast.batch?.green_weight_g, 0)} g
            </div>
          </div>
          <div>
            <div className="text-xs text-muted-foreground">{t("Weight loss")}</div>
            <div className="readout text-lg font-medium">
              {loss === null ? "–" : `${formatNumber(loss)} %`}
            </div>
          </div>
          <label className="flex flex-col gap-1.5 text-xs text-muted-foreground">
            {t("Roasted weight (g)")}
            <span className="flex gap-2">
              <input
                className={cn(inputCls, "w-32")}
                inputMode="decimal"
                value={weight}
                onChange={(e) => setWeight(e.target.value)}
              />
              <Button variant="secondary" size="lg" onClick={() => void save()}>
                {t("Save")}
              </Button>
            </span>
          </label>
          <div className="ml-auto flex gap-2">
            <Button
              variant="outline"
              onClick={() =>
                void api
                  .download(
                    `/api/roasts/${encodeURIComponent(id)}/alog`,
                    `${id}.alog`,
                    "text/plain",
                  )
                  .catch((e) => toast("critical", errText(e)))
              }
            >
              <Download />
              .alog
            </Button>
            <Button
              variant="outline"
              onClick={() =>
                void api
                  .download(`/api/roasts/${encodeURIComponent(id)}`, `${id}.roast.json`)
                  .catch((e) => toast("critical", errText(e)))
              }
            >
              <Download />
              .roast.json
            </Button>
          </div>
        </div>
      </Panel>
    </div>
  );
}

export function ArchiveScreen() {
  const { t, i18n } = useTranslation();
  const [list, setList] = useState<RoastSummary[] | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let alive = true;
    api
      .roasts()
      .then((r) => alive && setList(r))
      .catch((e) => alive && (setList([]), toast("critical", errText(e))));
    return () => {
      alive = false;
    };
  }, [tick]);

  return (
    <div className="grid min-h-0 flex-1 gap-4 overflow-y-auto p-4 lg:grid-cols-[22rem_minmax(0,1fr)]">
      <Panel title={t("Roasts")} className="h-fit">
        {list === null ? (
          <p className="text-sm text-muted-foreground">{t("Loading…")}</p>
        ) : list.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("No roasts saved yet.")}</p>
        ) : (
          <ul className="flex flex-col gap-1.5">
            {list.map((r) => {
              const loss = weightLossPct(r.green_weight_g, r.roasted_weight_g);
              return (
                <li key={r.id}>
                  <button
                    onClick={() => setSel(r.id)}
                    className={cn(
                      "flex w-full flex-col gap-1 rounded-xl border px-3 py-2.5 text-left transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                      sel === r.id
                        ? "border-brand/50 bg-brand-soft"
                        : "border-transparent hover:bg-accent",
                    )}
                  >
                    <span className="truncate text-sm font-medium">{r.title || r.id}</span>
                    <span className="text-xs text-muted-foreground">
                      {new Date(r.roasted_at).toLocaleString(i18n.language)}
                    </span>
                    <span className="readout text-xs text-muted-foreground">
                      {formatClock(r.total_s)} · {t("Drop")} {formatNumber(r.drop_bt)}°C
                      {loss !== null && ` · −${formatNumber(loss)}%`}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </Panel>
      {sel ? (
        <Detail id={sel} onChanged={() => setTick((n) => n + 1)} />
      ) : (
        <div className="grid place-items-center rounded-2xl border border-dashed border-border-strong p-10 text-sm text-muted-foreground">
          {t("Select a roast to see its curve.")}
        </div>
      )}
    </div>
  );
}
