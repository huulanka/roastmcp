import { useSyncExternalStore } from "react";

import type { AudioFrame } from "@/api/types.ts";

/** How much of the spectrogram is kept, seconds of roast time. */
export const SOUND_KEEP_S = 75;

export interface SoundBuffer {
  /** Spectrogram columns, oldest first; each `bands` bytes. */
  columns: Uint8Array[];
  bands: number;
  /** Columns per second of roast time. */
  perS: number;
  /** Recording time of the newest column; null before the first frame. */
  t: number | null;
  /** Latest band levels, dBFS. */
  crackDb: number | null;
  highDb: number | null;
  /** Bumped on every frame, for subscribers that draw. */
  version: number;
}

let buffer: SoundBuffer = {
  columns: [],
  bands: 64,
  perS: 20,
  t: null,
  crackDb: null,
  highDb: null,
  version: 0,
};
const listeners = new Set<() => void>();

function notify(): void {
  listeners.forEach((l) => l());
}

function decode(base64: string): Uint8Array {
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

/** Takes a WebSocket `audio` frame. */
export function pushSoundFrame(f: AudioFrame): void {
  const bytes = f.spectrum ? decode(f.spectrum) : new Uint8Array();
  const columns = buffer.columns.slice();
  for (let i = 0; i < f.columns; i++) columns.push(bytes.subarray(i * f.bands, (i + 1) * f.bands));
  const keep = Math.ceil(SOUND_KEEP_S * f.per_s);
  if (columns.length > keep) columns.splice(0, columns.length - keep);
  buffer = {
    columns,
    bands: f.bands,
    perS: f.per_s,
    t: f.t,
    crackDb: f.crack_db,
    highDb: f.high_db,
    version: buffer.version + 1,
  };
  notify();
}

/** A new recording: the old sound goes. */
export function resetSound(): void {
  buffer = {
    ...buffer,
    columns: [],
    t: null,
    crackDb: null,
    highDb: null,
    version: buffer.version + 1,
  };
  notify();
}

export function soundBuffer(): SoundBuffer {
  return buffer;
}

export function subscribeSound(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useSound(): SoundBuffer {
  return useSyncExternalStore(subscribeSound, soundBuffer);
}
