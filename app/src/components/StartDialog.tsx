import { useEffect, useState } from "react";
import i18n from "i18next";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Lot, RoastGoal, RoastPlan, RoastSummary } from "@/api/types.ts";
import { Field, UnitInput } from "@/components/Dialogs.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Kbd } from "@/components/ui/kbd.tsx";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import {
  isComplete,
  kg,
  lastRoastOfLot,
  parseNumber,
  pickableLots,
  suggestedTargets,
} from "@/lib/plan.ts";
import { bluetoothName } from "@/lib/ports.ts";
import { formatClock, formatNumber, weightLossPct } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { applySnapshot, useLive } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

const GOALS: RoastGoal[] = ["espresso", "filter", "omni", "other"];
const GOAL_NAME: Record<RoastGoal, string> = {
  espresso: "Espresso",
  filter: "Filter",
  omni: "Omni",
  other: "Own",
};
const LAST_LOT_KEY = "roastmcp.lastLot";
/** Radix reserves the empty string; this stands for "none" in the selects. */
const NONE = "none";

function remembered(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function remember(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private window: nothing to remember */
  }
}

function shortDate(iso: string): string {
  return new Intl.DateTimeFormat(i18n.language, { day: "numeric", month: "short" }).format(
    new Date(iso),
  );
}

interface Form {
  lotId: string;
  weight: string;
  goal: RoastGoal | null;
  refId: string;
  drop: string;
  dtr: string;
}

/** Goal, plan and targets suggested by a reference roast. */
function fromReference(ref: RoastSummary | null): Pick<Form, "goal" | "refId" | "drop" | "dtr"> {
  const s = suggestedTargets(ref);
  return {
    goal: s.goal,
    refId: ref?.id ?? "",
    drop: s.drop === null ? "" : String(s.drop),
    dtr: s.dtr === null ? "" : String(s.dtr),
  };
}

/**
 * Starting a roast (ADR 0012, 0014): which machine, the bean from the
 * inventory and its amount, the goal, the plan (a reference roast) and the two
 * targets. Everything is optional; the same fields exist for the AI as the MCP
 * tool `start_recording`.
 */
export function StartDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useTranslation();
  const machine = useLive().snapshot?.machine ?? null;
  const sim = machine?.simulated ?? false;
  const [lots, setLots] = useState<Lot[]>([]);
  const [roasts, setRoasts] = useState<RoastSummary[]>([]);
  const [form, setForm] = useState<Form>({
    lotId: "",
    weight: "",
    goal: null,
    refId: "",
    drop: "",
    dtr: "",
  });
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    Promise.all([api.lots(), api.roasts()])
      .then(([l, r]) => {
        if (!alive) return;
        const pickable = pickableLots(l);
        const lastLot = remembered(LAST_LOT_KEY);
        const lotId = pickable.find((x) => x.id === lastLot)?.id ?? pickable[0]?.id ?? "";
        const newest = [...r]
          .filter((x) => sim || !x.simulated)
          .sort((a, b) => b.roasted_at.localeCompare(a.roasted_at))[0];
        setLots(pickable);
        setRoasts(r);
        const last = lastRoastOfLot(r, lotId, sim);
        // The amount of the last roast of this bean, else of the last roast.
        const grams = last?.green_weight_g ?? newest?.green_weight_g ?? null;
        setForm({
          lotId,
          weight: grams ? String(grams) : "",
          ...fromReference(last),
        });
      })
      .catch((e) => toast("warning", e instanceof ApiError ? e.message : String(e)));
    return () => {
      alive = false;
    };
  }, [open, sim]);

  const set = (patch: Partial<Form>) => setForm((f) => ({ ...f, ...patch }));
  const lot = lots.find((l) => l.id === form.lotId) ?? null;
  const lastOfLot = lastRoastOfLot(roasts, form.lotId || null, sim);
  const weight = parseNumber(form.weight);
  const references = roasts
    .filter(isComplete)
    .sort((a, b) => b.roasted_at.localeCompare(a.roasted_at))
    .slice(0, 40);

  const start = async () => {
    setBusy(true);
    try {
      const drop = parseNumber(form.drop);
      const dtr = parseNumber(form.dtr);
      const plan: RoastPlan = {
        ...(form.goal ? { goal: form.goal } : {}),
        ...(form.refId ? { reference_id: form.refId } : {}),
        ...(drop !== null ? { drop_bt_c: drop } : {}),
        ...(dtr !== null ? { dtr_pct: dtr } : {}),
      };
      applySnapshot(
        await api.startRecording({
          lot_id: form.lotId || undefined,
          green_weight_g: weight ?? undefined,
          plan: Object.keys(plan).length ? plan : undefined,
        }),
      );
      if (form.lotId) remember(LAST_LOT_KEY, form.lotId);
      onOpenChange(false);
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const port = machine?.port ?? "";
  const machineLine = machine
    ? machine.simulated
      ? machine.name.replace(/^Simulator \((.*)\)$/, "$1")
      : [
          machine.name,
          bluetoothName(port)
            ? t("{{name}} · Bluetooth direct", { name: bluetoothName(port) })
            : port,
        ]
          .filter(Boolean)
          .join(" · ")
    : "";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-0 sm:max-w-xl">
        <form
          className="flex flex-col gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            void start();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("Start recording")}</DialogTitle>
            <DialogDescription>{t("All optional. Targets are guidance only.")}</DialogDescription>
          </DialogHeader>

          {machine && (
            <div className="flex min-w-0 items-center gap-2.5 rounded-lg bg-surface-raised px-3.5 py-2.5 text-sm">
              <span
                aria-hidden="true"
                className={cn(
                  "size-2 shrink-0 rounded-pill",
                  machine.simulated ? "bg-muted-foreground" : "bg-ok",
                )}
              />
              <span className="shrink-0 font-semibold">
                {machine.simulated ? t("Simulator") : t("Real roaster")}
              </span>
              <span className="truncate text-muted-foreground">{machineLine}</span>
            </div>
          )}

          <div className="flex flex-col gap-2">
            <div className="grid grid-cols-[minmax(0,1fr)_8.5rem] gap-3">
              <Field label={t("Bean")}>
                <Select
                  value={form.lotId || NONE}
                  onValueChange={(v) => {
                    const id = v === NONE ? "" : v;
                    const last = lastRoastOfLot(roasts, id || null, sim);
                    set({
                      lotId: id,
                      ...(last?.green_weight_g ? { weight: String(last.green_weight_g) } : {}),
                      ...fromReference(last),
                    });
                  }}
                >
                  <SelectTrigger aria-label={t("Bean")}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {lots.map((l) => (
                      <SelectItem key={l.id} value={l.id}>
                        <span className="flex min-w-0 items-baseline justify-between gap-4">
                          <span className="truncate">{l.name}</span>
                          <span className="shrink-0 tabular text-[13px] text-muted-foreground">
                            {kg(l.stock_g)}
                          </span>
                        </span>
                      </SelectItem>
                    ))}
                    {lots.length > 0 && <SelectSeparator />}
                    <SelectItem value={NONE}>
                      <span className="text-muted-foreground">
                        {t("Without a bean from the inventory")}
                      </span>
                    </SelectItem>
                  </SelectContent>
                </Select>
              </Field>
              <Field label={t("Green weight")}>
                <UnitInput
                  unit="g"
                  inputMode="decimal"
                  value={form.weight}
                  onChange={(e) => set({ weight: e.target.value })}
                />
              </Field>
            </div>
            <p className="min-h-5 text-[13px] text-muted-foreground">
              {lots.length === 0
                ? t("No beans in stock yet: record a purchase in Inventory.")
                : lot && lastOfLot
                  ? t("Last on {{date}}: {{time}} min, drop {{drop}} °C{{loss}}.", {
                      date: shortDate(lastOfLot.roasted_at),
                      time: formatClock(lastOfLot.total_s),
                      drop: formatNumber(lastOfLot.drop_bt, 1),
                      loss:
                        lastOfLot.green_weight_g && lastOfLot.roasted_weight_g
                          ? `, −${formatNumber(weightLossPct(lastOfLot.green_weight_g, lastOfLot.roasted_weight_g), 1)} %`
                          : "",
                    })
                  : null}
              {lot && weight !== null && (
                <>
                  {" "}
                  {lot.stock_g < weight
                    ? t("Only {{stock}} in stock.", { stock: kg(lot.stock_g) })
                    : t("Afterwards {{stock}} in stock.", { stock: kg(lot.stock_g - weight) })}
                </>
              )}
            </p>
          </div>

          <Field label={t("Goal")}>
            <div
              role="radiogroup"
              aria-label={t("Goal")}
              className="grid grid-cols-4 gap-1 rounded-lg bg-segment-track p-1"
            >
              {GOALS.map((g) => (
                <button
                  key={g}
                  type="button"
                  role="radio"
                  aria-checked={form.goal === g}
                  onClick={() => set({ goal: form.goal === g ? null : g })}
                  className={cn(
                    "h-9 cursor-pointer rounded-md text-[15px] transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                    form.goal === g
                      ? "bg-surface-selected font-medium text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {t(GOAL_NAME[g])}
                </button>
              ))}
            </div>
          </Field>

          <Field label={t("Plan")}>
            <Select
              value={form.refId || NONE}
              onValueChange={(v) => {
                const id = v === NONE ? "" : v;
                const s = fromReference(roasts.find((r) => r.id === id) ?? null);
                set({ refId: id, drop: s.drop, dtr: s.dtr, goal: s.goal ?? form.goal });
              }}
            >
              <SelectTrigger aria-label={t("Plan")} className="py-2">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>
                  <span className="text-muted-foreground">{t("No plan")}</span>
                </SelectItem>
                <SelectSeparator />
                {references.map((r) => (
                  <SelectItem key={r.id} value={r.id}>
                    <PlanOption roast={r} />
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>

          <div className="grid grid-cols-2 gap-3">
            <Field label={t("Drop at")}>
              <UnitInput
                unit="°C"
                inputMode="decimal"
                value={form.drop}
                onChange={(e) => set({ drop: e.target.value })}
              />
            </Field>
            <Field label={t("Development (DTR)")}>
              <UnitInput
                unit="%"
                inputMode="decimal"
                value={form.dtr}
                onChange={(e) => set({ dtr: e.target.value })}
              />
            </Field>
          </div>

          <DialogFooter className="pt-1">
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t("Cancel")}
            </Button>
            <Button type="submit" disabled={busy}>
              {t("Start recording")}
              <Kbd>↵</Kbd>
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** A reference roast as a plan: its curve shape, date and bean, then its numbers. */
function PlanOption({ roast }: { roast: RoastSummary }) {
  const { t } = useTranslation();
  const facts = [
    typeof roast.total_s === "number" ? `${formatClock(roast.total_s)} min` : null,
    typeof roast.drop_bt === "number" ? `Drop ${formatNumber(roast.drop_bt, 1)} °C` : null,
    typeof roast.dtr_pct === "number" ? `DTR ${formatNumber(roast.dtr_pct, 0)} %` : null,
    roast.simulated ? t("Simulator") : null,
  ].filter(Boolean);
  return (
    <span className="flex min-w-0 items-center gap-3">
      <svg aria-hidden="true" viewBox="0 0 40 20" className="h-5 w-10 shrink-0">
        <path
          d="M1 13 C6 18 9 18 13 15 S26 7 39 2"
          fill="none"
          stroke="var(--chart-reference)"
          strokeWidth="1.75"
          strokeDasharray="4 3"
        />
      </svg>
      <span className="flex min-w-0 flex-col">
        <span className="truncate">
          {shortDate(roast.roasted_at)} · {roast.beans?.split("\n")[0] || roast.title}
        </span>
        <span className="truncate text-[13px] text-muted-foreground">{facts.join(" · ")}</span>
      </span>
    </span>
  );
}
