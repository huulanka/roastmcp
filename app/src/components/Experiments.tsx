import { useEffect, useMemo, useState } from "react";
import i18n from "i18next";
import { ArrowRight, Check, Ellipsis, Plus, Trash2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type {
  Cupping,
  Experiment,
  ProfileSummary,
  RestEntry,
  RoastFigures,
  RoastSummary,
} from "@/api/types.ts";
import { Field, inputCls } from "@/components/Dialogs.tsx";
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
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
import {
  changeDescription,
  changeLine,
  roastsText,
  statusText,
  variableOf,
} from "@/lib/experiment.ts";
import { bestByRoast, scoreText } from "@/lib/tasting.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function shortDate(iso: string): string {
  return new Date(iso).toLocaleDateString(i18n.language, { day: "numeric", month: "short" });
}

const STATUS_CHIP: Record<Experiment["status"], string> = {
  planned: "border border-border-strong text-muted-foreground",
  running: "bg-brand/12 text-brand-text",
  done: "bg-ok/15 text-foreground",
};

export function StatusChip({ e, large = false }: { e: Experiment; large?: boolean }) {
  const { t } = useTranslation();
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center rounded-pill px-2.5 whitespace-nowrap",
        large ? "h-8 text-[14px]" : "h-6 text-[13px]",
        STATUS_CHIP[e.status],
      )}
    >
      {large && e.status === "running"
        ? t("running since {{date}}", { date: shortDate(e.created_at) })
        : statusText(e, t)}
    </span>
  );
}

/** The experiments, running ones first. */
export function ExperimentList({
  list,
  sel,
  onSelect,
}: {
  list: Experiment[];
  sel: string | null;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  if (list.length === 0) {
    return (
      <p className="text-[15px] text-muted-foreground">
        {t("No experiments yet. One hypothesis, one change, then roast and taste.")}
      </p>
    );
  }
  return (
    <ul className="-mx-2 flex min-h-0 flex-col overflow-y-auto">
      {list.map((e) => (
        <li key={e.id} className="border-b border-border last:border-b-0">
          <button
            type="button"
            onClick={() => onSelect(e.id)}
            aria-current={sel === e.id ? "true" : undefined}
            className={cn(
              "flex w-full cursor-pointer flex-col gap-1 rounded-xl px-3 py-3 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
              sel === e.id ? "bg-surface" : "hover:bg-surface",
            )}
          >
            <span className="flex items-start justify-between gap-2">
              <span className="line-clamp-2 text-[15px] font-medium">{e.hypothesis}</span>
              <StatusChip e={e} />
            </span>
            <span className="truncate text-[13px] text-muted-foreground">
              {changeLine(e)} · {roastsText(e, t)}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

const COLS =
  "grid grid-cols-[minmax(0,1fr)_4.5rem_4.5rem_5rem_4.5rem_5.5rem_2.75rem] items-center gap-x-4";

/** One experiment: the hypothesis, the change, its roasts and the conclusion. */
export function ExperimentDetail({
  experiment,
  onChanged,
  onDeleted,
}: {
  experiment: Experiment;
  onChanged: (e: Experiment) => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const e = experiment;
  const [roasts, setRoasts] = useState<RoastSummary[]>([]);
  const [figures, setFigures] = useState<Map<string, RoastFigures>>(new Map());
  const [cuppings, setCuppings] = useState<Cupping[]>([]);
  const [rest, setRest] = useState<Map<string, RestEntry>>(new Map());
  const [profile, setProfile] = useState<string | null>(null);
  const [conclusion, setConclusion] = useState(e.conclusion ?? "");
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const ids = e.roast_ids.join(",");

  useEffect(() => {
    let alive = true;
    api
      .roasts()
      .then((r) => alive && setRoasts(r))
      .catch(() => undefined);
    api
      .allCuppings()
      .then((c) => alive && setCuppings(c))
      .catch(() => undefined);
    api
      .rest()
      .then((r) => alive && setRest(new Map(r.map((x) => [x.roast_id, x]))))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    // Figures of roasts taken out stay in the map unused; only those in the list are read.
    if (!ids) return;
    let alive = true;
    api
      .consistency({ ids: ids.split(",") }, true)
      .then((c) => alive && setFigures(new Map(c.roasts.map((r) => [r.id, r.figures]))))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [ids]);

  useEffect(() => {
    // The detail is keyed by experiment, whose profile does not change.
    if (!e.profile_version_id) return;
    let alive = true;
    api
      .profileVersion(e.profile_version_id)
      .then(({ profile: p, version }) => {
        if (alive) setProfile(`${p.name} · v${version.version}`);
      })
      .catch(() => alive && setProfile(null));
    return () => {
      alive = false;
    };
  }, [e.profile_version_id]);

  const best = useMemo(() => bestByRoast(cuppings), [cuppings]);
  const mine = roasts
    .filter((r) => e.roast_ids.includes(r.id))
    .sort((a, b) => a.roasted_at.localeCompare(b.roasted_at));
  const addable = roasts.filter((r) => !e.roast_ids.includes(r.id)).slice(0, 40);
  const v = e.changed_variable.structured;

  const patch = async (body: Parameters<typeof api.patchExperiment>[1], done?: string) => {
    setBusy(true);
    try {
      onChanged(await api.patchExperiment(e.id, body));
      if (done) toast("success", done);
    } catch (err) {
      toast("critical", errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-5">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className="text-[13px] text-muted-foreground">{t("Hypothesis")}</span>
          <h2 className="font-display text-[32px] leading-tight">{e.hypothesis}</h2>
        </div>
        <span className="flex shrink-0 items-center gap-1">
          <StatusChip e={e} large />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label={t("More")}>
                <Ellipsis />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {e.status === "done" && (
                <DropdownMenuItem onSelect={() => void patch({ status: "running" })}>
                  {t("Reopen")}
                </DropdownMenuItem>
              )}
              <DropdownMenuItem danger onSelect={() => setConfirmDelete(true)}>
                <Trash2 />
                {t("Delete…")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </span>
      </div>

      <div className="grid grid-cols-3 gap-5">
        <div className="flex flex-col gap-1.5 rounded-card border border-card-border bg-surface p-card">
          <span className="text-[13px] text-muted-foreground">{t("One change")}</span>
          {v ? (
            <>
              <span className="font-semibold">{v.name}</span>
              <span className="flex items-center gap-2 tabular">
                <span className="text-muted-foreground">{v.from}</span>
                <ArrowRight aria-hidden="true" className="size-4 text-muted-foreground" />
                <span className="font-semibold text-brand-text">{v.to}</span>
              </span>
            </>
          ) : (
            <span>{e.changed_variable.description}</span>
          )}
        </div>
        <div className="flex flex-col gap-1.5 rounded-card border border-card-border bg-surface p-card">
          <span className="text-[13px] text-muted-foreground">{t("Expected")}</span>
          <span>{e.expected_outcome || "–"}</span>
        </div>
        <div className="flex flex-col gap-1.5 rounded-card border border-card-border bg-surface p-card">
          <span className="text-[13px] text-muted-foreground">{t("Profile")}</span>
          <span className="font-semibold">{profile ?? "–"}</span>
          {profile && (
            <span className="text-[13px] text-muted-foreground">
              {t("everything else unchanged")}
            </span>
          )}
        </div>
      </div>

      <section className="flex min-h-0 flex-col" aria-label={t("Roasts for it")}>
        <div className="flex items-center justify-between gap-3 pb-2.5">
          <h3 className="text-[15px] font-medium text-muted-foreground">{t("Roasts for it")}</h3>
          <Select
            value=""
            onValueChange={(id) => void patch({ add_roasts: [id] })}
            disabled={busy || addable.length === 0}
          >
            <SelectTrigger aria-label={t("Add a roast")} className="w-56">
              <span className="flex items-center gap-2">
                <Plus className="size-4" aria-hidden="true" />
                {t("Add a roast")}
              </span>
            </SelectTrigger>
            <SelectContent>
              {addable.map((r) => (
                <SelectItem key={r.id} value={r.id}>
                  <span className="flex min-w-0 items-baseline justify-between gap-4">
                    <span className="truncate">
                      {[r.batch_no, r.title || r.id].filter(Boolean).join(" · ")}
                    </span>
                    <span className="shrink-0 tabular text-[13px] text-muted-foreground">
                      {shortDate(r.roasted_at)}
                    </span>
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        {mine.length === 0 ? (
          <p className="border-t border-border py-4 text-[15px] text-muted-foreground">
            {t("No roast yet. Roast with the change, then add the roast here.")}
          </p>
        ) : (
          <div className="flex min-h-0 flex-col overflow-y-auto">
            <div
              className={`${COLS} border-t border-border py-2 text-[13px] text-muted-foreground`}
            >
              <span>{t("Roast")}</span>
              <span className="text-right">{t("Charge")}</span>
              <span className="text-right">{t("Dry end")}</span>
              <span className="text-right">{t("First crack")}</span>
              <span className="text-right">{t("DTR")}</span>
              <span className="text-right">{t("Tasting")}</span>
              <span />
            </div>
            {mine.map((r) => {
              const f = figures.get(r.id);
              const c = best.get(r.id);
              const resting = rest.get(r.id)?.state === "resting";
              return (
                <div
                  key={r.id}
                  className={`${COLS} border-t border-border py-1.5 tabular text-[15px]`}
                >
                  <span className="truncate">
                    {[r.batch_no ?? r.title, shortDate(r.roasted_at)].filter(Boolean).join(" · ")}
                  </span>
                  <span className="text-right">
                    {typeof f?.charge_bt_c === "number"
                      ? `${formatNumber(f.charge_bt_c, 0)}°`
                      : "–"}
                  </span>
                  <span className="text-right">{formatClock(f?.dry_end_s ?? null)}</span>
                  <span className="text-right">{formatClock(f?.first_crack_s ?? null)}</span>
                  <span className="text-right">
                    {typeof f?.dtr_pct === "number" ? `${formatNumber(f.dtr_pct)} %` : "–"}
                  </span>
                  <span className={cn("truncate text-right", !c && "text-muted-foreground")}>
                    {c ? scoreText(c, t, formatNumber) : resting ? t("resting") : "–"}
                  </span>
                  <Button
                    variant="ghost"
                    size="icon"
                    disabled={busy}
                    aria-label={t("Take {{roast}} out of the experiment", {
                      roast: r.batch_no ?? r.title,
                    })}
                    onClick={() => void patch({ remove_roasts: [r.id] })}
                  >
                    <X />
                  </Button>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <div className="flex-1" />

      <form
        className="flex items-end gap-3 border-t border-border pt-4"
        onSubmit={(ev) => {
          ev.preventDefault();
          const text = conclusion.trim();
          if (!text) return;
          void patch(
            { conclusion: text, ...(e.status !== "done" ? { status: "done" as const } : {}) },
            e.status === "done" ? t("Saved") : t("Experiment completed"),
          );
        }}
      >
        <label className="flex flex-1 flex-col gap-2 text-[13px] text-muted-foreground">
          {t("Result")}
          <input
            className={inputCls}
            value={conclusion}
            placeholder={t("What came out of it?")}
            onChange={(ev) => setConclusion(ev.target.value)}
          />
        </label>
        <Button type="submit" disabled={busy || !conclusion.trim()}>
          <Check />
          {e.status === "done" ? t("Save") : t("Complete")}
        </Button>
      </form>

      <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("Delete experiment?")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("Its roasts stay in the archive; only the experiment goes.")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() =>
                void api
                  .deleteExperiment(e.id)
                  .then(onDeleted)
                  .catch((err) => toast("critical", errText(err)))
              }
            >
              {t("Delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}

const NO_PROFILE = "none";

/** A new experiment: what you think, the one thing you change, what you expect. */
export function NewExperimentDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onCreated: (e: Experiment) => void;
}) {
  const { t } = useTranslation();
  const [hypothesis, setHypothesis] = useState("");
  const [name, setName] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [description, setDescription] = useState("");
  const [expected, setExpected] = useState("");
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [version, setVersion] = useState("");
  const [busy, setBusy] = useState(false);

  // Rendered only while open, so every opening starts empty.
  useEffect(() => {
    let alive = true;
    api
      .profiles()
      .then((p) => alive && setProfiles(p))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  const variable = variableOf(name, from, to);
  const change = changeDescription(description, variable);
  const canSave = hypothesis.trim() !== "" && change !== "";

  const save = async () => {
    setBusy(true);
    try {
      const e = await api.addExperiment({
        hypothesis: hypothesis.trim(),
        change,
        ...(variable ? { variable } : {}),
        ...(expected.trim() ? { expected_outcome: expected.trim() } : {}),
        ...(version ? { profile_version_id: version } : {}),
      });
      onCreated(e);
      onOpenChange(false);
    } catch (err) {
      toast("critical", errText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[38rem]">
        <form
          className="flex flex-col gap-5"
          onSubmit={(ev) => {
            ev.preventDefault();
            if (canSave) void save();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("New experiment")}</DialogTitle>
            <DialogDescription>
              {t("Change one thing at a time, so the cup tells you what it did.")}
            </DialogDescription>
          </DialogHeader>
          <Field label={t("Hypothesis")}>
            <textarea
              className={cn(inputCls, "min-h-18 resize-none py-2.5")}
              value={hypothesis}
              autoFocus
              placeholder={t("Charging hotter shortens drying without scorching.")}
              onChange={(ev) => setHypothesis(ev.target.value)}
            />
          </Field>
          <div className="grid grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1fr)] gap-3">
            <Field label={t("What changes")}>
              <input
                className={inputCls}
                value={name}
                placeholder={t("Charge temperature")}
                onChange={(ev) => setName(ev.target.value)}
              />
            </Field>
            <Field label={t("From")}>
              <input
                className={inputCls}
                value={from}
                placeholder="190 °C"
                onChange={(ev) => setFrom(ev.target.value)}
              />
            </Field>
            <Field label={t("To")}>
              <input
                className={inputCls}
                value={to}
                placeholder="200 °C"
                onChange={(ev) => setTo(ev.target.value)}
              />
            </Field>
          </div>
          {!variable && (
            <Field label={t("Or in your words")}>
              <input
                className={inputCls}
                value={description}
                onChange={(ev) => setDescription(ev.target.value)}
              />
            </Field>
          )}
          <Field label={t("Expected")}>
            <input
              className={inputCls}
              value={expected}
              placeholder={t("Drying about 20 s shorter, same development.")}
              onChange={(ev) => setExpected(ev.target.value)}
            />
          </Field>
          <Field label={t("Profile")}>
            <Select
              value={version || NO_PROFILE}
              onValueChange={(v) => setVersion(v === NO_PROFILE ? "" : v)}
            >
              <SelectTrigger aria-label={t("Profile")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_PROFILE}>
                  <span className="text-muted-foreground">{t("No profile")}</span>
                </SelectItem>
                {profiles.some((p) => p.latest) && <SelectSeparator />}
                {profiles
                  .filter((p) => p.latest)
                  .map((p) => (
                    <SelectItem key={p.id} value={p.latest?.id ?? ""}>
                      {p.name} · v{p.latest?.version}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </Field>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t("Cancel")}
            </Button>
            <Button type="submit" disabled={busy || !canSave}>
              {t("Create")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
