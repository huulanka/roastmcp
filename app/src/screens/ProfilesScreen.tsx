import { useEffect, useMemo, useState } from "react";
import { Plus, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type {
  Actuator,
  PlannedAction,
  ProfileDetail,
  ProfileSummary,
  RoastSummary,
} from "@/api/types.ts";
import { Field, inputCls, UnitInput } from "@/components/Dialogs.tsx";
import { RoastChart } from "@/components/RoastChart.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select.tsx";
import type { ChartData, ChartMarker } from "@/lib/chartData.ts";
import { isComplete, parseNumber } from "@/lib/plan.ts";
import {
  type CurvePoint,
  type Milestones,
  TEMPLATE_MILESTONES,
  fromRoast,
  fromTarget,
  milestonesProblem,
  phaseShares,
  recipeFromRoast,
  rorOf,
  sortActions,
  templateCurve,
  toTarget,
  turningPoint,
  warp,
} from "@/lib/profile.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

const TEMPLATE = "template";

/** The bean's name without Artisan's offer-sheet tail ("… PROCESS: Natural ALTITUDE: …"). */
function shortBeans(beans: string | null | undefined): string {
  const name = (beans ?? "").split(/\n| PROCESS:/)[0].trim();
  return name.length > 36 ? `${name.slice(0, 35)}…` : name;
}
const ACTUATORS: [Actuator, string][] = [
  ["burner", "Burner"],
  ["air", "Air"],
  ["drum", "Drum"],
];

/** "4:40" or "280" → seconds; null when it is not a time. */
function parseClock(text: string): number | null {
  const m = text.trim().match(/^(\d{1,2}):(\d{1,2})$/);
  if (m) return Number(m[1]) * 60 + Number(m[2]);
  const n = parseNumber(text);
  return n !== null && n >= 0 ? n : null;
}

/** A roast-time field: shows m:ss, takes m:ss or seconds, commits on blur or Enter. */
function ClockInput({
  value,
  onCommit,
  label,
}: {
  value: number;
  onCommit: (s: number) => void;
  label: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft === null) return;
    const s = parseClock(draft);
    if (s !== null) onCommit(Math.round(s));
    setDraft(null);
  };
  return (
    <input
      aria-label={label}
      className={cn(inputCls, "h-10 text-right tabular")}
      inputMode="numeric"
      value={draft ?? formatClock(value)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && commit()}
    />
  );
}

/** A number field with a unit that commits on blur or Enter. */
function NumberInput({
  value,
  unit,
  onCommit,
  label,
}: {
  value: number;
  unit: string;
  onCommit: (v: number) => void;
  label: string;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = () => {
    if (draft === null) return;
    const v = parseNumber(draft);
    if (v !== null) onCommit(v);
    setDraft(null);
  };
  return (
    <UnitInput
      aria-label={label}
      unit={unit}
      inputMode="decimal"
      className="h-10"
      value={draft ?? formatNumber(value, 0)}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && commit()}
    />
  );
}

/** Burner/air value at each time of the curve, from the recipe's time steps. */
function stepped(actions: readonly PlannedAction[], a: Actuator, xs: readonly number[]) {
  const steps = actions
    .filter((s) => s.actuator === a && s.trigger.at === "time")
    .map((s) => ({ t: s.trigger.at === "time" ? s.trigger.roast_time_s : 0, v: s.value }))
    .sort((p, q) => p.t - q.t);
  return xs.map((x) => {
    let v: number | null = null;
    for (const s of steps) if (s.t <= x) v = s.v;
    return v;
  });
}

function chartOf(
  curve: CurvePoint[],
  m: Milestones,
  recipe: PlannedAction[],
  t: (k: string) => string,
) {
  const xs = curve.map((p) => p.t);
  const tp = turningPoint(curve);
  const ror = rorOf(curve).map((v, i) => (curve[i].t < tp.t + 10 ? null : v));
  const data: ChartData = {
    x: xs,
    bt: curve.map((p) => p.bt),
    ror,
    burner: stepped(recipe, "burner", xs),
    air: stepped(recipe, "air", xs),
  };
  const markers: ChartMarker[] = [
    { x: 0, label: t("Charge") },
    { x: m.dryEnd.t, label: t("Dry end") },
    { x: m.firstCrack.t, label: t("First crack") },
    { x: m.drop.t, label: t("Drop") },
  ];
  return { data, markers };
}

function NewProfileDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onCreated: (id: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const [roasts, setRoasts] = useState<RoastSummary[]>([]);
  const [name, setName] = useState("");
  const [beans, setBeans] = useState("");
  const [source, setSource] = useState(TEMPLATE);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    api
      .roasts()
      .then((r) => {
        if (!alive) return;
        const real = r.filter((x) => !x.simulated && isComplete(x));
        setRoasts(real);
        setSource(real[0]?.id ?? TEMPLATE);
        setName("");
        setBeans("");
      })
      .catch((e) => toast("critical", errText(e)));
    return () => {
      alive = false;
    };
  }, [open]);

  const label = (r: RoastSummary) =>
    [
      r.batch_no,
      shortBeans(r.beans) || r.title,
      new Date(r.roasted_at).toLocaleDateString(i18n.language, { day: "numeric", month: "short" }),
    ]
      .filter(Boolean)
      .join(" · ");

  const create = async () => {
    setBusy(true);
    try {
      let curve: CurvePoint[];
      let milestones: Milestones;
      let recipe: PlannedAction[];
      let note: string;
      let grams: number | undefined;
      const from = roasts.find((r) => r.id === source);
      if (from) {
        const roast = await api.roast(from.id);
        const shaped = fromRoast(roast);
        if (!shaped) throw new Error(t("This roast has no CHARGE to start from."));
        ({ curve, milestones } = shaped);
        recipe = recipeFromRoast(roast);
        note = t("From {{roast}}", {
          roast:
            from.batch_no ??
            new Date(from.roasted_at).toLocaleDateString(i18n.language, {
              day: "numeric",
              month: "short",
            }),
        });
        grams = from.green_weight_g ?? undefined;
      } else {
        milestones = TEMPLATE_MILESTONES;
        curve = templateCurve(milestones);
        recipe = [
          { trigger: { at: "time", roast_time_s: 0 }, actuator: "burner", value: 80 },
          { trigger: { at: "time", roast_time_s: 0 }, actuator: "air", value: 20 },
        ];
        note = t("From the template");
        grams = 200;
      }
      const created = await api.addProfile({
        name: name.trim(),
        intended_beans: beans.trim() || undefined,
        batch_size_g: grams,
        target: toTarget(curve, milestones, recipe),
        change_note: note,
      });
      onCreated(created.id);
      onOpenChange(false);
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[34rem]">
        <form
          className="flex flex-col gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (name.trim()) void create();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("New profile")}</DialogTitle>
            <DialogDescription>
              {t(
                "Start from a roast you liked; its curve and its burner and air steps become version 1.",
              )}
            </DialogDescription>
          </DialogHeader>
          <Field label={t("Name")}>
            <input
              className={inputCls}
              value={name}
              autoFocus
              placeholder={t("e.g. Brazil espresso")}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label={t("Start from")}>
            <Select value={source} onValueChange={setSource}>
              <SelectTrigger aria-label={t("Start from")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {roasts.slice(0, 40).map((r) => (
                  <SelectItem key={r.id} value={r.id}>
                    {label(r)}
                  </SelectItem>
                ))}
                {roasts.length > 0 && <SelectSeparator />}
                <SelectItem value={TEMPLATE}>
                  <span className="text-muted-foreground">
                    {t("Template: 200 g espresso on the Cubean+")}
                  </span>
                </SelectItem>
              </SelectContent>
            </Select>
          </Field>
          <Field label={t("For which beans (optional)")}>
            <input
              className={inputCls}
              value={beans}
              placeholder={t("e.g. Brazil naturals")}
              onChange={(e) => setBeans(e.target.value)}
            />
          </Field>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t("Cancel")}
            </Button>
            <Button type="submit" disabled={busy || !name.trim()}>
              {t("Create")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

function Editor({ id, onChanged }: { id: string; onChanged: () => void }) {
  const { t } = useTranslation();
  const [detail, setDetail] = useState<ProfileDetail | null>(null);
  const [versionNo, setVersionNo] = useState<number | null>(null);
  const [milestones, setMilestones] = useState<Milestones | null>(null);
  const [recipe, setRecipe] = useState<PlannedAction[]>([]);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  const load = (d: ProfileDetail, no: number | null) => {
    const v = d.versions.find((x) => x.version === no) ?? d.versions[0];
    setDetail(d);
    setVersionNo(v?.version ?? null);
    if (v) {
      setMilestones(fromTarget(v.target).milestones);
      setRecipe(sortActions(v.target.actions));
    }
    setNote("");
  };

  useEffect(() => {
    let alive = true;
    api
      .profile(id)
      .then((d) => alive && load(d, null))
      .catch((e) => toast("critical", errText(e)));
    return () => {
      alive = false;
    };
  }, [id]);

  const version = detail?.versions.find((v) => v.version === versionNo) ?? null;
  const base = useMemo(() => (version ? fromTarget(version.target) : null), [version]);
  const problem = milestones ? milestonesProblem(milestones) : null;
  const curve = useMemo(() => {
    if (!base || !milestones) return [];
    return problem ? base.curve : warp(base.curve, base.milestones, milestones);
  }, [base, milestones, problem]);
  const chart = useMemo(
    () => (milestones && curve.length ? chartOf(curve, milestones, recipe, t) : null),
    [curve, milestones, recipe, t],
  );
  const dirty =
    !!version &&
    !!milestones &&
    (JSON.stringify(milestones) !== JSON.stringify(base?.milestones) ||
      JSON.stringify(recipe) !== JSON.stringify(sortActions(version.target.actions)));

  if (!detail || !version || !milestones || !chart)
    return <p className="p-card text-[15px] text-muted-foreground">{t("Loading…")}</p>;

  const newest = detail.versions[0]?.version ?? 1;
  const shares = phaseShares(milestones);
  const setM = (patch: Partial<Milestones>) => setMilestones({ ...milestones, ...patch });
  const setStep = (i: number, patch: Partial<PlannedAction>) =>
    setRecipe(recipe.map((s, j) => (j === i ? { ...s, ...patch } : s)));
  const save = async () => {
    setBusy(true);
    try {
      const d = await api.addProfileVersion(id, {
        target: toTarget(curve, milestones, recipe),
        change_note: note.trim() || undefined,
      });
      load(d, d.versions[0]?.version ?? null);
      onChanged();
      toast("success", t("Saved as version {{n}}", { n: d.versions[0]?.version ?? "" }));
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  const rows: [string, keyof Milestones][] = [
    ["Dry end", "dryEnd"],
    ["First crack", "firstCrack"],
    ["Drop", "drop"],
  ];

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-4">
      <div className="flex items-end justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <div className="flex items-center gap-3">
            <h2 className="truncate text-xl font-semibold">{detail.name}</h2>
            <Select value={String(version.version)} onValueChange={(v) => load(detail, Number(v))}>
              <SelectTrigger aria-label={t("Version")} className="h-9 w-auto shrink-0 gap-2 py-1">
                <SelectValue>v{version.version}</SelectValue>
              </SelectTrigger>
              <SelectContent className="max-w-96">
                {detail.versions.map((v) => (
                  <SelectItem key={v.id} value={String(v.version)}>
                    <span className="truncate">
                      v{v.version}
                      {v.change_note ? ` · ${v.change_note}` : ""}
                    </span>
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <span className="truncate text-[13px] text-muted-foreground">
            {[
              detail.intended_beans,
              detail.batch_size_g ? `${formatNumber(detail.batch_size_g, 0)} g` : null,
              version.change_note,
            ]
              .filter(Boolean)
              .join(" · ")}
          </span>
        </div>
        <form
          className="flex items-center gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (dirty && !problem) void save();
          }}
        >
          <input
            className={cn(inputCls, "w-56")}
            value={note}
            placeholder={t("What did you change?")}
            disabled={!dirty}
            onChange={(e) => setNote(e.target.value)}
          />
          <Button type="submit" disabled={!dirty || !!problem || busy}>
            {t("Save as v{{n}}", { n: newest + 1 })}
          </Button>
        </form>
      </div>

      <div className="h-72 min-h-0 lg:h-auto lg:min-h-48 lg:flex-1">
        <RoastChart
          data={chart.data}
          markers={chart.markers}
          minSpanS={Math.max(720, milestones.drop.t + 30)}
          actuators
          label={t("Target curve of {{name}}", { name: detail.name })}
        />
      </div>

      <div className="grid shrink-0 grid-cols-1 gap-6 border-t border-border pt-3 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)]">
        <section aria-label={t("Milestones")} className="flex flex-col gap-2">
          <h3 className="text-[13px] font-medium text-muted-foreground">{t("Milestones")}</h3>
          <div className="grid grid-cols-[6.5rem_6rem_7rem] items-center gap-x-3 gap-y-2 text-[15px]">
            <span>{t("Charge")}</span>
            <span className="text-right tabular text-muted-foreground">0:00</span>
            <NumberInput
              label={t("Charge temperature")}
              unit="°C"
              value={milestones.charge}
              onCommit={(v) => setM({ charge: v })}
            />
            {rows.map(([name, key]) => {
              const p = milestones[key] as CurvePoint;
              return (
                <Row key={key}>
                  <span>{t(name)}</span>
                  <ClockInput
                    label={t("{{name}} time", { name: t(name) })}
                    value={p.t}
                    onCommit={(s) => setM({ [key]: { ...p, t: s } })}
                  />
                  <NumberInput
                    label={t("{{name}} temperature", { name: t(name) })}
                    unit="°C"
                    value={p.bt}
                    onCommit={(v) => setM({ [key]: { ...p, bt: v } })}
                  />
                </Row>
              );
            })}
          </div>
          <p
            className={cn(
              "tabular text-[13px]",
              problem ? "text-danger-text" : "text-muted-foreground",
            )}
          >
            {problem
              ? t(problem)
              : t("Drying {{d}} % · Maillard {{m}} % · Development {{x}} % · {{total}} min", {
                  d: formatNumber(shares.drying, 0),
                  m: formatNumber(shares.maillard, 0),
                  x: formatNumber(shares.development, 0),
                  total: formatClock(milestones.drop.t),
                })}
          </p>
        </section>

        <section aria-label={t("Recipe")} className="flex min-h-0 flex-col gap-2">
          <div className="flex items-center justify-between">
            <h3 className="text-[13px] font-medium text-muted-foreground">{t("Recipe")}</h3>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                const last = recipe.at(-1);
                const at = last && last.trigger.at === "time" ? last.trigger.roast_time_s + 60 : 0;
                setRecipe([
                  ...recipe,
                  {
                    trigger: { at: "time", roast_time_s: Math.min(at, milestones.drop.t) },
                    actuator: "burner",
                    value: last?.actuator === "burner" ? last.value : 75,
                  },
                ]);
              }}
            >
              <Plus />
              {t("Step")}
            </Button>
          </div>
          <ol className="flex max-h-[12.5rem] min-h-0 flex-col gap-2 overflow-y-auto pr-1">
            {recipe.map((s, i) => (
              <li
                key={i}
                className="grid grid-cols-[5.5rem_minmax(0,9rem)_6.5rem_auto] items-center gap-2"
              >
                {s.trigger.at === "time" ? (
                  <ClockInput
                    label={t("Step time")}
                    value={s.trigger.roast_time_s}
                    onCommit={(v) =>
                      setRecipe(
                        sortActions(
                          recipe.map((x, j) =>
                            j === i ? { ...x, trigger: { at: "time", roast_time_s: v } } : x,
                          ),
                        ),
                      )
                    }
                  />
                ) : (
                  <span className="text-right tabular text-[15px]">
                    {formatNumber(s.trigger.bt_c, 0)} °C
                  </span>
                )}
                <Select
                  value={s.actuator}
                  onValueChange={(v) => setStep(i, { actuator: v as Actuator })}
                >
                  <SelectTrigger aria-label={t("Actuator")} className="h-10 py-1">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {ACTUATORS.map(([a, name]) => (
                      <SelectItem key={a} value={a}>
                        {t(name)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <NumberInput
                  label={t("Step value")}
                  unit="%"
                  value={s.value}
                  onCommit={(v) => setStep(i, { value: Math.max(0, Math.min(100, Math.round(v))) })}
                />
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={t("Remove step")}
                  onClick={() => setRecipe(recipe.filter((_, j) => j !== i))}
                >
                  <X />
                </Button>
              </li>
            ))}
            {recipe.length === 0 && (
              <li className="text-[15px] text-muted-foreground">{t("No steps yet.")}</li>
            )}
          </ol>
        </section>
      </div>
    </div>
  );
}

/** Three grid cells as one row (keeps the milestones grid flat). */
function Row({ children }: { children: React.ReactNode }) {
  return <>{children}</>;
}

/** The Profiles screen (its own tab): profiles with versions, milestones and recipe. */
export function ProfilesScreen() {
  const { t } = useTranslation();
  const [list, setList] = useState<ProfileSummary[] | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [tick, setTick] = useState(0);
  const [newOpen, setNewOpen] = useState(false);

  useEffect(() => {
    let alive = true;
    api
      .profiles()
      .then((p) => {
        if (!alive) return;
        setList(p);
        setSel((s) => s ?? p[0]?.id ?? null);
      })
      .catch((e) => alive && (setList([]), toast("critical", errText(e))));
    return () => {
      alive = false;
    };
  }, [tick]);

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-section overflow-y-auto p-gutter lg:flex-row lg:overflow-hidden">
      <section
        aria-label={t("Profiles")}
        className="flex min-h-0 flex-col gap-3 lg:w-[20rem] lg:shrink-0"
      >
        <div className="flex items-center justify-between gap-3">
          <h2 className="text-[15px] font-medium text-muted-foreground">{t("Profiles")}</h2>
          <Button onClick={() => setNewOpen(true)}>{t("New profile")}</Button>
        </div>
        {list === null ? (
          <p className="text-[15px] text-muted-foreground">{t("Loading…")}</p>
        ) : list.length === 0 ? (
          <p className="text-[15px] text-muted-foreground">
            {t("No profiles yet. Start one from a roast you liked.")}
          </p>
        ) : (
          <ul className="-mx-2 flex min-h-0 flex-col overflow-y-auto">
            {list.map((p) => (
              <li key={p.id} className="border-b border-border last:border-b-0">
                <button
                  type="button"
                  onClick={() => setSel(p.id)}
                  aria-current={sel === p.id ? "true" : undefined}
                  className={cn(
                    "flex w-full cursor-pointer flex-col gap-1 rounded-xl px-3 py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                    sel === p.id ? "bg-surface" : "hover:bg-surface",
                  )}
                >
                  <span className="flex items-baseline justify-between gap-2">
                    <span className="truncate text-[15px] font-medium">{p.name}</span>
                    {p.latest && (
                      <span className="shrink-0 tabular text-[13px] text-muted-foreground">
                        v{p.latest.version}
                      </span>
                    )}
                  </span>
                  <span className="truncate text-[13px] text-muted-foreground">
                    {[p.intended_beans, p.latest?.change_note].filter(Boolean).join(" · ") || "–"}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      {sel ? (
        <Editor key={sel} id={sel} onChanged={() => setTick((n) => n + 1)} />
      ) : (
        <div className="grid flex-1 place-items-center rounded-card border border-dashed border-border-strong p-10 text-[15px] text-muted-foreground">
          {t("Select a profile, or start a new one from a roast.")}
        </div>
      )}
      <NewProfileDialog
        open={newOpen}
        onOpenChange={setNewOpen}
        onCreated={(id) => {
          setSel(id);
          setTick((n) => n + 1);
        }}
      />
    </main>
  );
}
