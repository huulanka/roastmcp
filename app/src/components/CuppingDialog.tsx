import { useEffect, useState } from "react";
import { ArrowLeft, ArrowRight, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Cupping, CuppingScores } from "@/api/types.ts";
import { Field, inputCls, UnitInput } from "@/components/Dialogs.tsx";
import { Segmented } from "@/components/TopBar.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import { Slider } from "@/components/ui/slider.tsx";
import {
  addDescriptors,
  BREWS,
  brewText,
  dateField,
  meanScore,
  parseBrew,
  rated,
  restDays,
  SCORES,
  tastedAt,
  type Brew,
} from "@/lib/tasting.ts";
import { formatNumber } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { toast } from "@/state/toasts.ts";

const NO_BREW = "none";

function parseNumber(text: string): number | null {
  const v = parseFloat(text.replace(",", "."));
  return Number.isFinite(v) ? v : null;
}

/**
 * Recording a tasting, or changing one, on two pages: the impression
 * (date, brew, total, descriptors, note), then the optional scores.
 */
export function CuppingDialog({
  roast,
  cupping,
  onClose,
  onSaved,
}: {
  roast: { id: string; label: string; roasted_at: string; lot_id?: string | null };
  /** The tasting to change; `null` records a new one. */
  cupping: Cupping | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [page, setPage] = useState<"impression" | "scores">("impression");
  const [day, setDay] = useState(dateField(cupping ? new Date(cupping.cupped_at) : new Date()));
  const [rest, setRest] = useState(cupping ? String(cupping.rest_days) : "");
  const [total, setTotal] = useState(
    cupping?.overall_score !== undefined ? String(cupping.overall_score) : "",
  );
  const parsed = parseBrew(cupping?.brew_method, t);
  const [brew, setBrew] = useState<Brew | null>(parsed.brew);
  const [detail, setDetail] = useState(parsed.detail);
  const [descriptors, setDescriptors] = useState<string[]>(cupping?.descriptors ?? []);
  const [typed, setTyped] = useState("");
  const [notes, setNotes] = useState(cupping?.notes ?? "");
  const [scores, setScores] = useState<CuppingScores>(cupping?.scores ?? {});
  const [busy, setBusy] = useState(false);
  /** The lot's note for its next roast, as stored and as being edited. */
  const [nextRoast, setNextRoast] = useState<{ stored: string; text: string } | null>(null);

  useEffect(() => {
    if (!roast.lot_id) return;
    let alive = true;
    api
      .lot(roast.lot_id)
      .then((l) => {
        const text = l.next_roast ?? "";
        if (alive) setNextRoast({ stored: text, text });
      })
      // Without the lot the field simply does not show.
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [roast.lot_id]);

  const derivedRest = day ? restDays(roast.roasted_at, tastedAt(day, roast.roasted_at)) : null;
  const totalValue = total.trim() === "" ? null : parseNumber(total);
  const restValue = rest.trim() === "" ? null : parseNumber(rest);
  const valid =
    day !== "" &&
    (total.trim() === "" || (totalValue !== null && totalValue >= 0 && totalValue <= 100)) &&
    (rest.trim() === "" || (restValue !== null && restValue >= 0 && restValue % 1 === 0));
  const mean = meanScore(scores);
  const given = rated(scores).length;

  const save = async () => {
    if (!valid) return;
    const all = addDescriptors(descriptors, typed);
    const body = {
      cupped_at: tastedAt(day, roast.roasted_at),
      ...(restValue !== null ? { rest_days: restValue } : {}),
      ...(totalValue !== null ? { overall_score: totalValue } : {}),
      scores,
      descriptors: all,
      notes: notes.trim(),
      brew_method: brewText(brew, detail, t),
    };
    setBusy(true);
    try {
      if (cupping) await api.patchCupping(cupping.id, body);
      else await api.addCupping(roast.id, body);
      if (roast.lot_id && nextRoast && nextRoast.text.trim() !== nextRoast.stored) {
        await api.patchLot(roast.lot_id, { next_roast: nextRoast.text.trim() });
      }
      toast("success", t("Tasting saved"));
      onSaved();
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const steps = (
    <ol className="grid grid-cols-2 gap-2" aria-label={t("Steps")}>
      {(["impression", "scores"] as const).map((p, i) => (
        <li key={p}>
          <button
            type="button"
            onClick={() => setPage(p)}
            aria-current={page === p ? "step" : undefined}
            className="flex w-full cursor-pointer flex-col gap-1.5 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          >
            <span
              className={cn(
                "h-1 rounded-pill",
                i === 0 || page === "scores" ? "bg-brand" : "bg-surface-raised",
              )}
            />
            <span
              className={cn(
                "truncate text-[12px]",
                page === p ? "font-medium text-foreground" : "text-muted-foreground",
              )}
            >
              {p === "impression" ? t("Impression") : t("Scores")}
            </span>
          </button>
        </li>
      ))}
    </ol>
  );

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-0 sm:max-w-[40rem]">
        <form
          className="flex flex-col gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (page === "impression") setPage("scores");
            else void save();
          }}
        >
          <DialogHeader>
            <DialogTitle>{cupping ? t("Change tasting") : t("Record tasting")}</DialogTitle>
            <DialogDescription>
              {page === "impression"
                ? t("{{roast}} · roasted on {{date}}", {
                    roast: roast.label,
                    date: new Date(roast.roasted_at).toLocaleDateString(i18n.language, {
                      dateStyle: "medium",
                    }),
                  })
                : t("Rate only what you paid attention to. 0 to 10 in half steps.")}
            </DialogDescription>
          </DialogHeader>
          {steps}

          {page === "impression" ? (
            <>
              <div className="grid grid-cols-3 gap-3">
                <Field label={t("Tasted on")}>
                  <input
                    type="date"
                    className={cn(inputCls, "tabular")}
                    value={day}
                    min={dateField(new Date(roast.roasted_at))}
                    max={dateField(new Date())}
                    onChange={(e) => setDay(e.target.value)}
                  />
                </Field>
                <Field label={t("Rest")}>
                  <UnitInput
                    unit={t("days")}
                    inputMode="numeric"
                    value={rest}
                    placeholder={derivedRest !== null ? String(derivedRest) : ""}
                    aria-invalid={rest.trim() !== "" && restValue === null}
                    onChange={(e) => setRest(e.target.value)}
                  />
                </Field>
                <Field label={t("Total")}>
                  <UnitInput
                    unit={t("points")}
                    inputMode="decimal"
                    value={total}
                    aria-invalid={total.trim() !== "" && totalValue === null}
                    onChange={(e) => setTotal(e.target.value)}
                  />
                </Field>
              </div>

              <Field label={t("Brew")}>
                <span className="flex gap-2">
                  <Segmented<Brew | typeof NO_BREW>
                    label={t("Brew")}
                    value={brew ?? NO_BREW}
                    options={BREWS.map((b) => ({ value: b.key, label: t(b.name) }))}
                    onChange={(v) => setBrew(v === brew ? null : v === NO_BREW ? null : v)}
                    className="shrink-0"
                  />
                  <input
                    className={inputCls}
                    value={detail}
                    placeholder={t("V60 · 15 g / 250 g")}
                    onChange={(e) => setDetail(e.target.value)}
                  />
                </span>
              </Field>

              <Field label={t("Flavours")}>
                <span className="flex min-h-touch flex-wrap items-center gap-1.5 rounded-lg border border-border-strong bg-background px-2 py-1.5 focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/40">
                  {descriptors.map((d) => (
                    <span
                      key={d}
                      className="inline-flex h-7 items-center gap-1 rounded-pill bg-surface-raised pr-1 pl-2.5 text-[14px] text-foreground"
                    >
                      {d}
                      <button
                        type="button"
                        aria-label={t("Remove “{{text}}”", { text: d })}
                        onClick={() => setDescriptors(descriptors.filter((x) => x !== d))}
                        className="grid size-5 cursor-pointer place-items-center rounded-pill text-muted-foreground hover:text-foreground"
                      >
                        <X className="size-3.5" aria-hidden="true" />
                      </button>
                    </span>
                  ))}
                  <input
                    className="h-7 min-w-32 flex-1 bg-transparent px-1 text-[15px] text-foreground outline-none select-text placeholder:text-muted-foreground"
                    value={typed}
                    aria-label={t("Add flavour")}
                    placeholder={t("Add flavour…")}
                    onChange={(e) => {
                      const v = e.target.value;
                      if (v.includes(",")) {
                        setDescriptors(addDescriptors(descriptors, v));
                        setTyped("");
                      } else setTyped(v);
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && typed.trim()) {
                        e.preventDefault();
                        setDescriptors(addDescriptors(descriptors, typed));
                        setTyped("");
                      } else if (e.key === "Backspace" && typed === "" && descriptors.length) {
                        setDescriptors(descriptors.slice(0, -1));
                      }
                    }}
                  />
                </span>
              </Field>

              <Field label={t("Note")}>
                <textarea
                  className={cn(
                    inputCls,
                    nextRoast ? "min-h-18" : "min-h-24",
                    "resize-none py-2.5",
                  )}
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                />
              </Field>

              {nextRoast && (
                <Field label={t("Next time with this bean")}>
                  <input
                    className={inputCls}
                    value={nextRoast.text}
                    maxLength={500}
                    placeholder={t("e.g. charge 5 °C hotter, drop a little earlier")}
                    onChange={(e) => setNextRoast({ ...nextRoast, text: e.target.value })}
                  />
                </Field>
              )}
            </>
          ) : (
            <>
              <div className="grid grid-cols-2 gap-x-9 gap-y-3">
                {SCORES.map((s) => {
                  const value = scores[s.key];
                  const set = (v: number | undefined) =>
                    setScores((now) => {
                      const next = { ...now };
                      if (v === undefined) delete next[s.key];
                      else next[s.key] = v;
                      return next;
                    });
                  return (
                    <div key={s.key} className="flex flex-col">
                      <span className="flex h-7 items-center justify-between gap-2 text-[15px]">
                        <span className={value === undefined ? "text-muted-foreground" : ""}>
                          {t(s.name)}
                        </span>
                        {value === undefined ? (
                          <span className="text-[13px] text-muted-foreground">
                            {t("not rated")}
                          </span>
                        ) : (
                          <button
                            type="button"
                            onClick={() => set(undefined)}
                            aria-label={t("Clear {{name}}", { name: t(s.name) })}
                            title={t("Not rated")}
                            className="inline-flex cursor-pointer items-center gap-1 rounded-md tabular font-semibold hover:text-muted-foreground"
                          >
                            {formatNumber(value)}
                            <X className="size-3.5 text-muted-foreground" aria-hidden="true" />
                          </button>
                        )}
                      </span>
                      <Slider
                        min={0}
                        max={10}
                        step={0.5}
                        value={[value ?? 0]}
                        aria-label={t(s.name)}
                        style={
                          value === undefined
                            ? ({ "--slider-range": "var(--border-strong)" } as React.CSSProperties)
                            : undefined
                        }
                        onValueChange={([v]) => set(v)}
                      />
                    </div>
                  );
                })}
              </div>
              <div className="flex items-center justify-between border-t border-border pt-4">
                <span className="text-[13px] text-muted-foreground">
                  {given === 0
                    ? t("Nothing rated yet. That is fine, too.")
                    : t("Mean of the {{n}} scores given", { n: given })}
                </span>
                <span className="tabular text-[17px] font-semibold">
                  {mean === null ? "–" : formatNumber(mean)}
                </span>
              </div>
            </>
          )}

          <DialogFooter className="items-center pt-1 sm:justify-between">
            <Button type="button" variant="ghost" onClick={onClose}>
              {t("Cancel")}
            </Button>
            <span className="flex gap-2">
              {page === "impression" ? (
                <>
                  <Button
                    type="button"
                    variant="secondary"
                    disabled={busy || !valid}
                    onClick={() => void save()}
                  >
                    {t("Save")}
                  </Button>
                  <Button type="submit" disabled={!valid}>
                    {t("Next: {{page}}", { page: t("Scores") })}
                    <ArrowRight />
                  </Button>
                </>
              ) : (
                <>
                  <Button type="button" variant="secondary" onClick={() => setPage("impression")}>
                    <ArrowLeft />
                    {t("Back")}
                  </Button>
                  <Button type="submit" disabled={busy || !valid}>
                    {t("Save")}
                  </Button>
                </>
              )}
            </span>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
