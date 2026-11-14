import type { TFunction } from "i18next";

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
    re: /^DROP: burner off, bean cooling on\. AI \((?<holder>.+)\) handed control back to you\.$/,
    key: "DROP: burner off, bean cooling on. AI ({{holder}}) handed control back to you.",
  },
  {
    re: /^DROP at (?<target>\d+) °C expected within (?<s>\d+) s: bean cooling on\.$/,
    key: "DROP at {{target}} °C expected within {{s}} s: bean cooling on.",
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
    return t(p.key, parts);
  }
  return message;
}
