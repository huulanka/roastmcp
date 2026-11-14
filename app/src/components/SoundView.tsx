import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import type { Snapshot, SoundPop } from "@/api/types.ts";
import { EVENT_NAME } from "@/lib/chartData.ts";
import { FC_POPS, freqPos, popsWithin } from "@/lib/hearing.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { soundBuffer, subscribeSound, type SoundBuffer } from "@/state/sound.ts";

/** Seconds of sound shown. */
const SPAN_S = 60;
/** The spectrogram's colours, quiet to loud: roasted brown to cream. */
const STOPS: [number, [number, number, number]][] = [
  [0, [26, 17, 12]],
  [0.35, [74, 46, 29]],
  [0.65, [196, 69, 0]],
  [0.85, [255, 138, 61]],
  [1, [255, 236, 214]],
];

function colour(x: number): [number, number, number] {
  for (let i = 1; i < STOPS.length; i++) {
    const [p1, c1] = STOPS[i];
    const [p0, c0] = STOPS[i - 1];
    if (x <= p1) {
      const f = (x - p0) / (p1 - p0);
      return [0, 1, 2].map((k) => Math.round(c0[k] + (c1[k] - c0[k]) * f)) as [
        number,
        number,
        number,
      ];
    }
  }
  return STOPS[STOPS.length - 1][1];
}
const LUT = Array.from({ length: 256 }, (_, i) => colour(i / 255));

/** The 5th and 99.5th percentile of the shown levels: the colour range. */
function range(columns: readonly Uint8Array[]): [number, number] {
  const hist = new Uint32Array(256);
  let n = 0;
  for (const c of columns)
    for (const v of c) {
      hist[v]++;
      n++;
    }
  if (n === 0) return [0, 255];
  const at = (q: number) => {
    let acc = 0;
    for (let v = 0; v < 256; v++) {
      acc += hist[v];
      if (acc >= q * n) return v;
    }
    return 255;
  };
  const lo = at(0.05);
  return [lo, Math.max(lo + 24, at(0.995))];
}

interface Marks {
  pops: readonly SoundPop[];
  heard: { kind: string; t: number; label: string }[];
  base: number;
}

function draw(canvas: HTMLCanvasElement, buf: SoundBuffer, marks: Marks, labels: Labels): void {
  const dpr = window.devicePixelRatio || 1;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (w === 0 || h === 0) return;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = "#1a110c";
  ctx.fillRect(0, 0, w, h);

  const left = 64;
  const right = w - 16;
  const top = 16;
  const rateH = Math.max(48, Math.min(70, h * 0.16));
  const axisH = 26;
  const specBottom = h - axisH - rateH - 14;
  const specH = specBottom - top;
  const plotW = right - left;
  if (plotW <= 0 || specH <= 20) return;
  const tEnd = buf.t ?? 0;
  const tStart = tEnd - SPAN_S;
  const xOf = (t: number) => left + ((t - tStart) / SPAN_S) * plotW;
  const yOf = (hz: number) => specBottom - freqPos(hz) * specH;

  // The spectrogram, newest column at the right edge.
  const cols = buf.columns;
  if (cols.length > 0 && buf.t !== null) {
    const shown = Math.min(cols.length, Math.ceil(SPAN_S * buf.perS));
    const from = cols.length - shown;
    const img = new ImageData(shown, buf.bands);
    const [lo, hi] = range(cols.slice(from));
    for (let x = 0; x < shown; x++) {
      const c = cols[from + x];
      for (let b = 0; b < buf.bands; b++) {
        const v = Math.min(255, Math.max(0, Math.round(((c[b] - lo) / (hi - lo)) * 255)));
        const [r, g, bl] = LUT[v];
        const i = ((buf.bands - 1 - b) * shown + x) * 4;
        img.data[i] = r;
        img.data[i + 1] = g;
        img.data[i + 2] = bl;
        img.data[i + 3] = 255;
      }
    }
    const off = document.createElement("canvas");
    off.width = shown;
    off.height = buf.bands;
    off.getContext("2d")?.putImageData(img, 0, 0);
    const x0 = xOf(tEnd - shown / buf.perS);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(off, x0, top, right - x0, specH);
  }

  // The two bands.
  const shade = (lo: number, hi: number, fill: string, text: string, label: string) => {
    const y0 = yOf(hi);
    const y1 = yOf(lo);
    ctx.fillStyle = fill;
    ctx.fillRect(left, y0, plotW, y1 - y0);
    ctx.fillStyle = text;
    ctx.font = "12px system-ui, sans-serif";
    ctx.textBaseline = "top";
    ctx.fillText(label, left + 8, y0 + 4);
  };
  shade(400, 4000, "rgba(196,69,0,0.12)", "#f0b48a", labels.crackBand);
  shade(6000, 16000, "rgba(0,114,184,0.10)", "#8fc3e6", labels.highBand);

  // Frequency axis.
  ctx.fillStyle = "#d9c7b8";
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "right";
  for (const [hz, name] of [
    [16000, "16 kHz"],
    [4000, "4 kHz"],
    [1000, "1 kHz"],
    [400, "400 Hz"],
    [100, "100 Hz"],
  ] as const) {
    ctx.fillText(name, left - 8, Math.min(specBottom - 6, Math.max(top + 6, yOf(hz))));
  }
  ctx.textAlign = "left";

  // Rate of crack-band pops (20 s window) against the threshold.
  const rTop = specBottom + 14;
  const rBottom = rTop + rateH;
  ctx.fillStyle = "#22170f";
  ctx.fillRect(left, rTop, plotW, rateH);
  const maxRate = 15;
  const yRate = (n: number) => rBottom - (Math.min(n, maxRate) / maxRate) * (rateH - 6);
  ctx.strokeStyle = "#ff8a3d";
  ctx.setLineDash([4, 4]);
  ctx.globalAlpha = 0.8;
  ctx.beginPath();
  ctx.moveTo(left, yRate(FC_POPS));
  ctx.lineTo(right, yRate(FC_POPS));
  ctx.stroke();
  ctx.setLineDash([]);
  ctx.globalAlpha = 1;
  ctx.fillStyle = "#f0b48a";
  ctx.font = "11px system-ui, sans-serif";
  ctx.textBaseline = "bottom";
  ctx.fillText(labels.threshold, left + 6, yRate(FC_POPS) - 3);
  if (buf.t !== null) {
    ctx.strokeStyle = "#ffe6cc";
    ctx.lineWidth = 2;
    ctx.beginPath();
    for (let i = 0; i <= 120; i++) {
      const t = tStart + (i / 120) * SPAN_S;
      const n = popsWithin(marks.pops, "crack", t, 20);
      const x = xOf(t);
      if (i === 0) ctx.moveTo(x, yRate(n));
      else ctx.lineTo(x, yRate(n));
    }
    ctx.stroke();
    ctx.lineWidth = 1;
  }

  // Pops as ticks under the spectrogram.
  for (const p of marks.pops) {
    if (p.t < tStart || p.t > tEnd) continue;
    const x = xOf(p.t);
    ctx.fillStyle = p.band === "crack" ? "#ff8a3d" : "#8fc3e6";
    ctx.fillRect(x - 1, specBottom + 2, 2, 9);
  }

  // Heard cracks.
  for (const m of marks.heard) {
    if (m.t < tStart || m.t > tEnd) continue;
    const x = xOf(m.t);
    ctx.strokeStyle = "#ff8a3d";
    ctx.lineWidth = 2;
    ctx.setLineDash([6, 5]);
    ctx.beginPath();
    ctx.moveTo(x, top);
    ctx.lineTo(x, rBottom);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.lineWidth = 1;
    ctx.font = "600 13px system-ui, sans-serif";
    const tw = ctx.measureText(m.label).width;
    const bx = Math.min(x + 8, right - tw - 20);
    ctx.fillStyle = "#1a110c";
    ctx.strokeStyle = "#ff8a3d";
    ctx.beginPath();
    ctx.roundRect(bx, top + 8, tw + 20, 26, 8);
    ctx.fill();
    ctx.stroke();
    ctx.fillStyle = "#ffffff";
    ctx.textBaseline = "middle";
    ctx.fillText(m.label, bx + 10, top + 21);
  }

  // Time axis on the roast clock.
  ctx.fillStyle = "#d9c7b8";
  ctx.font = "12px system-ui, sans-serif";
  ctx.textBaseline = "top";
  ctx.textAlign = "center";
  const step = 15;
  for (let s = Math.ceil((tStart - marks.base) / step) * step; s <= tEnd - marks.base; s += step) {
    ctx.fillText(formatClock(s), xOf(s + marks.base), rBottom + 8);
  }
  ctx.textAlign = "left";
  if (buf.t === null) {
    ctx.fillStyle = "#d9c7b8";
    ctx.font = "14px system-ui, sans-serif";
    ctx.textBaseline = "middle";
    ctx.textAlign = "center";
    ctx.fillText(labels.waiting, left + plotW / 2, top + specH / 2);
    ctx.textAlign = "left";
  }
}

interface Labels {
  crackBand: string;
  highBand: string;
  threshold: string;
  waiting: string;
}

/**
 * The microphone as a live spectrogram of the last minute, with the pops,
 * their rate and the moment a crack was heard (ADR 0021 §8). Only to watch:
 * detection runs the same with the curve shown.
 */
export function SoundView({
  snapshot,
  pops,
  base,
}: {
  snapshot: Snapshot;
  pops: readonly SoundPop[];
  base: number | null;
}) {
  const { t } = useTranslation();
  const canvas = useRef<HTMLCanvasElement>(null);
  const audio = snapshot.audio;
  const heard = (audio?.heard ?? []).map((h) => {
    const ev = snapshot.events.find((e) => e.kind === h.kind);
    const state =
      ev?.origin === "detector" ? t("marked") : ev ? t("you marked it") : t("not marked");
    return {
      kind: h.kind,
      t: h.t,
      label: `${t(EVENT_NAME[h.kind])} ${formatClock(h.t - (base ?? 0))} · ${state}`,
    };
  });
  const marks = useRef<Marks>({ pops, heard, base: base ?? 0 });
  const labels = useRef<Labels>({
    crackBand: t("Crack band 0.4–4 kHz · first crack"),
    highBand: t("High band 6–16 kHz · second crack"),
    threshold: t("from {{n}} pops in 20 s", { n: FC_POPS }),
    waiting: t("Waiting for sound…"),
  });
  useEffect(() => {
    marks.current = { pops, heard, base: base ?? 0 };
    labels.current = {
      crackBand: t("Crack band 0.4–4 kHz · first crack"),
      highBand: t("High band 6–16 kHz · second crack"),
      threshold: t("from {{n}} pops in 20 s", { n: FC_POPS }),
      waiting: t("Waiting for sound…"),
    };
  });

  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    let frame = 0;
    const paint = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => draw(el, soundBuffer(), marks.current, labels.current));
    };
    paint();
    const unsub = subscribeSound(paint);
    const ro = new ResizeObserver(paint);
    ro.observe(el);
    return () => {
      cancelAnimationFrame(frame);
      unsub();
      ro.disconnect();
    };
  }, []);

  const high = audio?.high_10s ?? 0;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <canvas
        ref={canvas}
        role="img"
        aria-label={t("Spectrogram of the last 60 seconds: {{n}} pops in 20 s", {
          n: audio?.crack_20s ?? 0,
        })}
        className="min-h-56 w-full flex-1 rounded-2xl"
      />
      <div className="flex flex-wrap items-start gap-x-8 gap-y-2">
        <div className="flex flex-col gap-0.5">
          <span className="text-[13px] text-muted-foreground">{t("Pops in 20 s")}</span>
          <span className="tabular text-xl font-medium text-brand-text">
            {audio?.crack_20s ?? 0}
          </span>
        </div>
        <div className="flex flex-col gap-0.5">
          <span className="text-[13px] text-muted-foreground">{t("High band")}</span>
          <span className="tabular text-xl font-medium">
            {high > 0 ? t("{{n}} in 10 s", { n: high }) : t("quiet")}
          </span>
        </div>
        <div className="flex flex-col gap-0.5">
          <span className="text-[13px] text-muted-foreground">{t("Above the fan")}</span>
          <span className="tabular text-xl font-medium">
            {typeof audio?.pop_db === "number" ? `+${formatNumber(audio.pop_db, 0)} dB` : "–"}
          </span>
        </div>
        <div className="flex flex-col gap-0.5">
          <span className="text-[13px] text-muted-foreground">{t("Crack band level")}</span>
          <span className="tabular text-xl font-medium">
            {typeof audio?.crack_db === "number" ? `${formatNumber(audio.crack_db, 0)} dBFS` : "–"}
          </span>
        </div>
        <p className="max-w-80 min-w-48 flex-1 text-[13px] text-muted-foreground">
          {t(
            "Only to watch. Detection runs the same while the curve is shown; sound is only kept in memory.",
          )}
        </p>
      </div>
    </div>
  );
}
