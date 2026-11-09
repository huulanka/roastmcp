import type { Lot, RoastGoal, RoastSummary } from "@/api/types.ts";

/**
 * Batch numbers as the roaster writes them on the label: the lot's code and a
 * running number per lot, "BRYB-002". The code is the origin's country code
 * and the initials of the lot's name, unless the roaster set another.
 */

/** ISO 3166 codes of coffee origins, by the names people write (German and English). */
const COUNTRY_CODES: Record<string, string> = {
  australien: "AU",
  australia: "AU",
  bolivien: "BO",
  bolivia: "BO",
  brasilien: "BR",
  brasil: "BR",
  brazil: "BR",
  burundi: "BI",
  china: "CN",
  "costa rica": "CR",
  "dominikanische republik": "DO",
  "dominican republic": "DO",
  ecuador: "EC",
  "el salvador": "SV",
  äthiopien: "ET",
  aethiopien: "ET",
  ethiopia: "ET",
  guatemala: "GT",
  haiti: "HT",
  hawaii: "US",
  honduras: "HN",
  indien: "IN",
  india: "IN",
  indonesien: "ID",
  indonesia: "ID",
  jamaika: "JM",
  jamaica: "JM",
  jemen: "YE",
  yemen: "YE",
  kamerun: "CM",
  cameroon: "CM",
  kenia: "KE",
  kenya: "KE",
  kolumbien: "CO",
  colombia: "CO",
  kongo: "CD",
  congo: "CD",
  "dr kongo": "CD",
  "dr congo": "CD",
  kuba: "CU",
  cuba: "CU",
  laos: "LA",
  madagaskar: "MG",
  madagascar: "MG",
  malawi: "MW",
  mexiko: "MX",
  mexico: "MX",
  myanmar: "MM",
  nepal: "NP",
  nicaragua: "NI",
  panama: "PA",
  "papua-neuguinea": "PG",
  "papua neuguinea": "PG",
  "papua new guinea": "PG",
  peru: "PE",
  philippinen: "PH",
  philippines: "PH",
  ruanda: "RW",
  rwanda: "RW",
  sambia: "ZM",
  zambia: "ZM",
  simbabwe: "ZW",
  zimbabwe: "ZW",
  "sri lanka": "LK",
  taiwan: "TW",
  tansania: "TZ",
  tanzania: "TZ",
  thailand: "TH",
  "timor-leste": "TL",
  osttimor: "TL",
  uganda: "UG",
  usa: "US",
  venezuela: "VE",
  vietnam: "VN",
};

function plain(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase().trim();
}

/** "Brasilien" → "BR"; unknown names give their first two letters. */
export function countryCode(country: string | null | undefined): string {
  if (!country?.trim()) return "";
  const known = COUNTRY_CODES[country.trim().toLowerCase()] ?? COUNTRY_CODES[plain(country)];
  if (known) return known;
  return plain(country)
    .replace(/[^a-z]/g, "")
    .slice(0, 2)
    .toUpperCase();
}

/** "Yellow Bourbon / Natural" → "YB": initials of the first two words, or the first two letters of one. */
function initials(name: string): string {
  const words = plain(name)
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
  if (words.length === 0) return "";
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
  return (words[0][0] + words[1][0]).toUpperCase();
}

/** The code suggested for a lot: country code + initials, e.g. "BRYB". */
export function suggestBatchCode(lot: Pick<Lot, "name" | "origin_country">): string {
  return (countryCode(lot.origin_country) + initials(lot.name)).slice(0, 8);
}

/** The lot's code: its own, else the suggestion. */
export function batchCodeOf(lot: Pick<Lot, "name" | "origin_country" | "batch_code">): string {
  return lot.batch_code?.trim() || suggestBatchCode(lot);
}

/** A code as stored: letters and digits, upper case, at most 8. */
export function cleanBatchCode(raw: string): string {
  return raw
    .replace(/[^A-Za-z0-9]/g, "")
    .toUpperCase()
    .slice(0, 8);
}

/**
 * The real roasts of a lot: linked to it, or imported with a bean text that
 * names it (Artisan files know no lots).
 */
export function roastsOfLot(lot: Pick<Lot, "id" | "name">, roasts: readonly RoastSummary[]) {
  const name = plain(lot.name);
  return roasts.filter(
    (r) =>
      !r.simulated &&
      (r.lot_id === lot.id || (!r.lot_id && !!r.beans && plain(r.beans).includes(name))),
  );
}

/**
 * The next batch number of a lot: one more than its roasts so far, or than the
 * highest number already used with its code, whichever is larger.
 */
export function nextBatchNo(
  lot: Pick<Lot, "id" | "name" | "origin_country" | "batch_code">,
  roasts: readonly RoastSummary[],
): string {
  const code = batchCodeOf(lot);
  let n = roastsOfLot(lot, roasts).length;
  const own = new RegExp(`^${code}-?(\\d+)$`, "i");
  for (const r of roasts) {
    const m = r.batch_no?.match(own);
    if (m) n = Math.max(n, Number(m[1]));
  }
  return `${code}-${String(n + 1).padStart(3, "0")}`;
}

/** Two lines for the bag's label: number and bean, then goal and roast date. */
export function labelText(
  o: {
    batchNo: string | null | undefined;
    beans: string | null | undefined;
    goal: RoastGoal | null | undefined;
    roastedAt: string;
  },
  t: (key: string) => string,
  goalName: (g: RoastGoal) => string,
): string {
  const bean = (o.beans ?? "").replace(/\s*\/\s*/g, " ").trim();
  const first = [o.batchNo, bean].filter(Boolean).join(" · ");
  const d = new Date(o.roastedAt);
  const date = `${String(d.getDate()).padStart(2, "0")}.${String(d.getMonth() + 1).padStart(2, "0")}.${String(d.getFullYear()).slice(2)}`;
  const second = [o.goal ? goalName(o.goal) : null, `${t("roasted")} ${date}`]
    .filter(Boolean)
    .join(" · ");
  return `${first}\n${second}`;
}
