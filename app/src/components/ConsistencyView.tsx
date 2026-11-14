import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { ConsistencyResult, RoastSummary, Spread, SpreadKey } from "@/api/types.ts";
import { Segmented } from "@/components/TopBar.tsx";
import { FIGURES, furthest, position, unusual, type FigureKind } from "@/lib/consistency.ts";
import { signed, signedClock } from "@/lib/compare.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";

type GroupKind = "bean" | "profile";

function value(kind: FigureKind, v: number | null | undefined, precise = false): string {
  if (typeof v !== "number") return "–";
  if (kind === "clock") return formatClock(v);
  if (kind === "temp") return `${formatNumber(v, precise ? 1 : 0)}°`;
  return `${formatNumber(v)} %`;
}

function spreadText(kind: FigureKind, s: Spread): string {
  if (kind === "clock") return `±${formatClock(s.sd)}`;
  return `±${formatNumber(s.sd)}`;
}

function deltaText(kind: FigureKind, delta: number): string {
  if (kind === "clock") return signedClock(delta) ?? "±0:00";
  const d = signed(delta) ?? "±0";
  return kind === "temp" ? `${d}°` : `${d} %`;
}

const COLS = "grid grid-cols-[10rem_minmax(0,1fr)_5rem_4.5rem_5rem] items-center gap-x-5";

/** How the roast sits among the others of its bean or its profile. */
export function ConsistencyView({
  summary,
  all,
  tabs,
}: {
  summary: RoastSummary;
  /** The whole archive, to find the roasts of the same bean when it has no lot. */
  all: RoastSummary[];
  /** The view switch, shown left in the same row as the group choice. */
  tabs: ReactNode;
}) {
  const { t } = useTranslation();
  const sim = !!summary.simulated;
  const [groups, setGroups] = useState<Partial<Record<GroupKind, ConsistencyResult>>>({});
  const [kind, setKind] = useState<GroupKind>("bean");
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let alive = true;
    const bean = summary.lot_id
      ? api.consistency({ lot_id: summary.lot_id }, sim)
      : summary.beans
        ? api.consistency(
            {
              ids: all
                .filter((r) => !r.lot_id && r.beans === summary.beans && !!r.simulated === sim)
                .map((r) => r.id),
            },
            sim,
          )
        : Promise.resolve(null);
    const profile = summary.profile_version_id
      ? api
          .profileVersion(summary.profile_version_id)
          .then(({ version }) => api.consistency({ profile_id: version.profile_id }, sim))
      : Promise.resolve(null);
    Promise.allSettled([bean, profile]).then(([b, p]) => {
      if (!alive) return;
      const next: Partial<Record<GroupKind, ConsistencyResult>> = {};
      if (b.status === "fulfilled" && b.value) next.bean = b.value;
      if (p.status === "fulfilled" && p.value) next.profile = p.value;
      // A group that cannot be read (a deleted lot or profile) is simply not offered.
      setGroups(next);
      setKind(next.bean ? "bean" : "profile");
      setLoaded(true);
    });
    return () => {
      alive = false;
    };
  }, [summary, all, sim]);

  if (!loaded)
    return (
      <>
        <div className="flex items-center">{tabs}</div>
        <p className="text-[15px] text-muted-foreground">{t("Loading…")}</p>
      </>
    );

  const result = groups[kind];
  const options = (["bean", "profile"] as const)
    .filter((k) => groups[k])
    .map((k) => ({
      value: k,
      label:
        k === "bean"
          ? t("this bean · {{n}}", { n: groups[k]?.consistency.roasts ?? 0 })
          : t("this profile · {{n}}", { n: groups[k]?.consistency.roasts ?? 0 }),
    }));
  const own = result?.roasts.find((r) => r.id === summary.id)?.figures ?? null;
  const n = result?.consistency.roasts ?? 0;

  const head = (
    <div className="flex flex-wrap items-center justify-between gap-3">
      {tabs}
      {options.length > 0 && (
        <span className="flex items-center gap-3">
          <span className="text-[13px] text-muted-foreground">{t("Compared with")}</span>
          <Segmented<GroupKind>
            label={t("Compared with")}
            value={kind}
            options={options}
            onChange={setKind}
          />
        </span>
      )}
    </div>
  );

  if (!result || n < 2 || !own) {
    return (
      <div className="flex min-h-0 flex-1 flex-col gap-4">
        {head}
        <div className="grid min-h-48 flex-1 place-items-center rounded-card border border-dashed border-border-strong p-10 text-center text-[15px] text-muted-foreground">
          {summary.lot_id || summary.beans || summary.profile_version_id
            ? t("Needs at least two finished roasts of this bean or profile.")
            : t("This roast has no bean and no profile to compare it with.")}
        </div>
      </div>
    );
  }

  const far = furthest(result);
  const odd = unusual(result, summary.id);
  const figure = (key: SpreadKey) => FIGURES.find((f) => f.key === key);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4">
      {head}
      <div className="flex min-h-0 shrink-0 flex-col overflow-y-auto rounded-card border border-card-border bg-surface px-card py-1">
        <div className={`${COLS} py-2 text-[13px] text-muted-foreground`}>
          <span>{t("Figure")}</span>
          <span>{t("Range of the {{n}} roasts", { n })}</span>
          <span className="text-right">{t("Mean")}</span>
          <span className="text-right">{t("Spread")}</span>
          <span className="text-right">{t("This one")}</span>
        </div>
        {FIGURES.map((f) => {
          const s = result.consistency[f.key];
          const v = own[f.key];
          if (!s) return null;
          return (
            <div key={f.key} className={`${COLS} border-t border-border py-2.5 text-[15px]`}>
              <span className="truncate">{t(f.name)}</span>
              <span className="flex items-center gap-2.5 tabular text-[13px] text-muted-foreground">
                <span className="w-12 shrink-0 text-right">{value(f.kind, s.min)}</span>
                <span className="relative h-2 flex-1 rounded-pill bg-surface-raised">
                  {typeof v === "number" && (
                    <span
                      className="absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-pill bg-brand"
                      style={{ left: `${position(v, s) * 100}%` }}
                    />
                  )}
                </span>
                <span className="w-12 shrink-0">{value(f.kind, s.max)}</span>
              </span>
              <span className="text-right tabular">{value(f.kind, s.mean, true)}</span>
              <span className="text-right tabular text-muted-foreground">
                {spreadText(f.kind, s)}
              </span>
              <span className="text-right tabular font-semibold">{value(f.kind, v)}</span>
            </div>
          );
        })}
      </div>
      <div className="flex flex-wrap items-start gap-x-8 gap-y-3">
        {result.consistency.curve_sd_c !== null && (
          <div className="flex flex-col gap-1">
            <span className="text-[13px] text-muted-foreground">
              {t("Curves apart on average")}
            </span>
            <span className="tabular text-value leading-none">
              {formatNumber(result.consistency.curve_sd_c)}°
            </span>
          </div>
        )}
        {far && (
          <div className="flex max-w-72 min-w-0 flex-col gap-1">
            <span className="text-[13px] text-muted-foreground">{t("Furthest from the mean")}</span>
            <span className="truncate text-[17px] font-medium">{far.label}</span>
            <span className="text-[13px] text-muted-foreground">
              {t(figure(far.key)?.name ?? "")}{" "}
              {deltaText(figure(far.key)?.kind ?? "pct", far.delta)}
            </span>
          </div>
        )}
        <div className="flex min-w-56 flex-1 flex-col gap-1">
          <span className="text-[13px] text-muted-foreground">{t("This roast")}</span>
          <span className="text-[17px] font-medium">
            {n < 3
              ? t("Too few roasts yet to say what is usual.")
              : odd.length === 0
                ? t("Within the usual spread in every figure.")
                : t("Outside the usual spread: {{list}}.", {
                    list: odd
                      .map(
                        (d) =>
                          `${t(figure(d.key)?.name ?? "")} ${deltaText(figure(d.key)?.kind ?? "pct", d.delta)}`,
                      )
                      .join(", "),
                  })}
          </span>
        </div>
      </div>
    </div>
  );
}
