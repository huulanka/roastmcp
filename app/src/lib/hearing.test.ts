import { describe, expect, it } from "vitest";

import type { AudioStatus, RoastEvent } from "@/api/types.ts";
import { freqPos, heardBanner, heardVsMarked, micLook, popsWithin } from "@/lib/hearing.ts";

const audio = (patch: Partial<AudioStatus> = {}): AudioStatus => ({
  mode: "mark",
  state: "listening",
  device: "MacBook Pro Microphone",
  rate: 48000,
  error: null,
  crack_20s: 0,
  high_10s: 0,
  crack_db: null,
  high_db: null,
  simulated: false,
  heard: [],
  ...patch,
});
const ev = (kind: RoastEvent["kind"], t: number, origin: RoastEvent["origin"]): RoastEvent => ({
  kind,
  t,
  origin,
  bt: 195,
});

describe("heardBanner", () => {
  const heard = [{ kind: "first_crack_start" as const, t: 460, bt: 195.1, pops: 7 }];

  it("announces a heard and marked first crack, Enter only at first", () => {
    const b = heardBanner(
      { t: 480, events: [ev("first_crack_start", 460, "detector")], audio: audio({ heard }) },
      new Set(),
    );
    expect(b).toMatchObject({ kind: "first_crack_start", marked: true, enter: true, pops: 7 });
    const later = heardBanner(
      { t: 520, events: [ev("first_crack_start", 460, "detector")], audio: audio({ heard }) },
      new Set(),
    );
    expect(later?.enter).toBe(false);
  });

  it("goes after 90 s, once dismissed, or when a person marked it", () => {
    const events = [ev("first_crack_start", 460, "detector")];
    expect(heardBanner({ t: 560, events, audio: audio({ heard }) }, new Set())).toBeNull();
    const b = heardBanner({ t: 470, events, audio: audio({ heard }) }, new Set());
    expect(heardBanner({ t: 470, events, audio: audio({ heard }) }, new Set([b!.key]))).toBeNull();
    expect(
      heardBanner(
        { t: 470, events: [ev("first_crack_start", 465, "human")], audio: audio({ heard }) },
        new Set(),
      ),
    ).toBeNull();
  });

  it("shows a crack heard in show mode as not marked", () => {
    const b = heardBanner({ t: 470, events: [], audio: audio({ heard, mode: "show" }) }, new Set());
    expect(b?.marked).toBe(false);
  });
});

describe("hearing helpers", () => {
  it("counts the pops of a band in a window", () => {
    const pops = [
      { t: 1, band: "crack" as const, db: 5 },
      { t: 15, band: "crack" as const, db: 5 },
      { t: 16, band: "high" as const, db: 5 },
      { t: 20, band: "crack" as const, db: 5 },
    ];
    expect(popsWithin(pops, "crack", 20, 20)).toBe(3);
    expect(popsWithin(pops, "crack", 20, 10)).toBe(2);
    expect(popsWithin(pops, "high", 20, 10)).toBe(1);
  });

  it("tells the pill what to show", () => {
    expect(micLook(undefined, [])).toBe("off");
    expect(micLook(audio({ state: "silent" }), [])).toBe("problem");
    expect(micLook(audio({ state: "idle" }), [])).toBe("idle");
    expect(micLook(audio(), [ev("first_crack_start", 1, "detector")])).toBe("heard");
    expect(micLook(audio(), [ev("first_crack_start", 1, "human")])).toBe("listening");
  });

  it("places frequencies on the log axis", () => {
    expect(freqPos(100)).toBe(0);
    expect(freqPos(16000)).toBeCloseTo(1);
    expect(freqPos(1265)).toBeCloseTo(0.5, 2);
  });
});

describe("heardVsMarked", () => {
  it("compares the heard first crack with the mark", () => {
    const sound = {
      pops: [],
      levels: [],
      heard: [{ kind: "first_crack_start" as const, t: 470, bt: 195, pops: 7 }],
    };
    const events = [ev("charge", 10, "human"), ev("first_crack_start", 472, "human")];
    expect(heardVsMarked({ events, sound })).toEqual({
      heardS: 460,
      markedS: 462,
      same: false,
      pops: 7,
    });
    expect(heardVsMarked({ events, sound: null })).toBeNull();
  });
});
