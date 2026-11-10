import { useEffect, useMemo, useRef, useState } from "react";
import { Copy, Download, Ellipsis, Trash2, Upload } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Roast, RoastGoal, RoastSummary } from "@/api/types.ts";
import { UnitInput } from "@/components/Dialogs.tsx";
import { RoastChart } from "@/components/RoastChart.tsx";
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
import { comparisonCurve, diff, signed, signedClock } from "@/lib/compare.ts";
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
  onChanged,
  onDeleted,
}: {
  id: string;
  summary: RoastSummary | null;
  /** The other roasts of the archive, to compare with. */
  others: RoastSummary[];
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [roast, setRoast] = useState<Roast | null>(null);
  const [weight, setWeight] = useState("");
  const [whole, setWhole] = useState("");
  const [ground, setGround] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [compareId, setCompareId] = useState<string | null>(null);
  const [compared, setCompared] = useState<Roast | null>(null);
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
    const own = buildChartData(points, base, rorStart(events, base), dropTime(events));
    const cmp = compareId && compared?.id === compareId ? comparisonCurve(compared) : null;
    return {
      data: cmp ? mergeReference(own, cmp.points, cmp.rorFrom) : own,
      markers: buildMarkers(events, base, (k) => t(EVENT_NAME[k])),
      band: developmentBand(events, base, last),
    };
  }, [roast, id, t, compareId, compared]);

  if (!roast || !view)
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

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-5">
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
        <div className="flex flex-wrap gap-2">
          {others.length > 0 && (
            <Select
              value={compareId ?? NO_COMPARE}
              onValueChange={(v) => setCompareId(v === NO_COMPARE ? null : v)}
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
          )}
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
      <div className="h-80 min-h-0 lg:h-auto lg:min-h-64 lg:flex-1">
        <RoastChart
          data={view.data}
          markers={view.markers}
          band={view.band}
          actuators
          refName={other ? (other.batch_no ?? t("Comparison")) : undefined}
          label={t("Roast curve of {{title}}", { title: roast.title || id })}
        />
      </div>
      <div className="flex flex-wrap items-start justify-between gap-x-8 gap-y-4 border-t border-border pt-4">
        <div className="flex flex-wrap gap-x-8 gap-y-3">
          <Metric
            label={t("Duration")}
            value={formatClock(summary?.total_s ?? null)}
            delta={other && signedClock(diff(summary?.total_s, other.total_s))}
          />
          <Metric
            label={t("First crack")}
            value={formatClock(summary?.first_crack_s ?? null)}
            delta={other && signedClock(diff(summary?.first_crack_s, other.first_crack_s))}
          />
          <Metric
            label={t("Drop")}
            value={`${formatNumber(summary?.drop_bt, 0)}°`}
            delta={other && signed(diff(summary?.drop_bt, other.drop_bt), 0)}
          />
          <Metric
            label={t("Development")}
            value={`${formatNumber(summary?.dtr_pct)} %`}
            delta={other && signed(diff(summary?.dtr_pct, other.dtr_pct))}
          />
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
    return () => {
      alive = false;
    };
  }, [tick]);

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
          <Button variant="ghost" size="sm" onClick={() => picker.current?.click()}>
            <Upload />
            {t("Import")}
          </Button>
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
        <Detail
          key={sel}
          id={sel}
          summary={list?.find((r) => r.id === sel) ?? null}
          others={(list ?? []).filter((r) => r.id !== sel)}
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
