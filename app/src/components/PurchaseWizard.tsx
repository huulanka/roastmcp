import { useState } from "react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Lot } from "@/api/types.ts";
import { Field, inputCls, UnitInput } from "@/components/Dialogs.tsx";
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
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select.tsx";
import {
  CERTIFICATIONS,
  DECAFS,
  detailsOf,
  detailsToForm,
  DRYINGS,
  EMPTY_LOT_FORM,
  FERMENTATIONS,
  formToDetails,
  HONEY_GRADES,
  type LotForm,
  PACKAGINGS,
  PROCESSES,
  SPECIES,
} from "@/lib/lot.ts";
import { kg, parseNumber } from "@/lib/plan.ts";
import { cn } from "@/lib/utils.ts";
import { toast } from "@/state/toasts.ts";

/** What the wizard does: a new lot, more of one (restock), or edit one's details. */
export type PurchaseMode =
  { kind: "new" } | { kind: "restock"; lotId?: string } | { kind: "edit"; lot: Lot };

const STEPS = ["Purchase", "Origin", "Processing", "Quality", "Supplier & storage"];
const NONE = "none";

function today(): string {
  return new Date().toISOString().slice(0, 10);
}

/** A pill that is on or off: one of a group (radio) or several (checkbox). */
function Pill({
  on,
  onClick,
  children,
  multi = false,
}: {
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
  multi?: boolean;
}) {
  return (
    <button
      type="button"
      role={multi ? "checkbox" : "radio"}
      aria-checked={on}
      onClick={onClick}
      className={cn(
        "h-9 cursor-pointer rounded-pill border px-3.5 text-[14px] transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
        on
          ? "border-foreground bg-foreground text-background"
          : "border-border-strong text-foreground hover:border-muted-foreground",
      )}
    >
      {children}
    </button>
  );
}

/** A select over a fixed list, with "–" for none. */
function Choice<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T | "";
  options: [T, string][];
  onChange: (v: T | "") => void;
}) {
  const { t } = useTranslation();
  return (
    <Field label={label}>
      <Select value={value || NONE} onValueChange={(v) => onChange(v === NONE ? "" : (v as T))}>
        <SelectTrigger aria-label={label}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NONE}>
            <span className="text-muted-foreground">–</span>
          </SelectItem>
          {options.map(([v, name]) => (
            <SelectItem key={v} value={v}>
              {t(name)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </Field>
  );
}

/**
 * Recording a purchase (ADR 0014) as a short wizard: name and amount first, then
 * one page per group of the importer's offer sheet. Every page fits without
 * scrolling, and the lot can be saved from any page.
 */
export function PurchaseWizard({
  mode,
  lots,
  onClose,
  onSaved,
}: {
  mode: PurchaseMode;
  lots: Lot[];
  onClose: () => void;
  onSaved: (lot: Lot) => void;
}) {
  const { t } = useTranslation();
  const editing = mode.kind === "edit" ? mode.lot : null;
  const [restock, setRestock] = useState(mode.kind === "restock");
  const [lotId, setLotId] = useState(
    mode.kind === "restock" ? (mode.lotId ?? lots[0]?.id ?? "") : "",
  );
  const [name, setName] = useState(editing?.name ?? "");
  const [amount, setAmount] = useState("");
  const [date, setDate] = useState(today());
  const [note, setNote] = useState("");
  const [form, setForm] = useState<LotForm>(
    editing ? detailsToForm(detailsOf(editing)) : EMPTY_LOT_FORM,
  );
  const [step, setStep] = useState(0);
  const [busy, setBusy] = useState(false);

  const set = (patch: Partial<LotForm>) => setForm((f) => ({ ...f, ...patch }));
  const text = (key: keyof LotForm) => ({
    value: form[key] as string,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => set({ [key]: e.target.value }),
  });
  const toggle = <T,>(list: T[], v: T) =>
    list.includes(v) ? list.filter((x) => x !== v) : [...list, v];

  const kgAmount = parseNumber(amount);
  const grams = kgAmount !== null && kgAmount > 0 ? Math.round(kgAmount * 1000) : null;
  const canSave = editing
    ? name.trim() !== ""
    : restock
      ? lotId !== "" && grams !== null
      : name.trim() !== "" && grams !== null;
  const pages = restock ? 1 : STEPS.length;

  const save = async () => {
    if (!canSave) return;
    setBusy(true);
    try {
      let lot: Lot;
      if (editing) {
        lot = await api.patchLot(editing.id, { name: name.trim(), details: formToDetails(form) });
      } else if (restock) {
        lot = await api.addPurchase(lotId, {
          grams: grams ?? 0,
          date,
          note: note.trim() || undefined,
        });
      } else {
        lot = await api.addLot({
          name: name.trim(),
          purchase_g: grams ?? undefined,
          purchase_date: date,
          ...formToDetails(form),
        });
      }
      onSaved(lot);
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const title = editing
    ? t("Edit {{name}}", { name: editing.name })
    : restock
      ? t("Record a purchase")
      : t("New bean");

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="gap-0 sm:max-w-2xl">
        <form
          className="flex flex-col gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
            <DialogDescription>
              {editing || restock
                ? t("As on the importer's offer sheet.")
                : t(
                    "Only name and amount are needed; the rest as on the offer sheet, now or later.",
                  )}
            </DialogDescription>
          </DialogHeader>

          {!editing && lots.length > 0 && (
            <div
              role="radiogroup"
              aria-label={t("Purchase")}
              className="grid grid-cols-2 gap-1 rounded-lg bg-segment-track p-1"
            >
              {[false, true].map((r) => (
                <button
                  key={String(r)}
                  type="button"
                  role="radio"
                  aria-checked={restock === r}
                  onClick={() => {
                    setRestock(r);
                    setStep(0);
                    if (r && !lotId) setLotId(lots[0]?.id ?? "");
                  }}
                  className={cn(
                    "h-9 cursor-pointer rounded-md text-[15px] transition-colors outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                    restock === r
                      ? "bg-surface-selected font-medium text-foreground shadow-sm"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {r ? t("More of a bean") : t("New bean")}
                </button>
              ))}
            </div>
          )}

          {pages > 1 && (
            <ol className="grid grid-cols-5 gap-2" aria-label={t("Steps")}>
              {STEPS.map((s, i) => (
                <li key={s}>
                  <button
                    type="button"
                    onClick={() => setStep(i)}
                    aria-current={step === i ? "step" : undefined}
                    className="flex w-full cursor-pointer flex-col gap-1.5 text-left outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                  >
                    <span
                      className={cn(
                        "h-1 rounded-pill",
                        i <= step ? "bg-brand" : "bg-surface-raised",
                      )}
                    />
                    <span
                      className={cn(
                        "truncate text-[12px]",
                        step === i ? "font-medium text-foreground" : "text-muted-foreground",
                      )}
                    >
                      {t(s)}
                    </span>
                  </button>
                </li>
              ))}
            </ol>
          )}

          <div className="flex min-h-[19rem] flex-col gap-4">
            {step === 0 &&
              (restock ? (
                <>
                  <Field label={t("Bean")}>
                    <Select value={lotId} onValueChange={setLotId}>
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
                      </SelectContent>
                    </Select>
                  </Field>
                  <div className="grid grid-cols-2 gap-3">
                    <Field label={t("Amount")}>
                      <UnitInput
                        unit="kg"
                        inputMode="decimal"
                        value={amount}
                        onChange={(e) => setAmount(e.target.value)}
                      />
                    </Field>
                    <Field label={t("Bought on")}>
                      <input
                        type="date"
                        className={inputCls}
                        value={date}
                        onChange={(e) => setDate(e.target.value)}
                      />
                    </Field>
                  </div>
                  <Field label={t("Note")}>
                    <input
                      className={inputCls}
                      value={note}
                      onChange={(e) => setNote(e.target.value)}
                    />
                  </Field>
                </>
              ) : (
                <>
                  <Field label={t("Name")}>
                    <input
                      className={inputCls}
                      value={name}
                      placeholder={t("e.g. GL-BR-001 Brasilien Amarelo")}
                      onChange={(e) => setName(e.target.value)}
                    />
                  </Field>
                  {!editing && (
                    <div className="grid grid-cols-2 gap-3">
                      <Field label={t("Amount")}>
                        <UnitInput
                          unit="kg"
                          inputMode="decimal"
                          value={amount}
                          onChange={(e) => setAmount(e.target.value)}
                        />
                      </Field>
                      <Field label={t("Bought on")}>
                        <input
                          type="date"
                          className={inputCls}
                          value={date}
                          onChange={(e) => setDate(e.target.value)}
                        />
                      </Field>
                    </div>
                  )}
                  <p className="text-[13px] text-muted-foreground">
                    {editing
                      ? t("The stock changes with purchases and corrections, not here.")
                      : t(
                          "Origin, processing, quality and supplier on the next pages, all optional.",
                        )}
                  </p>
                </>
              ))}

            {step === 1 && (
              <>
                <div className="grid grid-cols-2 gap-3">
                  <Field label={t("Country")}>
                    <input className={inputCls} {...text("origin_country")} />
                  </Field>
                  <Field label={t("Region")}>
                    <input className={inputCls} placeholder={t("e.g. Huila")} {...text("region")} />
                  </Field>
                </div>
                <Field label={t("Producer, farm, cooperative or station")}>
                  <input className={inputCls} {...text("producer")} />
                </Field>
                <div className="grid grid-cols-2 gap-3">
                  <Field label={t("Altitude")}>
                    <span className="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2">
                      <UnitInput
                        unit="m"
                        inputMode="numeric"
                        aria-label={t("from")}
                        {...text("altitude_min")}
                      />
                      <span className="text-muted-foreground">–</span>
                      <UnitInput
                        unit="m"
                        inputMode="numeric"
                        aria-label={t("to")}
                        {...text("altitude_max")}
                      />
                    </span>
                  </Field>
                  <Choice
                    label={t("Species")}
                    value={form.species}
                    options={SPECIES}
                    onChange={(species) => set({ species })}
                  />
                </div>
                <Field label={t("Varieties (comma separated)")}>
                  <input
                    className={inputCls}
                    placeholder="Castillo, Caturra"
                    {...text("varieties")}
                  />
                </Field>
              </>
            )}

            {step === 2 && (
              <>
                <Field label={t("Base method")}>
                  <div
                    role="radiogroup"
                    aria-label={t("Base method")}
                    className="flex flex-wrap gap-1.5"
                  >
                    {PROCESSES.map(([p, label]) => (
                      <Pill
                        key={p}
                        on={form.process === p}
                        onClick={() => set({ process: form.process === p ? "" : p })}
                      >
                        {t(label)}
                      </Pill>
                    ))}
                  </div>
                </Field>
                {form.process === "honey" && (
                  <Field label={t("Honey grade")}>
                    <div
                      role="radiogroup"
                      aria-label={t("Honey grade")}
                      className="flex flex-wrap gap-1.5"
                    >
                      {HONEY_GRADES.map(([g, label]) => (
                        <Pill
                          key={g}
                          on={form.honey === g}
                          onClick={() => set({ honey: form.honey === g ? "" : g })}
                        >
                          {t(label)}
                        </Pill>
                      ))}
                    </div>
                  </Field>
                )}
                <Field label={t("Fermentation (several possible)")}>
                  <div
                    role="group"
                    aria-label={t("Fermentation")}
                    className="flex flex-wrap gap-1.5"
                  >
                    {FERMENTATIONS.map(([f, label]) => (
                      <Pill
                        key={f}
                        multi
                        on={form.fermentation.includes(f)}
                        onClick={() => set({ fermentation: toggle(form.fermentation, f) })}
                      >
                        {t(label)}
                      </Pill>
                    ))}
                  </div>
                </Field>
                <div className="grid grid-cols-3 gap-3">
                  <Field label={t("Fermentation time")}>
                    <UnitInput unit="h" inputMode="decimal" {...text("fermentation_hours")} />
                  </Field>
                  <Choice
                    label={t("Drying")}
                    value={form.drying}
                    options={DRYINGS}
                    onChange={(drying) => set({ drying })}
                  />
                  <Choice
                    label={t("Decaffeinated")}
                    value={form.decaf}
                    options={DECAFS}
                    onChange={(decaf) => set({ decaf })}
                  />
                </div>
                <Field label={t("Details as on the offer sheet")}>
                  <input
                    className={inputCls}
                    placeholder={t("e.g. 72 h anaerobic in the cherry, 18 days on raised beds")}
                    {...text("process_detail")}
                  />
                </Field>
              </>
            )}

            {step === 3 && (
              <>
                <div className="grid grid-cols-3 gap-3">
                  <Field label={t("Crop year")}>
                    <input className={inputCls} placeholder="2025/26" {...text("harvest")} />
                  </Field>
                  <Field label={t("Arrived on")}>
                    <input type="date" className={inputCls} {...text("arrival_date")} />
                  </Field>
                  <Field label={t("Grade")}>
                    <input className={inputCls} placeholder="SHB, AA, G1 …" {...text("grade")} />
                  </Field>
                  <Field label={t("Screen size")}>
                    <input className={inputCls} placeholder="15+, 17/18" {...text("screen_size")} />
                  </Field>
                  <Field label={t("Moisture")}>
                    <UnitInput unit="%" inputMode="decimal" {...text("moisture_pct")} />
                  </Field>
                  <Field label={t("Water activity")}>
                    <UnitInput unit="aw" inputMode="decimal" {...text("water_activity")} />
                  </Field>
                  <Field label={t("Density")}>
                    <UnitInput unit="g/l" inputMode="decimal" {...text("density_g_per_l")} />
                  </Field>
                  <Field label={t("Score (supplier)")}>
                    <UnitInput unit="SCA" inputMode="decimal" {...text("cupping_score")} />
                  </Field>
                </div>
                <Field label={t("Tasting notes (supplier)")}>
                  <input className={inputCls} {...text("flavor_notes")} />
                </Field>
              </>
            )}

            {step === 4 && (
              <>
                <div className="grid grid-cols-3 gap-3">
                  <Field label={t("Supplier / importer")}>
                    <input className={inputCls} {...text("supplier")} />
                  </Field>
                  <Field label={t("Supplier's lot no.")}>
                    <input className={inputCls} {...text("supplier_lot")} />
                  </Field>
                  <Field label={t("Price per kg")}>
                    <UnitInput unit="€" inputMode="decimal" {...text("price_per_kg")} />
                  </Field>
                  <Choice
                    label={t("Packaging")}
                    value={form.packaging}
                    options={PACKAGINGS}
                    onChange={(packaging) => set({ packaging })}
                  />
                  <div className="col-span-2">
                    <Field label={t("Storage location")}>
                      <input className={inputCls} {...text("storage_location")} />
                    </Field>
                  </div>
                </div>
                <Field label={t("Certifications")}>
                  <div
                    role="group"
                    aria-label={t("Certifications")}
                    className="flex flex-wrap gap-1.5"
                  >
                    {CERTIFICATIONS.map(([c, label]) => (
                      <Pill
                        key={c}
                        multi
                        on={form.certifications.includes(c)}
                        onClick={() => set({ certifications: toggle(form.certifications, c) })}
                      >
                        {t(label)}
                      </Pill>
                    ))}
                  </div>
                </Field>
                <Field label={t("Note")}>
                  <input className={inputCls} {...text("notes")} />
                </Field>
              </>
            )}
          </div>

          <DialogFooter className="items-center sm:justify-between">
            <Button
              type="button"
              variant="ghost"
              onClick={() => (step > 0 ? setStep(step - 1) : onClose())}
            >
              {step > 0 ? t("Back") : t("Cancel")}
            </Button>
            <div className="flex gap-2">
              {step < pages - 1 && (
                <Button type="button" variant="secondary" onClick={() => setStep(step + 1)}>
                  {t("Next: {{page}}", { page: t(STEPS[step + 1] ?? "") })}
                </Button>
              )}
              <Button type="submit" disabled={busy || !canSave}>
                {editing ? t("Save") : t("Into the inventory")}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
