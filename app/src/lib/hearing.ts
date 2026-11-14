import type { AudioStatus, EventKind, HeardMark, Roast, Snapshot, SoundPop } from "@/api/types.ts";

/** A heard crack is announced in a banner this long, seconds of roast time. */
export const BANNER_S = 90;
/** For this long Enter marks the heard crack anew ("Mark now ↵"), seconds. */
export const ENTER_S = 45;
/** First crack: this many crack-band pops in 20 s (normal sensitivity). */
export const FC_POPS = 5;

/** The kinds a banner announces. */
const ANNOUNCED: EventKind[] = ["first_crack_start", "second_crack_start"];

export interface HeardBanner {
  kind: EventKind;
  /** Recording time of the first pop of the series. */
  t: number;
  bt: number | null;
  pops: number;
  /** The heard crack is the roast's mark (mode "mark"); otherwise only shown. */
  marked: boolean;
  /** Enter marks it now. */
  enter: boolean;
  /** Identity, for dismissing. */
  key: string;
}

/**
 * The newest heard crack worth a banner: heard less than 90 s ago, not
 * marked by a person or an AI since, not dismissed.
 */
export function heardBanner(
  snapshot: Pick<Snapshot, "t" | "events" | "audio">,
  dismissed: ReadonlySet<string>,
): HeardBanner | null {
  const now = snapshot.t;
  const heard = snapshot.audio?.heard ?? [];
  if (now === null || heard.length === 0) return null;
  const latest = heard
    .filter((h) => ANNOUNCED.includes(h.kind))
    .reduce<HeardMark | null>((a, b) => (a === null || b.t > a.t ? b : a), null);
  if (!latest) return null;
  const age = now - latest.t;
  if (age > BANNER_S) return null;
  const key = `${latest.kind}@${latest.t.toFixed(1)}`;
  if (dismissed.has(key)) return null;
  const ev = snapshot.events.find((e) => e.kind === latest.kind);
  if (ev && ev.origin !== "detector") return null;
  return {
    kind: latest.kind,
    t: latest.t,
    bt: latest.bt ?? null,
    pops: latest.pops,
    marked: ev?.origin === "detector",
    enter: age <= ENTER_S,
    key,
  };
}

/** Pops of a band within `[to − seconds, to]`. */
export function popsWithin(
  pops: readonly SoundPop[],
  band: SoundPop["band"],
  to: number,
  seconds: number,
): number {
  let n = 0;
  for (let i = pops.length - 1; i >= 0; i--) {
    const p = pops[i];
    if (p.t > to) continue;
    if (p.t < to - seconds) break;
    if (p.band === band) n++;
  }
  return n;
}

/** What the microphone pill says. */
export type MicLook = "off" | "idle" | "starting" | "listening" | "heard" | "problem";

export function micLook(audio: AudioStatus | undefined, events: Snapshot["events"]): MicLook {
  if (!audio || audio.state === "off") return "off";
  if (audio.state === "silent" || audio.state === "error") return "problem";
  if (audio.state === "idle") return "idle";
  if (audio.state === "starting") return "starting";
  if (events.some((e) => e.kind === "first_crack_start" && e.origin === "detector")) return "heard";
  return "listening";
}

/** y of a frequency on the spectrogram's log axis, 0 (100 Hz) … 1 (16 kHz). */
export function freqPos(hz: number): number {
  return Math.log(hz / 100) / Math.log(16000 / 100);
}

/** First crack of a saved roast as heard and as marked, seconds since CHARGE. */
export interface HeardVsMarked {
  heardS: number;
  /** The roast's first crack mark; null when there is none. */
  markedS: number | null;
  /** The mark is the heard one (nobody marked it by hand). */
  same: boolean;
  pops: number;
}

export function heardVsMarked(roast: Pick<Roast, "events" | "sound">): HeardVsMarked | null {
  const heard = roast.sound?.heard?.find((h) => h.kind === "first_crack_start");
  if (!heard) return null;
  const events = roast.events ?? [];
  const base = events.find((e) => e.kind === "charge")?.t ?? 0;
  const mark = events.find((e) => e.kind === "first_crack_start");
  return {
    heardS: heard.t - base,
    markedS: mark ? mark.t - base : null,
    same: mark?.origin === "detector",
    pops: heard.pops,
  };
}
