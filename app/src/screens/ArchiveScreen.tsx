import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, Download, Ellipsis, Plus, Trash2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type {
  Alignment,
  Comparison,
  Cupping,
  RestEntry,
  Roast,
  RoastGoal,
  RoastSummary,
} from "@/api/types.ts";
import { ConsistencyView } from "@/components/ConsistencyView.tsx";
import { CuppingDialog } from "@/components/CuppingDialog.tsx";
import { UnitInput } from "@/components/Dialogs.tsx";
import { RoastChart } from "@/components/RoastChart.tsx";
import { TastingView } from "@/components/TastingView.tsx";
import { Segmented } from "@/components/TopBar.tsx";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu.tsx";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select.tsx";
import { labelText } from "@/lib/batch.ts";
import {
  EVENT_NAME,
  buildChartData,
  dropTime,
  rorStart,
  buildMarkers,
  chargeTime,
  developmentBand,
  mergeReference,
} from "@/lib/chartData.ts";
import {
  comparisonCurve,
  diff,
  firstCrackShift,
  furthestAt,
  signed,
  signedClock,
} from "@/lib/compare.ts";
import { bestByRoast, restChip, restLine, scoreText } from "@/lib/tasting.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber, weightLossPct } from "@/lib/units.ts";
import { attempt } from "@/state/actions.ts";
import { applySnapshot, useLive } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

/** The weight loss alone, for the list ("−15.2 %"). */
function lossText(green: number | null | undefined, roasted: number | null | undefined) {
  const loss = weightLossPct(green, roasted);
  return loss === null ? null : `−${formatNumber(loss)} %`;
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

const GOAL_NAME: Record<RoastGoal, string> = {
  espresso: "Espresso",
  filter: "Filter",
  omni: "Omni",
  other: "Own",
};

const NO_COMPARE = "none";

function roastLabel(r: RoastSummary): string {
  return [r.batch_no, r.title || r.id].filter(Boolean).join(" · ");
}

function parseNumber(text: string): number | null {
  const v = parseFloat(text.replace(",", "."));
  return Number.isFinite(v) ? v : null;
}

function shortDate(iso: string, lang: string): string {
  return new Date(iso).toLocaleDateString(lang, { day: "numeric", month: "short" });
}

const CHIP: Record<RestEntry["state"], string> = {
  resting: "bg-surface-raised text-muted-foreground",
  ready: "bg-ok/15 text-foreground",
  past_best: "border border-border-strong text-muted-foreground",
};

/** Where a roast is in its rest window, as a small chip. */
function RestChip({ entry, large = false }: { entry: RestEntry; large?: boolean }) {
  const { t, i18n } = useTranslation();
  const date = (iso: string) => shortDate(iso, i18n.language);
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-pill px-2.5 whitespace-nowrap",
        large ? "h-8 text-[14px]" : "h-6 text-[13px]",
        CHIP[entry.state],
      )}
    >
      {large ? restLine(entry, t, date) : restChip(entry, t, date)}
    </span>
  );
}

type View = "curve" | "tasting" | "consistency";

/** One figure of the roast, with its difference to the compared roast below. */
function Metric({ label, value, delta }: { label: string; value: string; delta?: string | null }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[13px] text-muted-foreground">{label}</span>
      <span className="tabular text-value leading-none">{value}</span>
      <span className="h-4 tabular text-[13px] leading-none text-muted-foreground">
        {delta ?? ""}
      </span>
    </div>
  );
}

function Detail({
  id,
  summary,
  others,
  all,
  rest,
  cuppings,
  onChanged,
  onDeleted,
}: {
  id: string;
  summary: RoastSummary | null;
  /** The other roasts of the archive, to compare with. */
  others: RoastSummary[];
  /** The whole archive. */
  all: RoastSummary[];
  rest: RestEntry | null;
  /** This roast's tastings, newest first. */
  cuppings: Cupping[];
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [roast, setRoast] = useState<Roast | null>(null);
  const [weight, setWeight] = useState("");
  const [whole, setWhole] = useState("");
  const [ground, setGround] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [view, setView] = useState<View>("curve");
  const [compareId, setCompareId] = useState<string | null>(null);
  const [compared, setCompared] = useState<Roast | null>(null);
  const [align, setAlign] = useState<Alignment>("first_crack");
  const [comparison, setComparison] = useState<Comparison | null>(null);
  const [tasting, setTasting] = useState<Cupping | "new" | null>(null);
  const other = compareId ? (others.find((r) => r.id === compareId) ?? null) : null;

  useEffect(() => {
    if (!compareId) return;
    let alive = true;
    api
      .roast(compareId)
      .then((r) => alive && setCompared(r))
      .catch((e) => toast("critical", errText(e)));
    return () => {
      alive = false;
    };
  }, [compareId]);

  useEffect(() => {
    if (!compareId) return;
    let alive = true;
    api
      .compare(id, compareId, align)
      .then((c) => alive && setComparison(c))
      .catch(() => alive && setComparison(null));
    return () => {
      alive = false;
    };
  }, [id, compareId, align]);

  useEffect(() => {
    let alive = true;
    api
      .roast(id)
      .then((r) => {
        if (!alive) return;
        setRoast(r);
        setWeight(r.batch?.roasted_weight_g?.toString() ?? "");
        setWhole(r.roast_degree?.whole_bean?.toString() ?? "");
        setGround(r.roast_degree?.ground?.toString() ?? "");
      })
      .catch((e) => toast("critical", errText(e)));
    return () => {
      alive = false;
    };
  }, [id]);

  // Lined up on first crack only when both roasts have one.
  const fcAligned =
    align === "first_crack" &&
    typeof summary?.first_crack_s === "number" &&
    typeof other?.first_crack_s === "number";

  const chart = useMemo(() => {
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
    const own = buildChartData(points, base, rorStart(events, base), dropTime(events));
    const shift = fcAligned ? firstCrackShift(summary?.first_crack_s, other?.first_crack_s) : 0;
    const cmp = compareId && compared?.id === compareId ? comparisonCurve(compared, shift) : null;
    return {
      data: cmp ? mergeReference(own, cmp.points, cmp.rorFrom) : own,
      markers: buildMarkers(events, base, (k) => t(EVENT_NAME[k])),
      band: developmentBand(events, base, last),
    };
  }, [roast, id, t, compareId, compared, fcAligned, summary, other]);

  if (!roast || !chart)
    return <p className="p-card text-[15px] text-muted-foreground">{t("Loading…")}</p>;

  const save = async () => {
    const w = parseNumber(weight);
    try {
      setRoast(
        await api.patchRoast(id, {
          ...(w !== null ? { roasted_weight_g: w } : {}),
          roast_degree: {
            scale: roast.roast_degree?.scale ?? "agtron",
            whole_bean: parseNumber(whole),
            ground: parseNumber(ground),
          },
        }),
      );
      onChanged();
      toast("success", t("Saved"));
    } catch (e) {
      toast("critical", errText(e));
    }
  };
  const loss = weightLossPct(roast.batch?.green_weight_g, roast.batch?.roasted_weight_g);
  const otherLoss = other ? weightLossPct(other.green_weight_g, other.roasted_weight_g) : null;
  const download = (path: string, name: string, type?: string) =>
    void api.download(path, name, type).catch((e) => toast("critical", errText(e)));
  const batchNo = summary?.batch_no ?? roast.batch?.batch_no ?? null;
  const title = roast.title || id;
  const when = roast.roasted_at
    ? new Date(roast.roasted_at).toLocaleString(i18n.language, {
        dateStyle: "medium",
        timeStyle: "short",
      })
    : "";
  // What deleting gives back to the stock (simulator roasts never took any).
  const credit =
    summary?.lot_id && !summary.simulated && summary.green_weight_g ? summary.green_weight_g : null;
  const copyLabel = () => {
    const text = labelText(
      {
        batchNo,
        beans: roast.batch?.beans ?? title,
        goal: roast.plan?.goal ?? summary?.goal ?? null,
        roastedAt: roast.roasted_at ?? new Date().toISOString(),
      },
      t,
      (g) => t(GOAL_NAME[g]),
    );
    navigator.clipboard
      .writeText(text)
      .then(() => toast("success", t("Label copied")))
      .catch((e) => toast("critical", errText(e)));
  };
  const remove = async () => {
    try {
      const r = await api.deleteRoast(id);
      toast(
        "success",
        r.credited_g > 0
          ? t("Deleted. {{grams}} g are back in stock.", { grams: formatNumber(r.credited_g, 0) })
          : t("Deleted."),
      );
      onDeleted();
    } catch (e) {
      toast("critical", errText(e));
    }
  };

  const fc = summary?.first_crack_s ?? null;
  const total = summary?.total_s ?? null;
  const devS = typeof fc === "number" && typeof total === "number" ? total - fc : null;
  const cmp = other && comparison ? comparison : null;
  const gapAt = cmp ? furthestAt(cmp.curve, cmp.max_delta_bt) : null;
  const tabs = (
    <Segmented<View>
      label={t("View")}
      value={view}
      options={[
        { value: "curve", label: t("Curve") },
        {
          value: "tasting",
          label: cuppings.length ? t("Tasting · {{n}}", { n: cuppings.length }) : t("Tasting"),
        },
        { value: "consistency", label: t("Consistency") },
      ]}
      onChange={setView}
    />
  );

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-4">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="truncate text-xl font-semibold">
            {batchNo && <span className="tabular">{batchNo} · </span>}
            {title}
          </h2>
          <span className="text-[13px] text-muted-foreground">
            {when || id}
            {roast.batch?.beans ? ` · ${roast.batch.beans}` : ""}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {rest && <RestChip entry={rest} large />}
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
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="icon" aria-label={t("More")}>
                <Ellipsis />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent>
              <DropdownMenuItem
                onSelect={() =>
                  download(`/api/roasts/${encodeURIComponent(id)}`, `${id}.roast.json`)
                }
              >
                <Download />
                .roast.json
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={copyLabel}>
                <Copy />
                {t("Copy label")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem danger onSelect={() => setConfirmDelete(true)}>
                <Trash2 />
                {t("Delete…")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("Delete roast?")}</AlertDialogTitle>
            <AlertDialogDescription>
              {[batchNo, title].filter(Boolean).join(" · ")}
              {when ? `, ${when}` : ""}.{" "}
              {credit !== null &&
                t("{{grams}} g go back into stock.", { grams: formatNumber(credit, 0) }) + " "}
              {t("Its files move to the trash folder (roasts/trash) and can be restored there.")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={() => void remove()}>
              {t("Delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {view !== "consistency" && (
        <div className="flex flex-wrap items-center justify-between gap-3">
          {tabs}
          {view === "curve" && others.length > 0 && (
            <div className="flex flex-wrap items-center gap-2">
              <Select
                value={compareId ?? NO_COMPARE}
                onValueChange={(v) => {
                  setCompareId(v === NO_COMPARE ? null : v);
                  setComparison(null);
                }}
              >
                <SelectTrigger aria-label={t("Compare with")} className="w-56">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={NO_COMPARE}>
                    <span className="text-muted-foreground">{t("Compare with…")}</span>
                  </SelectItem>
                  <SelectSeparator />
                  {others.map((r) => (
                    <SelectItem key={r.id} value={r.id}>
                      <span className="flex min-w-0 items-baseline justify-between gap-4">
                        <span className="truncate">{roastLabel(r)}</span>
                        <span className="shrink-0 tabular text-[13px] text-muted-foreground">
                          {new Date(r.roasted_at).toLocaleDateString(i18n.language, {
                            dateStyle: "short",
                          })}
                        </span>
                      </span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {other && (
                <Segmented<Alignment>
                  label={t("Line up on")}
                  value={align}
                  options={[
                    { value: "charge", label: t("from charge") },
                    { value: "first_crack", label: t("on first crack") },
                  ]}
                  onChange={setAlign}
                />
              )}
            </div>
          )}
          {view === "tasting" && (
            <Button onClick={() => setTasting("new")}>
              <Plus />
              {t("Record tasting")}
            </Button>
          )}
        </div>
      )}

      {view === "curve" && (
        <>
          <div className="h-80 min-h-0 lg:h-auto lg:min-h-56 lg:flex-1">
            <RoastChart
              data={chart.data}
              markers={chart.markers}
              band={chart.band}
              actuators
              refName={other ? (other.batch_no ?? t("Comparison")) : undefined}
              label={t("Roast curve of {{title}}", { title: roast.title || id })}
            />
          </div>
          {other && align === "first_crack" && !fcAligned && (
            <p className="-mt-2 text-[13px] text-muted-foreground">
              {t("One of the two has no first crack: lined up on charge.")}
            </p>
          )}
          <div className="flex flex-wrap items-start justify-between gap-x-8 gap-y-4 border-t border-border pt-4">
            <div className="flex flex-wrap gap-x-7 gap-y-3">
              <Metric
                label={t("To first crack")}
                value={formatClock(fc)}
                delta={other && signedClock(diff(fc, other.first_crack_s))}
              />
              <Metric
                label={t("Development time")}
                value={formatClock(devS)}
                delta={cmp && signedClock(cmp.delta.development_s ?? null)}
              />
              <Metric
                label={t("DTR")}
                value={`${formatNumber(summary?.dtr_pct)} %`}
                delta={other && signed(diff(summary?.dtr_pct, other.dtr_pct))}
              />
              <Metric
                label={t("Drop")}
                value={`${formatNumber(summary?.drop_bt, 0)}°`}
                delta={other && signed(diff(summary?.drop_bt, other.drop_bt), 0)}
              />
              {cmp ? (
                <Metric
                  label={t("Largest gap")}
                  value={cmp.max_delta_bt === null ? "–" : `${formatNumber(cmp.max_delta_bt)}°`}
                  delta={
                    gapAt === null
                      ? null
                      : cmp.align === "first_crack"
                        ? gapAt < 0
                          ? t("{{t}} before first crack", { t: formatClock(-gapAt) })
                          : t("{{t}} after first crack", { t: formatClock(gapAt) })
                        : t("at {{t}}", { t: formatClock(gapAt) })
                  }
                />
              ) : (
                <Metric
                  label={t("Duration")}
                  value={formatClock(total)}
                  delta={other && signedClock(diff(total, other.total_s))}
                />
              )}
              <Metric
                label={t("Weight loss")}
                value={loss === null ? "–" : `${formatNumber(loss)} %`}
                delta={
                  other
                    ? signed(diff(loss, otherLoss))
                    : roast.batch?.green_weight_g
                      ? t("of {{grams}} g", { grams: formatNumber(roast.batch.green_weight_g, 0) })
                      : null
                }
              />
            </div>
            <form
              className="flex items-end gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void save();
              }}
            >
              <label className="flex w-28 flex-col gap-1 text-[13px] text-muted-foreground">
                {t("Roasted weight")}
                <UnitInput
                  unit="g"
                  inputMode="decimal"
                  value={weight}
                  onChange={(e) => setWeight(e.target.value)}
                />
              </label>
              <label className="flex w-32 flex-col gap-1 text-[13px] text-muted-foreground">
                {t("Colour, beans")}
                <UnitInput
                  unit="Agtron"
                  compact
                  inputMode="decimal"
                  value={whole}
                  onChange={(e) => setWhole(e.target.value)}
                />
              </label>
              <label className="flex w-32 flex-col gap-1 text-[13px] text-muted-foreground">
                {t("Colour, ground")}
                <UnitInput
                  unit="Agtron"
                  compact
                  inputMode="decimal"
                  value={ground}
                  onChange={(e) => setGround(e.target.value)}
                />
              </label>
              <Button type="submit" variant="secondary">
                {t("Save")}
              </Button>
            </form>
          </div>
        </>
      )}

      {view === "tasting" && (
        <TastingView cuppings={cuppings} onEdit={(c) => setTasting(c)} onChanged={onChanged} />
      )}

      {view === "consistency" &&
        (summary ? <ConsistencyView summary={summary} all={all} tabs={tabs} /> : tabs)}

      {tasting && (
        <CuppingDialog
          roast={{
            id,
            label: [batchNo, title].filter(Boolean).join(" · "),
            roasted_at: roast.roasted_at ?? summary?.roasted_at ?? new Date().toISOString(),
            lot_id: summary?.lot_id ?? null,
          }}
          cupping={tasting === "new" ? null : tasting}
          onClose={() => setTasting(null)}
          onSaved={() => {
            setTasting(null);
            onChanged();
          }}
        />
      )}
    </div>
  );
}

export function ArchiveScreen() {
  const { t, i18n } = useTranslation();
  const { snapshot } = useLive();
  const [list, setList] = useState<RoastSummary[] | null>(null);
  const [rest, setRest] = useState<Map<string, RestEntry>>(new Map());
  const [cuppings, setCuppings] = useState<Cupping[]>([]);
  const [filter, setFilter] = useState<"all" | "ready">("all");
  const [sel, setSel] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const best = useMemo(() => bestByRoast(cuppings), [cuppings]);
  const refId = snapshot?.reference?.id ?? null;
  const picker = useRef<HTMLInputElement>(null);

  const importFiles = async (files: File[]) => {
    let added = 0;
    let known = 0;
    let lastId: string | null = null;
    for (const f of files) {
      try {
        const r = await api.importAlog(f);
        if (r.already_there) known++;
        else added++;
        lastId = r.id;
      } catch (e) {
        toast("critical", `${f.name}: ${errText(e)}`);
      }
    }
    if (added + known > 0) {
      toast(
        "success",
        known > 0
          ? t("{{added}} imported, {{known}} already in the archive", { added, known })
          : t("{{added}} imported", { added }),
      );
      setTick((n) => n + 1);
      if (lastId) setSel(lastId);
    }
  };

  useEffect(() => {
    let alive = true;
    api
      .roasts()
      .then((r) => alive && setList(r))
      .catch((e) => alive && (setList([]), toast("critical", errText(e))));
    // Rest and tastings only add to the list; without them it still works.
    api
      .rest()
      .then((r) => alive && setRest(new Map(r.map((e) => [e.roast_id, e]))))
      .catch(() => undefined);
    api
      .allCuppings()
      .then((c) => alive && setCuppings(c))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [tick]);

  const ready = (list ?? []).filter((r) => rest.get(r.id)?.state === "ready");
  const shown = filter === "ready" ? ready : (list ?? []);

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-section overflow-y-auto p-gutter lg:flex-row lg:overflow-hidden">
      <section
        aria-label={t("Roasts")}
        className="flex min-h-0 flex-col gap-3 lg:w-[26rem] lg:shrink-0"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault();
          const files = Array.from(e.dataTransfer.files).filter((f) => f.name.endsWith(".alog"));
          if (files.length > 0) void importFiles(files);
        }}
      >
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-[15px] font-medium text-muted-foreground">{t("Roasts")}</h2>
          <span className="flex items-center gap-2">
            <Segmented<"all" | "ready">
              label={t("Show")}
              value={filter}
              options={[
                { value: "all", label: t("All") },
                {
                  value: "ready",
                  label: ready.length
                    ? t("Ready to drink · {{n}}", { n: ready.length })
                    : t("Ready to drink"),
                },
              ]}
              onChange={setFilter}
            />
            <Button
              variant="ghost"
              size="icon"
              aria-label={t("Import")}
              title={t("Import .alog files")}
              onClick={() => picker.current?.click()}
            >
              <Upload />
            </Button>
          </span>
          <input
            ref={picker}
            type="file"
            accept=".alog"
            multiple
            hidden
            onChange={(e) => {
              const files = Array.from(e.target.files ?? []);
              e.target.value = "";
              if (files.length > 0) void importFiles(files);
            }}
          />
        </div>
        {list === null ? (
          <p className="text-[15px] text-muted-foreground">{t("Loading…")}</p>
        ) : list.length === 0 ? (
          <p className="text-[15px] text-muted-foreground">{t("No roasts saved yet.")}</p>
        ) : shown.length === 0 ? (
          <p className="text-[15px] text-muted-foreground">
            {t("No roast is in its window right now.")}
          </p>
        ) : (
          <ul className="-mx-2 flex min-h-0 flex-col overflow-y-auto">
            {shown.map((r) => (
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
                      {r.batch_no && (
                        <span className="shrink-0 tabular text-[15px] font-medium text-muted-foreground">
                          {r.batch_no}
                        </span>
                      )}
                      <span className="truncate text-[15px] font-medium">{r.title || r.id}</span>
                      {refId === r.id && (
                        <span className="shrink-0 text-[13px] text-brand-text">
                          {t("Reference")}
                        </span>
                      )}
                    </span>
                    <span className="flex items-center justify-between gap-2 text-[13px] text-muted-foreground">
                      <span className="truncate">
                        {new Date(r.roasted_at).toLocaleString(i18n.language, {
                          dateStyle: "medium",
                          timeStyle: "short",
                        })}
                      </span>
                      {rest.get(r.id) && <RestChip entry={rest.get(r.id) as RestEntry} />}
                    </span>
                    <span className="truncate tabular text-[13px] text-muted-foreground">
                      {[
                        formatClock(r.total_s),
                        r.drop_bt !== null ? `${t("Drop")} ${formatNumber(r.drop_bt, 0)}°` : null,
                        lossText(r.green_weight_g, r.roasted_weight_g),
                        best.get(r.id)
                          ? scoreText(best.get(r.id) as Cupping, t, formatNumber)
                          : null,
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
        <Detail
          key={sel}
          id={sel}
          summary={list?.find((r) => r.id === sel) ?? null}
          others={(list ?? []).filter((r) => r.id !== sel)}
          all={list ?? []}
          rest={rest.get(sel) ?? null}
          cuppings={cuppings.filter((c) => c.roast_id === sel)}
          onChanged={() => setTick((n) => n + 1)}
          onDeleted={() => {
            setSel(null);
            setTick((n) => n + 1);
          }}
        />
      ) : (
        <div className="grid flex-1 place-items-center rounded-card border border-dashed border-border-strong p-10 text-[15px] text-muted-foreground">
          {t("Select a roast to see its curve.")}
        </div>
      )}
    </main>
  );
}
