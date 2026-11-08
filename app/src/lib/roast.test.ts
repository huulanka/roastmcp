import { describe, expect, it } from "vitest";

import type { EventKind, SafetyStatus } from "@/api/types.ts";
import {
  latestAiNote,
  liveDtr,
  nextAction,
  presenceView,
  primaryLabel,
  rorAlert,
  rorDelta,
} from "./roast.ts";

const ev = (kind: EventKind, t = 0) => ({ t, kind, origin: "human" as const, bt: null });

describe("next action", () => {
  it("starts a recording when connected and idle", () => {
    expect(nextAction("connected", true, [])).toEqual({ type: "start" });
    expect(nextAction("finished", true, [])).toEqual({ type: "start" });
    expect(nextAction("disconnected", false, [])).toBeNull();
  });

  it("walks charge, first crack, drop, cooling end, then save", () => {
    const events = [] as ReturnType<typeof ev>[];
    expect(nextAction("recording", true, events)).toEqual({ type: "mark", kind: "charge" });
    events.push(ev("charge"), ev("turning_point"), ev("dry_end"));
    expect(nextAction("charged", true, events)).toEqual({
      type: "mark",
      kind: "first_crack_start",
    });
    events.push(ev("first_crack_start"));
    expect(nextAction("charged", true, events)).toEqual({ type: "mark", kind: "drop" });
    events.push(ev("drop"));
    expect(nextAction("dropped", true, events)).toEqual({ type: "mark", kind: "cool_end" });
    events.push(ev("cool_end"));
    expect(nextAction("cooling", true, events)).toEqual({ type: "stop" });
  });

  it("skips first crack when the roast was dropped without one", () => {
    expect(nextAction("dropped", true, [ev("charge"), ev("drop")])).toEqual({
      type: "mark",
      kind: "cool_end",
    });
  });
});

describe("rate of rise", () => {
  const x = [0, 10, 20, 30, 40];
  it("compares the RoR now with 30 s ago", () => {
    expect(rorDelta(x, [12, 11, 10, 9, 8])).toBe(-3);
    expect(rorDelta(x, [8, 8.5, 9, 9.5, 10])).toBe(1.5);
    expect(rorDelta([0, 10], [10, 9])).toBeNull();
    expect(rorDelta(x, [null, null, null, null, null])).toBeNull();
  });

  const at = (delta: number, sinceCharge: number, firstCrack: number | null = null) =>
    rorAlert({ delta, sinceCharge, firstCrack, roastS: 600, dropped: false });
  it("flags a crash only after first crack or late in the roast", () => {
    expect(at(-3.5, 200)).toBeNull();
    expect(at(-3.5, 420)).toBe("crash");
    expect(at(-3, 300, 280)).toBe("crash");
    expect(at(-2, 500, 400)).toBeNull();
  });
  it("flags a flick after first crack", () => {
    expect(at(1.2, 500, 400)).toBe("flick");
    expect(at(1.2, 300)).toBeNull();
    expect(
      rorAlert({ delta: -5, sinceCharge: 700, firstCrack: 500, roastS: null, dropped: true }),
    ).toBeNull();
  });

  it("runs the development ratio live from first crack until drop", () => {
    const events = [ev("charge", 10), ev("first_crack_start", 410)];
    expect(liveDtr(events, 10, 400)).toBeNull();
    expect(liveDtr(events, 10, 510)).toEqual({ dtr: 20, devS: 100 });
    expect(liveDtr([...events, ev("drop", 510)], 10, 900)).toEqual({ dtr: 20, devS: 100 });
    expect(liveDtr([ev("charge", 10)], 10, 530)).toBeNull();
  });

  it("labels the primary button", () => {
    const t = (k: string) => k;
    expect(primaryLabel(t, { type: "mark", kind: "drop" })).toBe("Mark drop");
    expect(primaryLabel(t, { type: "start" })).toBe("Start recording");
  });
});

describe("notes and presence", () => {
  it("picks the latest AI note", () => {
    const notes = [
      { t: 10, text: "a", origin: "ai" },
      { t: 30, text: "me", origin: "human" },
      { t: 20, text: "b", origin: "ai" },
    ];
    expect(latestAiNote(notes)?.text).toBe("b");
    expect(latestAiNote([])).toBeNull();
  });

  const safety = (due: number | null, holder: string | null): SafetyStatus => ({
    emergency_stop: false,
    ai_suspended: false,
    lease_holder: holder,
    lease_remaining_s: holder ? 60 : null,
    presence_due_in_s: due,
    overheat: false,
    connected: true,
    actuators: {},
  });

  it("is only urgent while an AI holds control", () => {
    expect(presenceView(safety(-20, null))).toMatchObject({ overdue: true, urgent: false });
    expect(presenceView(safety(-20, "claude"))).toMatchObject({ overdue: true, urgent: true });
    expect(presenceView(safety(300, "claude"))).toMatchObject({ urgent: false, ask: false });
    expect(presenceView(safety(45, null)).ask).toBe(true);
    expect(presenceView(safety(null, null))).toMatchObject({ due: null, ask: false });
  });
});
