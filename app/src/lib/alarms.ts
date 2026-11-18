import type { TFunction } from "i18next";

import type { EventKind } from "@/api/types.ts";
import { EVENT_NAME } from "@/lib/chartData.ts";

type T = TFunction;

/**
 * The daemon's messages are English text. These are the ones it sends, as
 * patterns with named parts; `t` looks up the English template, so German
 * comes from the translation table. A part named `rest` is translated in
 * turn (a message built from two). A message that matches nothing stays as
 * it is: better English than a wrong translation.
 */
export const PATTERNS: { re: RegExp; key: string }[] = [
  {
    re: /^Connected to (?<name>[^.]+(?:\.[^ ][^.]*)*)\. (?<rest>.+)$/,
    key: "Connected to {{name}}. {{rest}}",
  },
  { re: /^Connected to (?<name>.+)\.$/, key: "Connected to {{name}}." },
  {
    re: /^Recipe (?<label>.+): (?<actuator>burner|air|drum|filter|cooling) (?<value>-?\d+) %\.$/,
    key: "Recipe {{label}}: {{actuator}} {{value}} %.",
  },
  {
    re: /^Autopilot: burner (?<value>\d+) % \((?<delta>[+-]?\d+)\), the roast heads (?<c>\d+) °C above the plan\.$/,
    key: "Autopilot: burner {{value}} % ({{delta}}), the roast heads {{c}} °C above the plan.",
  },
  {
    re: /^Autopilot: burner (?<value>\d+) % \((?<delta>[+-]?\d+)\), the roast heads (?<c>\d+) °C below the plan\.$/,
    key: "Autopilot: burner {{value}} % ({{delta}}), the roast heads {{c}} °C below the plan.",
  },
  {
    re: /^Autopilot: burner (?<value>\d+) % \((?<delta>[+-]?\d+)\), the roast heads (?<c>\d+) °C above the plan \((?<pred>[+-]?\d+\.\d) °C in (?<h>\d+) s; now (?<now>[+-]?\d+\.\d) °C, median (?<med>[+-]?\d+\.\d) °C; RoR (?<ror>-?\d+\.\d) vs plan (?<pror>-?\d+\.\d)\)\.$/,
    key: "Autopilot: burner {{value}} % ({{delta}}), the roast heads {{c}} °C above the plan ({{pred}} °C in {{h}} s; now {{now}} °C, median {{med}} °C; RoR {{ror}} vs plan {{pror}}).",
  },
  {
    re: /^Autopilot: burner (?<value>\d+) % \((?<delta>[+-]?\d+)\), the roast heads (?<c>\d+) °C below the plan \((?<pred>[+-]?\d+\.\d) °C in (?<h>\d+) s; now (?<now>[+-]?\d+\.\d) °C, median (?<med>[+-]?\d+\.\d) °C; RoR (?<ror>-?\d+\.\d) vs plan (?<pror>-?\d+\.\d)\)\.$/,
    key: "Autopilot: burner {{value}} % ({{delta}}), the roast heads {{c}} °C below the plan ({{pred}} °C in {{h}} s; now {{now}} °C, median {{med}} °C; RoR {{ror}} vs plan {{pror}}).",
  },
  {
    re: /^Autopilot: back on the plan - the recipe's next burner steps move (?<shift>[+-]?\d+) points back toward the recipe\.$/,
    key: "Autopilot: back on the plan - the recipe's next burner steps move {{shift}} points back toward the recipe.",
  },
  {
    re: /^Preheating to (?<target>\d+) °C at air (?<air>\d+) %\. Confirm presence every (?<every>\d+) min while it runs \(a gong sounds before\)\.$/,
    key: "Preheating to {{target}} °C at air {{air}} %. Confirm presence every {{every}} min while it runs (a gong sounds before).",
  },
  {
    re: /^(?<holder>.+) asks for control\. Press “I'm at the roaster” in the app to allow it\.$/,
    key: "{{holder}} asks for control. Press “I'm at the roaster” in the app to allow it.",
  },
  {
    re: /^The roast is saved, but its green weight was not taken off the stock: (?<why>.+)$/,
    key: "The roast is saved, but its green weight was not taken off the stock: {{why}}",
  },
  { re: /^Saved "(?<title>.+)"\.$/, key: "Saved “{{title}}”." },
  {
    re: /^(?<kind>.+) marked at (?<at>\S+); it replaces the one detected at (?<was>\S+) \(phases, DTR and the plan use the marked one\)\.$/,
    key: "{{kind}} marked at {{at}}; it replaces the one detected at {{was}} (phases, DTR and the plan use the marked one).",
  },
  {
    re: /^(?<kind>.+) marked at (?<at>\S+); it replaces the one heard at (?<was>\S+) \(phases, DTR and the plan use the marked one\)\.$/,
    key: "{{kind}} marked at {{at}}; it replaces the one heard at {{was}} (phases, DTR and the plan use the marked one).",
  },
  {
    re: /^(?<kind>\S+) heard and marked: (?<at>.+) \((?<n>\d+) pops\)\. Your mark replaces it\.$/,
    key: "{{kind}} heard and marked: {{at}} ({{n}} pops). Your mark replaces it.",
  },
  {
    re: /^Not listening for first crack: (?<why>.+)\.$/,
    key: "Not listening for first crack: {{why}}.",
  },
  {
    re: /^DROP: burner off, bean cooling on\. AI \((?<holder>.+)\) handed control back to you\.$/,
    key: "DROP: burner off, bean cooling on. AI ({{holder}}) handed control back to you.",
  },
  {
    re: /^DROP at (?<target>\d+) °C expected within (?<s>\d+) s: bean cooling on\.$/,
    key: "DROP at {{target}} °C expected within {{s}} s: bean cooling on.",
  },
  {
    re: /^DROP at DTR (?<dtr>\d+) % expected within (?<s>\d+) s: bean cooling on\.$/,
    key: "DROP at DTR {{dtr}} % expected within {{s}} s: bean cooling on.",
  },
  {
    re: /^Plan targets disagree: (?<bt>\d+) °C at (?<bt_at>\S+) \(DTR (?<dtr_bt>\d+) %\), DTR (?<dtr>\d+) % at (?<dtr_at>\S+) \(about (?<bt_dtr>\d+) °C\)\. DROP follows the earlier\.$/,
    key: "Plan targets disagree: {{bt}} °C at {{bt_at}} (DTR {{dtr_bt}} %), DTR {{dtr}} % at {{dtr_at}} (about {{bt_dtr}} °C). DROP follows the earlier.",
  },
  {
    re: /^Plan targets disagree: (?<bt>\d+) °C at (?<bt_at>\S+) \(DTR (?<dtr_bt>\d+) %\), DTR (?<dtr>\d+) % at (?<dtr_at>\S+) \(about (?<bt_dtr>\d+) °C\)\. DROP follows the later\.$/,
    key: "Plan targets disagree: {{bt}} °C at {{bt_at}} (DTR {{dtr_bt}} %), DTR {{dtr}} % at {{dtr_at}} (about {{bt_dtr}} °C). DROP follows the later.",
  },
  {
    re: /^Plan targets disagree: (?<bt>\d+) °C at (?<bt_at>\S+) \(DTR (?<dtr_bt>\d+) %\), DTR (?<dtr>\d+) % at (?<dtr_at>\S+) \(about (?<bt_dtr>\d+) °C\)\. DROP follows the temperature\.$/,
    key: "Plan targets disagree: {{bt}} °C at {{bt_at}} (DTR {{dtr_bt}} %), DTR {{dtr}} % at {{dtr_at}} (about {{bt_dtr}} °C). DROP follows the temperature.",
  },
  {
    re: /^Plan targets disagree: (?<bt>\d+) °C at (?<bt_at>\S+) \(DTR (?<dtr_bt>\d+) %\), DTR (?<dtr>\d+) % at (?<dtr_at>\S+) \(about (?<bt_dtr>\d+) °C\)\. DROP follows the DTR\.$/,
    key: "Plan targets disagree: {{bt}} °C at {{bt_at}} (DTR {{dtr_bt}} %), DTR {{dtr}} % at {{dtr_at}} (about {{bt_dtr}} °C). DROP follows the DTR.",
  },
  {
    re: /^A recording was interrupted; it was recovered to (?<path>.+)\.$/,
    key: "A recording was interrupted; it was recovered to {{path}}.",
  },
  { re: /^Recovery failed: (?<e>.+)$/, key: "Recovery failed: {{e}}" },
  {
    re: /^Roaster connection lost\. Check the machine - you have control\. \((?<e>.+)\)$/,
    key: "Roaster connection lost. Check the machine - you have control. ({{e}})",
  },
  {
    re: /^Emergency stop \((?<reason>[^)]+)\): burner off, air high, drum running\.$/,
    key: "Emergency stop ({{reason}}): burner off, air high, drum running.",
  },
  {
    re: /^AI \((?<holder>.+)\) disconnected - you have control\.$/,
    key: "AI ({{holder}}) disconnected - you have control.",
  },
  {
    re: /^CHARGE taken back: it turned at (?<tp>\d+) °C after (?<s>\d+) s - beans turn near 100 °C\. No beans went in\? Charge again when ready\.$/,
    key: "CHARGE taken back: it turned at {{tp}} °C after {{s}} s - beans turn near 100 °C. No beans went in? Charge again when ready.",
  },
  {
    re: /^CHARGE taken back: the bean temperature fell only to (?<bt>\d+) °C in (?<s>\d+) s\. No beans went in\? Charge again when ready\.$/,
    key: "CHARGE taken back: the bean temperature fell only to {{bt}} °C in {{s}} s. No beans went in? Charge again when ready.",
  },
  {
    re: /^CHARGE taken back: no turning point in (?<s>\d+) s\. No beans went in\? Charge again when ready\.$/,
    key: "CHARGE taken back: no turning point in {{s}} s. No beans went in? Charge again when ready.",
  },
  {
    re: /^The preheat holds (?<target>\d+) °C again until CHARGE\.$/,
    key: "The preheat holds {{target}} °C again until CHARGE.",
  },
  {
    re: /^BT (?<bt>\d+) °C at (?<at>\S+) is (?<above>\d+) °C above the plan \(no beans, or a false CHARGE\?\): burner off, air high, drum running\.$/,
    key: "BT {{bt}} °C at {{at}} is {{above}} °C above the plan (no beans, or a false CHARGE?): burner off, air high, drum running.",
  },
  {
    re: /^BT (?<bt>\d+) °C at (?<at>\S+) is (?<above>\d+) °C above the plan \(no beans, or a false CHARGE\?\)\. Check the roaster!$/,
    key: "BT {{bt}} °C at {{at}} is {{above}} °C above the plan (no beans, or a false CHARGE?). Check the roaster!",
  },
  {
    re: /^BT (?<bt>\d+) °C at (?<at>\S+) heads for (?<to>\d+) °C within a minute \(limit (?<limit>\d+) °C\): burner off, air high, drum running\.$/,
    key: "BT {{bt}} °C at {{at}} heads for {{to}} °C within a minute (limit {{limit}} °C): burner off, air high, drum running.",
  },
  {
    re: /^BT (?<bt>\d+) °C at (?<at>\S+) heads for (?<to>\d+) °C within a minute \(limit (?<limit>\d+) °C\)\. Check the roaster!$/,
    key: "BT {{bt}} °C at {{at}} heads for {{to}} °C within a minute (limit {{limit}} °C). Check the roaster!",
  },
  {
    re: /^No first crack marked at (?<bt>\d+) °C \(expected about (?<exp>\d+) °C\)\. Mark it; at (?<at>\d+) °C the steps tied to it start without a mark\.$/,
    key: "No first crack marked at {{bt}} °C (expected about {{exp}} °C). Mark it; at {{at}} °C the steps tied to it start without a mark.",
  },
  {
    re: /^No first crack marked at (?<bt>\d+) °C \(expected about (?<exp>\d+) °C\)\. Mark it when you hear it\.$/,
    key: "No first crack marked at {{bt}} °C (expected about {{exp}} °C). Mark it when you hear it.",
  },
  {
    re: /^First crack not marked: the steps tied to it run from (?<at>\S+), when the beans passed (?<exp>\d+) °C\. Your mark moves the steps still to come\.$/,
    key: "First crack not marked: the steps tied to it run from {{at}}, when the beans passed {{exp}} °C. Your mark moves the steps still to come.",
  },
  {
    re: /^Rate of rise (?<ror>-?\d+\.\d) °C\/min after first crack: the roast may stall\. More heat, or drop soon\?$/,
    key: "Rate of rise {{ror}} °C/min after first crack: the roast may stall. More heat, or drop soon?",
  },
];

/** Messages without variable parts; their English text is the key. */
export const STATIC_ALARMS = [
  "Burner off, air high, drum running until you change them.",
  "Roaster connection lost. Check the machine - you have control.",
  "Presence was not confirmed in time: safe state, AI control ended.",
  "Autopilot off - you have control.",
  "Emergency stop - the autopilot is off.",
  "The plan is not a profile any more - the autopilot is off.",
  "Preheat stopped - you have control.",
  "CHARGE - preheat ended, you have control.",
  "Preheat ended - you have control.",
  "Preheat ended: presence not confirmed or control not allowed.",
  "The preheat holds on until CHARGE.",
  "Disconnected - the autopilot is off.",
  "The link is lost - the autopilot is off.",
  "CHARGE detected.",
  "DROP detected: the roast is saved.",
  "No new readings from the roaster for 5 seconds.",
  "You took over. AI control is suspended until you allow it again.",
  "Emergency stop released by a person.",
  "AI released control - you have control.",
  "The recording ended - the autopilot is off.",
  "DROP: burner off, bean cooling on. The autopilot is off.",
  "DROP: burner off, bean cooling on.",
  "CHARGE taken back and the preheat cannot hold: burner off, air high, drum running.",
  "The microphone delivers only silence: allow RoastMCP to use the microphone in the system settings (Privacy & Security).",
];

const ACTUATOR_NAME: Record<string, string> = {
  burner: "burner",
  air: "air",
  drum: "drum",
  filter: "filter",
  cooling: "cooling",
};

/** A daemon message in the person's language, where it is known. */
export function alarmText(message: string, t: T): string {
  if (STATIC_ALARMS.includes(message)) return t(message);
  for (const p of PATTERNS) {
    const m = p.re.exec(message);
    if (!m?.groups) continue;
    const parts: Record<string, string> = { ...m.groups };
    if (parts.rest !== undefined) {
      const rest = alarmText(parts.rest, t);
      // A message built from two is only translated if its second part is known.
      if (rest === parts.rest && !STATIC_ALARMS.includes(parts.rest)) return message;
      parts.rest = rest;
    }
    if (parts.actuator !== undefined) parts.actuator = t(ACTUATOR_NAME[parts.actuator]);
    if (parts.kind !== undefined && parts.kind in EVENT_NAME)
      parts.kind = t(EVENT_NAME[parts.kind as EventKind]);
    if (parts.why !== undefined) parts.why = t(parts.why);
    return t(p.key, parts);
  }
  return message;
}
