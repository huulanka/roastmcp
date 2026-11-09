import { useEffect, useMemo, useState } from "react";
import i18n from "i18next";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Lot, RoastSummary, StockMovement } from "@/api/types.ts";
import { UnitInput } from "@/components/Dialogs.tsx";
import { PurchaseWizard, type PurchaseMode } from "@/components/PurchaseWizard.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  adjustmentFor,
  CERTIFICATIONS,
  DECAFS,
  DRYINGS,
  FERMENTATIONS,
  HONEY_GRADES,
  lotSummary,
  nameOf,
  PACKAGINGS,
  PROCESSES,
  SPECIES,
} from "@/lib/lot.ts";
import { batchCodeOf } from "@/lib/batch.ts";
import { kg, parseNumber } from "@/lib/plan.ts";
import { formatNumber } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function shortDate(iso: string): string {
  return new Intl.DateTimeFormat(i18n.language, {
    day: "numeric",
    month: "short",
    year: "2-digit",
  }).format(new Date(iso));
}

function signedGrams(g: number): string {
  const abs = Math.abs(g);
  const text = abs >= 1000 ? kg(abs) : `${formatNumber(abs, 0)} g`;
  return `${g < 0 ? "−" : "+"}${text}`;
}

/** The filled-in details of a lot, as label and value, in offer-sheet order. */
function facts(lot: Lot, t: (k: string) => string): [string, string][] {
  const out: [string, string | undefined | null][] = [
    [t("Origin"), [lot.origin_country, lot.region].filter(Boolean).join(", ")],
    [t("Producer"), lot.producer],
    [
      t("Altitude"),
      lot.altitude
        ? `${lot.altitude.min_m}${lot.altitude.max_m ? `–${lot.altitude.max_m}` : ""} m`
        : null,
    ],
    [t("Species"), lot.species ? t(nameOf(SPECIES, lot.species)) : null],
    [t("Varieties"), lot.varieties?.join(", ")],
    [
      t("Processing"),
      [
        lot.process
          ? lot.process === "honey" && lot.honey
            ? `${t(nameOf(HONEY_GRADES, lot.honey))} ${t("Honey")}`
            : t(nameOf(PROCESSES, lot.process))
          : null,
        ...(lot.fermentation ?? []).map((f) => t(nameOf(FERMENTATIONS, f))),
        lot.fermentation_hours ? `${formatNumber(lot.fermentation_hours, 0)} h` : null,
        lot.drying ? t(nameOf(DRYINGS, lot.drying)) : null,
      ]
        .filter(Boolean)
        .join(" · "),
    ],
    [t("Decaffeinated"), lot.decaf ? t(nameOf(DECAFS, lot.decaf)) : null],
    [t("Details"), lot.process_detail],
    [t("Crop year"), lot.harvest],
    [t("Arrived on"), lot.arrival_date ? shortDate(lot.arrival_date) : null],
    [t("Grade"), [lot.grade, lot.screen_size].filter(Boolean).join(" · ")],
    [
      t("Measured"),
      [
        lot.moisture_pct !== undefined ? `${formatNumber(lot.moisture_pct, 1)} %` : null,
        lot.water_activity !== undefined ? `${formatNumber(lot.water_activity, 2)} aw` : null,
        lot.density_g_per_l !== undefined ? `${formatNumber(lot.density_g_per_l, 0)} g/l` : null,
      ]
        .filter(Boolean)
        .join(" · "),
    ],
    [
      t("Supplier's cup"),
      [
        lot.cupping_score !== undefined ? `${formatNumber(lot.cupping_score, 1)} SCA` : null,
        lot.flavor_notes,
      ]
        .filter(Boolean)
        .join(" · "),
    ],
    [t("Supplier"), [lot.supplier, lot.supplier_lot].filter(Boolean).join(" · ")],
    [
      t("Price"),
      lot.price_per_kg
        ? `${formatNumber(lot.price_per_kg.amount, 2)} ${lot.price_per_kg.currency}/kg`
        : null,
    ],
    [
      t("Storage"),
      [lot.packaging ? t(nameOf(PACKAGINGS, lot.packaging)) : null, lot.storage_location]
        .filter(Boolean)
        .join(" · "),
    ],
    [
      t("Certifications"),
      (lot.certifications ?? []).map((c) => t(nameOf(CERTIFICATIONS, c))).join(", "),
    ],
    [t("Note"), lot.notes],
  ];
  return out.filter((f): f is [string, string] => !!f[1]);
}

function LotDetail({
  lot,
  roasts,
  onWizard,
  onChanged,
}: {
  lot: Lot;
  roasts: RoastSummary[];
  onWizard: (m: PurchaseMode) => void;
  onChanged: (lot: Lot) => void;
}) {
  const { t } = useTranslation();
  const [movements, setMovements] = useState<StockMovement[]>([]);
  const [weighed, setWeighed] = useState("");

  useEffect(() => {
    let alive = true;
    api
      .lotMovements(lot.id)
      .then((m) => alive && setMovements(m))
      .catch((e) => toast("warning", errText(e)));
    return () => {
      alive = false;
    };
  }, [lot.id, lot.stock_g]);

  const byId = useMemo(() => new Map(roasts.map((r) => [r.id, r])), [roasts]);
  const ofLot = roasts.filter((r) => r.lot_id === lot.id && !r.simulated);
  const usual = ofLot.find((r) => r.green_weight_g)?.green_weight_g ?? null;
  const weighedG = parseNumber(weighed);

  const correct = async () => {
    if (weighedG === null) return;
    const grams = adjustmentFor(lot.stock_g, weighedG);
    if (grams === 0) {
      setWeighed("");
      return;
    }
    try {
      onChanged(await api.adjustLot(lot.id, { grams, note: t("weighed") }));
      setWeighed("");
    } catch (e) {
      toast("critical", errText(e));
    }
  };

  const archive = async () => {
    try {
      onChanged(await api.patchLot(lot.id, { archived: !lot.archived }));
    } catch (e) {
      toast("critical", errText(e));
    }
  };

  // "BRYB-002 · Roast": the batch number says which bag it is.
  const roastLabel = (r: RoastSummary | undefined) =>
    r?.batch_no ? `${r.batch_no} · ${t("Roast")}` : r?.title || t("Roast");
  const kindLabel = (m: StockMovement) =>
    m.kind === "purchase"
      ? t("Purchase")
      : m.kind === "roast_debit"
        ? roastLabel(m.roast_id ? byId.get(m.roast_id) : undefined)
        : t("Correction");

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-5 overflow-y-auto">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="text-xl font-semibold">{lot.name}</h2>
          <p className="text-[14px] text-muted-foreground">
            {[lotSummary(lot, t), `${t("Batch code")} ${batchCodeOf(lot)}`]
              .filter(Boolean)
              .join(" · ")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={() => onWizard({ kind: "restock", lotId: lot.id })}>
            {t("More of this bean")}
          </Button>
          <Button variant="outline" onClick={() => onWizard({ kind: "edit", lot })}>
            {t("Edit")}
          </Button>
          <Button variant="ghost" onClick={() => void archive()}>
            {lot.archived ? t("Restore") : t("Archive this bean")}
          </Button>
        </div>
      </div>

      <div className="flex flex-wrap items-end gap-x-10 gap-y-4 border-t border-border pt-4">
        <div className="flex flex-col gap-1">
          <span className="text-[13px] text-muted-foreground">{t("In stock")}</span>
          <span
            className={cn(
              "font-display text-[3rem] leading-none",
              lot.stock_g < 0 && "text-danger-text",
            )}
          >
            {kg(lot.stock_g)}
          </span>
          {usual && lot.stock_g > 0 && (
            <span className="text-[13px] text-muted-foreground">
              {t("enough for about {{n}} roasts of {{g}} g", {
                n: Math.floor(lot.stock_g / usual),
                g: formatNumber(usual, 0),
              })}
            </span>
          )}
        </div>
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void correct();
          }}
        >
          <span className="text-[13px] text-muted-foreground">{t("Weighed the bag?")}</span>
          <span className="flex gap-2">
            <span className="w-36">
              <UnitInput
                unit="g"
                inputMode="decimal"
                aria-label={t("Weighed")}
                value={weighed}
                onChange={(e) => setWeighed(e.target.value)}
              />
            </span>
            <Button type="submit" variant="outline" disabled={weighedG === null}>
              {t("Set stock")}
            </Button>
          </span>
        </form>
      </div>

      {facts(lot, t).length > 0 && (
        <dl className="grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-x-8 gap-y-3 border-t border-border pt-4 text-[14px]">
          {facts(lot, t).map(([label, value]) => (
            <div key={label} className="flex min-w-0 flex-col gap-0.5">
              <dt className="text-[13px] text-muted-foreground">{label}</dt>
              <dd className="break-words">{value}</dd>
            </div>
          ))}
        </dl>
      )}

      <section aria-label={t("Movements")} className="flex flex-col border-t border-border pt-4">
        <h3 className="pb-2 text-[13px] font-medium text-muted-foreground">{t("Movements")}</h3>
        <ol className="flex flex-col">
          {movements.map((m) => (
            <li
              key={m.id}
              className="grid grid-cols-[6rem_minmax(0,1fr)_auto] items-baseline gap-3 border-b border-border py-2.5 text-[14px] last:border-b-0"
            >
              <span className="text-muted-foreground">{shortDate(m.at)}</span>
              <span className="truncate">
                {kindLabel(m)}
                {m.note && <span className="text-muted-foreground"> · {m.note}</span>}
              </span>
              <span className={cn("tabular font-medium", m.grams < 0 && "text-muted-foreground")}>
                {signedGrams(m.grams)}
              </span>
            </li>
          ))}
        </ol>
      </section>
    </div>
  );
}

/** The Inventory screen (its own tab): green coffee lots, stock and movements (ADR 0014). */
export function InventoryView() {
  const { t } = useTranslation();
  const [lots, setLots] = useState<Lot[] | null>(null);
  const [roasts, setRoasts] = useState<RoastSummary[]>([]);
  const [archived, setArchived] = useState(false);
  const [sel, setSel] = useState<string | null>(null);
  const [wizard, setWizard] = useState<PurchaseMode | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let alive = true;
    Promise.all([api.lots(archived), api.roasts()])
      .then(([l, r]) => {
        if (!alive) return;
        setLots(l);
        setRoasts(r);
      })
      .catch((e) => alive && (setLots([]), toast("critical", errText(e))));
    return () => {
      alive = false;
    };
  }, [archived, tick]);

  const selected = lots?.find((l) => l.id === sel) ?? lots?.[0] ?? null;
  const active = (lots ?? []).filter((l) => !l.archived);
  const total = active.reduce((sum, l) => sum + Math.max(0, l.stock_g), 0);
  const roastCount = (id: string) =>
    roasts.filter((r) => r.lot_id === id && !r.simulated && typeof r.total_s === "number");

  const changed = (lot: Lot) => {
    setSel(lot.id);
    setTick((n) => n + 1);
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-section lg:flex-row lg:overflow-hidden">
      <section
        aria-label={t("Inventory")}
        className="flex min-h-0 flex-col gap-3 lg:w-[28rem] lg:shrink-0"
      >
        <div className="flex items-center justify-between gap-3">
          <span className="text-[14px] text-muted-foreground">
            {active.length > 0
              ? active.length === 1
                ? t("1 bean · {{stock}}", { stock: kg(total) })
                : t("{{n}} beans · {{stock}}", { n: active.length, stock: kg(total) })
              : ""}
          </span>
          <Button onClick={() => setWizard({ kind: lots?.length ? "restock" : "new" })}>
            {t("Record a purchase")}
          </Button>
        </div>
        {lots === null ? (
          <p className="text-[15px] text-muted-foreground">{t("Loading…")}</p>
        ) : lots.length === 0 ? (
          <div className="flex flex-col items-start gap-3 rounded-card border border-dashed border-border-strong p-6 text-[15px] text-muted-foreground">
            {archived ? t("No used-up beans.") : t("No beans in the inventory yet.")}
          </div>
        ) : (
          <ul className="-mx-2 flex min-h-0 flex-col overflow-y-auto">
            {lots.map((l) => {
              const done = roastCount(l.id);
              const last = done.sort((a, b) => b.roasted_at.localeCompare(a.roasted_at))[0];
              return (
                <li key={l.id} className="border-b border-border last:border-b-0">
                  <button
                    type="button"
                    onClick={() => setSel(l.id)}
                    aria-current={selected?.id === l.id ? "true" : undefined}
                    className={cn(
                      "flex w-full cursor-pointer items-center gap-4 rounded-xl px-2 py-3 text-left transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                      selected?.id === l.id ? "bg-surface" : "hover:bg-surface",
                    )}
                  >
                    <span className="flex min-w-0 flex-1 flex-col gap-1">
                      <span className="truncate text-[15px] font-medium">
                        {l.name}
                        {l.archived && (
                          <span className="font-normal text-muted-foreground">
                            {" "}
                            · {t("archived")}
                          </span>
                        )}
                      </span>
                      <span className="truncate text-[13px] text-muted-foreground">
                        {[
                          lotSummary(l, t),
                          done.length
                            ? done.length === 1
                              ? t("1 roast")
                              : t("{{n}} roasts", { n: done.length })
                            : null,
                          last ? shortDate(last.roasted_at) : null,
                        ]
                          .filter(Boolean)
                          .join(" · ")}
                      </span>
                    </span>
                    <span
                      className={cn(
                        "shrink-0 tabular text-[15px] font-medium",
                        l.stock_g < 0 && "text-danger-text",
                      )}
                    >
                      {kg(l.stock_g)}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        <button
          type="button"
          onClick={() => setArchived((a) => !a)}
          className="cursor-pointer self-start text-[13px] text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          {archived ? t("Hide used-up beans") : t("Show used-up beans")}
        </button>
      </section>
      {selected ? (
        <LotDetail
          key={selected.id}
          lot={selected}
          roasts={roasts}
          onWizard={setWizard}
          onChanged={changed}
        />
      ) : (
        <div className="grid flex-1 place-items-center rounded-card border border-dashed border-border-strong p-10 text-center text-[15px] text-muted-foreground">
          <div className="flex flex-col items-center gap-4">
            {t("Record your first purchase: name and amount are enough.")}
            <Button onClick={() => setWizard({ kind: "new" })}>{t("Record a purchase")}</Button>
          </div>
        </div>
      )}
      {wizard && (
        <PurchaseWizard
          mode={wizard}
          lots={active}
          onClose={() => setWizard(null)}
          onSaved={(lot) => {
            setWizard(null);
            changed(lot);
            toast(
              "success",
              t("Saved {{name}}: {{stock}} in stock.", { name: lot.name, stock: kg(lot.stock_g) }),
            );
          }}
        />
      )}
    </div>
  );
}
