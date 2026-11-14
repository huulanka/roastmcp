import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import { de } from "@/i18n/de.ts";
import { alarmText, PATTERNS, STATIC_ALARMS } from "./alarms.ts";

/** Looks up the German text and fills the placeholders, like i18next. */
const t = ((key: string, options?: Record<string, unknown>): string =>
  (de[key] ?? key).replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
    String(options?.[name]),
  )) as unknown as TFunction;

describe("daemon messages in German", () => {
  it("has a translation for every known message", () => {
    for (const m of STATIC_ALARMS) expect(de[m], m).toBeDefined();
    for (const p of PATTERNS) expect(de[p.key], p.key).toBeDefined();
  });

  it("translates fixed messages and messages with values", () => {
    expect(alarmText("CHARGE detected.", t)).toBe("CHARGE erkannt.");
    expect(alarmText("Recipe FC +0:30: burner 35 %.", t)).toBe("Rezept FC +0:30: Brenner 35 %.");
    expect(alarmText("Autopilot: burner 42 % (-3), the roast heads 4 °C above the plan.", t)).toBe(
      "Autopilot: Brenner 42 % (-3), die Röstung läuft 4 °C über dem Plan.",
    );
    expect(alarmText("DROP at 206 °C expected within 30 s: bean cooling on.", t)).toBe(
      "DROP bei 206 °C in 30 s erwartet: Bohnenkühlung an.",
    );
  });

  it("translates a message built from two", () => {
    expect(
      alarmText(
        "Connected to Simulator (Skywalker Cubean+). Burner off, air high, drum running until you change them.",
        t,
      ),
    ).toBe(
      "Verbunden mit Simulator (Skywalker Cubean+). Brenner aus, Luft hoch, Trommel läuft, bis du etwas änderst.",
    );
    expect(alarmText("Connected to TC4 on /dev/cu.AKROASTER.", t)).toBe(
      "Verbunden mit TC4 on /dev/cu.AKROASTER.",
    );
  });

  it("leaves unknown messages alone", () => {
    expect(alarmText("Something new happened.", t)).toBe("Something new happened.");
    expect(alarmText("Connected to X. Something new.", t)).toBe("Connected to X. Something new.");
  });
});
