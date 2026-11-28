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
    expect(
      alarmText(
        "Autopilot: burner 90 % (+5), the roast heads 4 °C below the plan (-4.0 °C in 90 s; now -1.9 °C, median -1.1 °C; RoR 15.4 vs plan 16.8).",
        t,
      ),
    ).toBe(
      "Autopilot: Brenner 90 % (+5), die Röstung läuft 4 °C unter dem Plan (-4.0 °C in 90 s; jetzt -1.9 °C, Median -1.1 °C; RoR 15.4 statt 16.8).",
    );
    expect(
      alarmText(
        "Autopilot: back on the plan - the recipe's next burner steps move -3 points back toward the recipe.",
        t,
      ),
    ).toBe(
      "Autopilot: wieder auf dem Plan – die nächsten Brennerstufen des Rezepts gehen -3 Punkte zurück Richtung Rezept.",
    );
    expect(
      alarmText(
        "First crack not marked: the steps tied to it run from 8:40, when the beans passed 194 °C. Your mark moves the steps still to come.",
        t,
      ),
    ).toBe(
      "First Crack nicht markiert: Die FC-Schritte laufen ab 8:40, als die Bohnen 194 °C erreichten. Deine Marke verschiebt die noch offenen Schritte.",
    );
    expect(
      alarmText(
        "The roaster reports burner 0 % and air 100 %, but burner 80 % and air 20 % was set: sent again.",
        t,
      ),
    ).toBe(
      "Der Röster meldet Brenner 0 % und Luft 100 %, eingestellt war Brenner 80 % und Luft 20 %: erneut gesendet.",
    );
    expect(alarmText("DROP at 206 °C expected within 30 s: bean cooling on.", t)).toBe(
      "DROP bei 206 °C in 30 s erwartet: Bohnenkühlung an.",
    );
    expect(
      alarmText(
        "CHARGE taken back: it turned at 166 °C after 25 s - beans turn near 100 °C. No beans went in? Charge again when ready.",
        t,
      ),
    ).toBe(
      "CHARGE zurückgenommen: Wendepunkt bei 166 °C nach 25 s – Bohnen wenden um 100 °C. Keine Bohnen eingefüllt? Neu chargen, wenn bereit.",
    );
    expect(
      alarmText(
        "BT 189 °C at 0:56 is 89 °C above the plan (no beans, or a false CHARGE?): burner off, air high, drum running.",
        t,
      ),
    ).toBe(
      "BT 189 °C bei 0:56 liegt 89 °C über dem Plan (keine Bohnen oder falscher CHARGE?): Brenner aus, Luft hoch, Trommel läuft.",
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
