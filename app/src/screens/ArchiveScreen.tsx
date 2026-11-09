import { useEffect, useMemo, useState } from "react";
import { Download } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Roast, RoastSummary } from "@/api/types.ts";
import { inputCls } from "@/components/Dialogs.tsx";
import { RoastChart } from "@/components/RoastChart.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  EVENT_NAME,
  buildChartData,
  dropTime,
  rorStart,
  buildMarkers,
  chargeTime,
  developmentBand,
} from "@/lib/chartData.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber, weightLossPct } from "@/lib/units.ts";
import { attempt } from "@/state/actions.ts";
import { applySnapshot, useLive } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function weights(green: number | null | undefined, roasted: number | null | undefined): string {
  if (!green) return "";
  if (!roasted) return `${formatNumber(green, 0)} g`;
  const loss = weightLossPct(green, roasted);
  return [
    `${formatNumber(green, 0)} → ${formatNumber(roasted, 0)} g`,
    loss !== null ? `−${formatNumber(loss)} %` : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/** Set or clear the reference roast; hidden on daemons without reference support. */
function ReferenceButton({ id, size = "default" }: { id: string; size?: "default" | "sm" }) {
  const { t } = useTranslation();
  const { snapshot } = useLive();
  if (!snapshot || snapshot.reference === undefined) return null;
  const isRef = snapshot.reference?.id === id;
  return (
    <Button
      variant={isRef ? "secondary" : "outline"}
      size={size}
      aria-pressed={isRef}
      onClick={(e) => {
        e.stopPropagation();
        void attempt(async () => applySnapshot(await api.setReference(isRef ? null : id)));
      }}
    >
      {isRef ? t("Is reference") : t("Use as reference")}
    </Button>
  );
}

function Detail({ id, onChanged }: { id: string; onChanged: () => void }) {
  const { t, i18n } = useTranslation();
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
      data: buildChartData(points, base, rorStart(events, base), dropTime(events)),
      markers: buildMarkers(events, base, (k) => t(EVENT_NAME[k])),
      band: developmentBand(events, base, last),
    };
  }, [roast, id, t]);

  if (!roast || !view)
    return <p className="p-card text-[15px] text-muted-foreground">{t("Loading…")}</p>;

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
  const download = (path: string, name: string, type?: string) =>
    void api.download(path, name, type).catch((e) => toast("critical", errText(e)));

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-5">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="truncate text-xl font-semibold">{roast.title || id}</h2>
          <span className="text-[13px] text-muted-foreground">
            {roast.roasted_at
              ? new Date(roast.roasted_at).toLocaleString(i18n.language, {
                  dateStyle: "medium",
                  timeStyle: "short",
                })
              : id}
            {roast.batch?.beans ? ` · ${roast.batch.beans}` : ""}
          </span>
        </div>
        <div className="flex flex-wrap gap-2">
          <ReferenceButton id={id} />
          <Button
            variant="outline"
            onClick={() =>
              download(`/api/roasts/${encodeURIComponent(id)}/alog`, `${id}.alog`, "text/plain")
            }
          >
            <Download />
            .alog
          </Button>
          <Button
            variant="outline"
            onClick={() => download(`/api/roasts/${encodeURIComponent(id)}`, `${id}.roast.json`)}
          >
            <Download />
            .roast.json
          </Button>
        </div>
      </div>
      <div className="h-80 min-h-0 lg:h-auto lg:min-h-64 lg:flex-1">
        <RoastChart
          data={view.data}
          markers={view.markers}
          band={view.band}
          actuators
          label={t("Roast curve of {{title}}", { title: roast.title || id })}
        />
      </div>
      <div className="flex flex-wrap items-end gap-x-10 gap-y-4 border-t border-border pt-4">
        <div className="flex flex-col gap-1">
          <span className="text-[13px] text-muted-foreground">{t("Green weight")}</span>
          <span className="tabular text-value leading-none">
            {formatNumber(roast.batch?.green_weight_g, 0)} g
          </span>
        </div>
        <div className="flex flex-col gap-1">
          <span className="text-[13px] text-muted-foreground">{t("Weight loss")}</span>
          <span className="tabular text-value leading-none">
            {loss === null ? "–" : `${formatNumber(loss)} %`}
          </span>
        </div>
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <label htmlFor="roasted-weight" className="text-[13px] text-muted-foreground">
            {t("Roasted weight (g)")}
          </label>
          <span className="flex gap-2">
            <input
              id="roasted-weight"
              className={cn(inputCls, "w-32 tabular")}
              inputMode="decimal"
              value={weight}
              onChange={(e) => setWeight(e.target.value)}
            />
            <Button type="submit" variant="secondary">
              {t("Save")}
            </Button>
          </span>
        </form>
      </div>
    </div>
  );
}

export function ArchiveScreen() {
  const { t, i18n } = useTranslation();
  const { snapshot } = useLive();
  const [list, setList] = useState<RoastSummary[] | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const refId = snapshot?.reference?.id ?? null;

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
    <main className="flex min-h-0 flex-1 flex-col gap-section overflow-y-auto p-gutter lg:flex-row lg:overflow-hidden">
      <section
        aria-label={t("Roasts")}
        className="flex min-h-0 flex-col gap-3 lg:w-[26rem] lg:shrink-0"
      >
        <h2 className="text-[15px] font-medium text-muted-foreground">{t("Roasts")}</h2>
        {list === null ? (
          <p className="text-[15px] text-muted-foreground">{t("Loading…")}</p>
        ) : list.length === 0 ? (
          <p className="text-[15px] text-muted-foreground">{t("No roasts saved yet.")}</p>
        ) : (
          <ul className="-mx-2 flex min-h-0 flex-col overflow-y-auto">
            {list.map((r) => (
              <li key={r.id} className="border-b border-border last:border-b-0">
                <div
                  className={cn(
                    "flex items-center gap-3 rounded-xl px-2 transition-colors duration-150",
                    sel === r.id ? "bg-surface" : "hover:bg-surface",
                  )}
                >
                  <button
                    type="button"
                    onClick={() => setSel(r.id)}
                    aria-current={sel === r.id ? "true" : undefined}
                    className="flex min-w-0 flex-1 cursor-pointer flex-col gap-1 rounded-lg py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                  >
                    <span className="flex items-baseline gap-2">
                      <span className="truncate text-[15px] font-medium">{r.title || r.id}</span>
                      {refId === r.id && (
                        <span className="shrink-0 text-[13px] text-brand-text">
                          {t("Reference")}
                        </span>
                      )}
                    </span>
                    <span className="text-[13px] text-muted-foreground">
                      {new Date(r.roasted_at).toLocaleString(i18n.language, {
                        dateStyle: "medium",
                        timeStyle: "short",
                      })}
                    </span>
                    <span className="truncate tabular text-[13px] text-muted-foreground">
                      {[
                        formatClock(r.total_s),
                        r.drop_bt !== null ? `${t("Drop")} ${formatNumber(r.drop_bt, 0)}°` : null,
                        weights(r.green_weight_g, r.roasted_weight_g),
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  </button>
                  <ReferenceButton id={r.id} size="sm" />
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>
      {sel ? (
        <Detail id={sel} onChanged={() => setTick((n) => n + 1)} />
      ) : (
        <div className="grid flex-1 place-items-center rounded-card border border-dashed border-border-strong p-10 text-[15px] text-muted-foreground">
          {t("Select a roast to see its curve.")}
        </div>
      )}
    </main>
  );
}
