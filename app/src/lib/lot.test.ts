import { describe, expect, it } from "vitest";

import type { Lot } from "@/api/types.ts";
import {
  adjustmentFor,
  detailsOf,
  detailsToForm,
  EMPTY_LOT_FORM,
  formToDetails,
  lotSummary,
} from "./lot.ts";

describe("lot details form", () => {
  it("sends only what was filled in, parsed", () => {
    const d = formToDetails({
      ...EMPTY_LOT_FORM,
      origin_country: " Kolumbien ",
      altitude_min: "1950",
      altitude_max: "2080",
      varieties: "Castillo, , Caturra",
      process: "washed",
      honey: "red",
      fermentation: ["anaerobic"],
      moisture_pct: "10,8",
      price_per_kg: "12,50",
      certifications: ["organic"],
    });
    expect(d).toEqual({
      origin_country: "Kolumbien",
      altitude: { min_m: 1950, max_m: 2080 },
      varieties: ["Castillo", "Caturra"],
      process: "washed",
      fermentation: ["anaerobic"],
      moisture_pct: 10.8,
      price_per_kg: { amount: 12.5, currency: "EUR" },
      certifications: ["organic"],
    });
    expect(formToDetails(EMPTY_LOT_FORM)).toEqual({});
  });

  it("keeps a honey grade only with the honey process", () => {
    expect(formToDetails({ ...EMPTY_LOT_FORM, process: "honey", honey: "red" }).honey).toBe("red");
  });

  it("reads a lot back into the form unchanged", () => {
    const lot: Lot = {
      id: "1",
      name: "88graines Castillo Washed",
      archived: false,
      created_at: "2026-11-09T00:00:00Z",
      stock_g: 2000,
      origin_country: "Kolumbien",
      altitude: { min_m: 1950, max_m: 2080 },
      varieties: ["Castillo"],
      process: "washed",
      supplier: "88graines",
    };
    const d = detailsOf(lot);
    expect(d).not.toHaveProperty("stock_g");
    expect(formToDetails(detailsToForm(d))).toEqual(d);
    expect(lotSummary(d, (k) => k)).toBe("Kolumbien · Washed · Castillo");
  });

  it("corrects the stock to what was weighed", () => {
    expect(adjustmentFor(2973, 2950)).toBe(-23);
    expect(adjustmentFor(-120, 0)).toBe(120);
  });
});
