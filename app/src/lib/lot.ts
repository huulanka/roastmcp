import type {
  Certification,
  Decaf,
  Drying,
  Fermentation,
  HoneyGrade,
  Lot,
  LotDetails,
  Packaging,
  Process,
  Species,
} from "@/api/types.ts";
import { parseNumber } from "./plan.ts";

/** Display names (English keys, translated by i18n), in the order offered. */
export const PROCESSES: [Process, string][] = [
  ["washed", "Washed"],
  ["natural", "Natural"],
  ["honey", "Honey"],
  ["pulped_natural", "Pulped natural"],
  ["semi_washed", "Semi-washed"],
  ["wet_hulled", "Wet-hulled"],
  ["other", "Other"],
];
export const HONEY_GRADES: [HoneyGrade, string][] = [
  ["white", "White"],
  ["yellow", "Yellow"],
  ["red", "Red"],
  ["black", "Black"],
];
export const FERMENTATIONS: [Fermentation, string][] = [
  ["anaerobic", "Anaerobic"],
  ["carbonic_maceration", "Carbonic maceration"],
  ["thermal_shock", "Thermal shock"],
  ["extended", "Extended / double"],
  ["inoculated", "Inoculated (yeast, koji …)"],
  ["infused", "Infused"],
];
export const DRYINGS: [Drying, string][] = [
  ["raised_beds", "Raised beds"],
  ["patio", "Patio"],
  ["greenhouse", "Greenhouse / solar"],
  ["mechanical", "Mechanical"],
];
export const DECAFS: [Decaf, string][] = [
  ["swiss_water", "Swiss Water"],
  ["mountain_water", "Mountain Water"],
  ["ethyl_acetate", "Sugar cane (EA)"],
  ["co2", "CO₂"],
  ["methylene_chloride", "Methylene chloride"],
];
export const SPECIES: [Species, string][] = [
  ["arabica", "Arabica"],
  ["robusta", "Robusta (canephora)"],
  ["liberica", "Liberica"],
  ["excelsa", "Excelsa"],
];
export const PACKAGINGS: [Packaging, string][] = [
  ["grainpro", "GrainPro"],
  ["vacuum", "Vacuum"],
  ["jute", "Jute"],
  ["ecotact", "Ecotact"],
  ["other", "Other"],
];
export const CERTIFICATIONS: [Certification, string][] = [
  ["organic", "Organic"],
  ["fairtrade", "Fairtrade"],
  ["rainforest_alliance", "Rainforest Alliance"],
];

export function nameOf<T extends string>(list: [T, string][], value: T | undefined): string {
  return list.find(([v]) => v === value)?.[1] ?? "";
}

/** The details form: text as typed, choices as values. */
export interface LotForm {
  origin_country: string;
  region: string;
  producer: string;
  altitude_min: string;
  altitude_max: string;
  species: Species | "";
  varieties: string;
  process: Process | "";
  honey: HoneyGrade | "";
  fermentation: Fermentation[];
  fermentation_hours: string;
  drying: Drying | "";
  decaf: Decaf | "";
  process_detail: string;
  harvest: string;
  arrival_date: string;
  grade: string;
  screen_size: string;
  moisture_pct: string;
  water_activity: string;
  density_g_per_l: string;
  cupping_score: string;
  flavor_notes: string;
  supplier: string;
  supplier_lot: string;
  price_per_kg: string;
  packaging: Packaging | "";
  storage_location: string;
  certifications: Certification[];
  notes: string;
}

const num = (v: number | undefined) => (v === undefined || v === null ? "" : String(v));

export function detailsToForm(d: LotDetails): LotForm {
  return {
    origin_country: d.origin_country ?? "",
    region: d.region ?? "",
    producer: d.producer ?? "",
    altitude_min: num(d.altitude?.min_m),
    altitude_max: num(d.altitude?.max_m),
    species: d.species ?? "",
    varieties: (d.varieties ?? []).join(", "),
    process: d.process ?? "",
    honey: d.honey ?? "",
    fermentation: d.fermentation ?? [],
    fermentation_hours: num(d.fermentation_hours),
    drying: d.drying ?? "",
    decaf: d.decaf ?? "",
    process_detail: d.process_detail ?? "",
    harvest: d.harvest ?? "",
    arrival_date: d.arrival_date ?? "",
    grade: d.grade ?? "",
    screen_size: d.screen_size ?? "",
    moisture_pct: num(d.moisture_pct),
    water_activity: num(d.water_activity),
    density_g_per_l: num(d.density_g_per_l),
    cupping_score: num(d.cupping_score),
    flavor_notes: d.flavor_notes ?? "",
    supplier: d.supplier ?? "",
    supplier_lot: d.supplier_lot ?? "",
    price_per_kg: num(d.price_per_kg?.amount),
    packaging: d.packaging ?? "",
    storage_location: d.storage_location ?? "",
    certifications: d.certifications ?? [],
    notes: d.notes ?? "",
  };
}

export const EMPTY_LOT_FORM: LotForm = detailsToForm({});

/** The details to send; blanks are left out, numbers parsed (comma or point). */
export function formToDetails(f: LotForm): LotDetails {
  const d: LotDetails = {};
  const text = (v: string) => v.trim() || undefined;
  const n = (v: string) => parseNumber(v) ?? undefined;
  d.origin_country = text(f.origin_country);
  d.region = text(f.region);
  d.producer = text(f.producer);
  const min = parseNumber(f.altitude_min);
  const max = parseNumber(f.altitude_max);
  if (min !== null) {
    d.altitude = { min_m: Math.round(min), ...(max !== null ? { max_m: Math.round(max) } : {}) };
  } else if (max !== null) {
    d.altitude = { min_m: Math.round(max) };
  }
  d.species = f.species || undefined;
  const varieties = f.varieties
    .split(",")
    .map((v) => v.trim())
    .filter(Boolean);
  if (varieties.length) d.varieties = varieties;
  d.process = f.process || undefined;
  d.honey = f.process === "honey" ? f.honey || undefined : undefined;
  if (f.fermentation.length) d.fermentation = f.fermentation;
  d.fermentation_hours = n(f.fermentation_hours);
  d.drying = f.drying || undefined;
  d.decaf = f.decaf || undefined;
  d.process_detail = text(f.process_detail);
  d.harvest = text(f.harvest);
  d.arrival_date = text(f.arrival_date);
  d.grade = text(f.grade);
  d.screen_size = text(f.screen_size);
  d.moisture_pct = n(f.moisture_pct);
  d.water_activity = n(f.water_activity);
  d.density_g_per_l = n(f.density_g_per_l);
  d.cupping_score = n(f.cupping_score);
  d.flavor_notes = text(f.flavor_notes);
  d.supplier = text(f.supplier);
  d.supplier_lot = text(f.supplier_lot);
  const price = parseNumber(f.price_per_kg);
  if (price !== null) d.price_per_kg = { amount: price, currency: "EUR" };
  d.packaging = f.packaging || undefined;
  d.storage_location = text(f.storage_location);
  if (f.certifications.length) d.certifications = f.certifications;
  d.notes = text(f.notes);
  // Drop the keys left undefined, so the request stays small.
  return Object.fromEntries(Object.entries(d).filter(([, v]) => v !== undefined)) as LotDetails;
}

/** The details of a lot, without its identity and stock. */
export function detailsOf(lot: Lot): LotDetails {
  const {
    id: _id,
    name: _n,
    archived: _a,
    created_at: _c,
    purchase_date: _p,
    stock_g: _s,
    ...d
  } = lot;
  return d;
}

/** "Brasilien · Natural · Castillo" for lists. */
export function lotSummary(lot: LotDetails, t: (key: string) => string): string {
  const process = lot.process
    ? lot.process === "honey" && lot.honey
      ? `${t(nameOf(HONEY_GRADES, lot.honey))} ${t("Honey")}`
      : t(nameOf(PROCESSES, lot.process))
    : null;
  return [lot.origin_country, process, lot.varieties?.join(", ")].filter(Boolean).join(" · ");
}

/** The correction that makes the stock what was weighed. */
export function adjustmentFor(stockG: number, weighedG: number): number {
  return Math.round((weighedG - stockG) * 10) / 10;
}
