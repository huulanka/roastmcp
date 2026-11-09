import { describe, expect, it } from "vitest";

import type { RoastSummary } from "@/api/types.ts";
import {
  batchCodeOf,
  cleanBatchCode,
  countryCode,
  labelText,
  nextBatchNo,
  roastsOfLot,
  suggestBatchCode,
} from "./batch.ts";

const roast = (o: Partial<RoastSummary>): RoastSummary => ({
  id: Math.random().toString(),
  title: "t",
  roasted_at: "2026-09-29T17:39:00Z",
  total_s: 600,
  drop_bt: 204,
  green_weight_g: 200,
  roasted_weight_g: 171,
  ...o,
});

const yb = { id: "yb", name: "Yellow Bourbon / Natural", origin_country: "Brasilien" };

describe("batch codes", () => {
  it("suggests country code and initials", () => {
    expect(suggestBatchCode(yb)).toBe("BRYB");
    expect(suggestBatchCode({ name: "Milenio / Black Honey", origin_country: "Costa Rica" })).toBe(
      "CRMB",
    );
    expect(suggestBatchCode({ name: "Castillo", origin_country: "Kolumbien" })).toBe("COCA");
    expect(suggestBatchCode({ name: "Guji", origin_country: "Äthiopien" })).toBe("ETGU");
    expect(countryCode("Atlantis")).toBe("AT");
    expect(countryCode(undefined)).toBe("");
  });

  it("prefers the lot's own code and cleans what is typed", () => {
    expect(batchCodeOf({ ...yb, batch_code: "YBN" })).toBe("YBN");
    expect(batchCodeOf({ ...yb, batch_code: " " })).toBe("BRYB");
    expect(cleanBatchCode("br-yb 2!")).toBe("BRYB2");
  });
});

describe("next batch number", () => {
  it("counts the lot's real roasts, imports included", () => {
    const roasts = [
      roast({ beans: "88graines Yellow Bourbon / Natural PROCESS: Natural", batch_no: "BR011" }),
      roast({ lot_id: "yb", simulated: true }),
      roast({ lot_id: "other" }),
    ];
    expect(roastsOfLot(yb, roasts)).toHaveLength(1);
    expect(nextBatchNo(yb, roasts)).toBe("BRYB-002");
  });

  it("continues after the highest number used with the code", () => {
    const roasts = [roast({ lot_id: "yb", batch_no: "BRYB-007" })];
    expect(nextBatchNo(yb, roasts)).toBe("BRYB-008");
    expect(nextBatchNo(yb, [])).toBe("BRYB-001");
  });
});

describe("label", () => {
  it("has the number, the bean, the goal and the date", () => {
    const text = labelText(
      {
        batchNo: "BRYB-002",
        beans: "Yellow Bourbon / Natural",
        goal: "espresso",
        roastedAt: "2026-11-09T18:04:00",
      },
      (k) => (k === "roasted" ? "geröstet" : k),
      () => "Espresso",
    );
    expect(text).toBe("BRYB-002 · Yellow Bourbon Natural\nEspresso · geröstet 09.11.26");
  });
});
