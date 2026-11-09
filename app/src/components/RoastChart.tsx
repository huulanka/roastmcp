import { useEffect, useRef, useState } from "react";
import i18n from "i18next";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

import type { ChartBand, ChartData, ChartMarker, Series } from "@/lib/chartData.ts";
import { rorRange, rorTicks } from "@/lib/chartData.ts";
import { formatClock } from "@/lib/units.ts";
import { useResolvedTheme } from "@/state/theme.ts";

/** Things drawn on top of the curves of a live roast. */
export interface ChartOverlay {
  /** The latest sample: the "now" marker and the start of the projection. */
  now: { x: number; bt: number } | null;
  /** Expected BT a short time ahead. */
  projected: { x: number; bt: number } | null;
  /** The next milestone: a target temperature with its label. */
  target: { bt: number; label: string } | null;
}

const PCT_MAX = 300;
/** Width of the following window, seconds. */
const FOLLOW_SPAN_S = 360;
/** Room ahead of the latest sample (projection and the next minutes), seconds. */
const AHEAD_S = 45;

/** How the time axis behaves: the whole roast, or a window that follows the latest sample. */
export type ChartView = "whole" | "follow";
const NO_OVERLAY: ChartOverlay = { now: null, projected: null, target: null };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "gray";
}

function uiFont(px: number): string {
  return `${px}px ${getComputedStyle(document.body).fontFamily}`;
}

function toUPlot(d: ChartData, actuators: boolean): uPlot.AlignedData {
  const empty = () => d.x.map(() => undefined);
  const cols = [d.x, d.bt, d.ror, d.refBt ?? empty(), d.refRor ?? empty()];
  if (actuators) cols.push(d.burner, d.air);
  return cols as uPlot.AlignedData;
}

export function RoastChart({
  data,
  markers,
  band = null,
  overlay = NO_OVERLAY,
  minSpanS = 0,
  actuators = false,
  view = "whole",
  label,
}: {
  data: ChartData;
  markers: ChartMarker[];
  /** Lightly tinted span, the development phase. */
  band?: ChartBand | null;
  overlay?: ChartOverlay;
  minSpanS?: number;
  /** Also draw burner and air (stepped, lower third). */
  actuators?: boolean;
  /** Whole roast (default) or a window following the latest sample. */
  view?: ChartView;
  /** Accessible summary of the chart. */
  label: string;
}) {
  const theme = useResolvedTheme();
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const tip = useRef<HTMLDivElement>(null);
  const markerRef = useRef(markers);
  const dataRef = useRef(data);
  const bandRef = useRef(band);
  const overlayRef = useRef(overlay);
  const viewRef = useRef(view);
  /** A range chosen by dragging or scrolling; null follows the view. */
  const zoomRef = useRef<[number, number] | null>(null);
  /** The view a zoom was made in; the "back" button shows only there. */
  const [zoomedIn, setZoomedIn] = useState<ChartView | null>(null);
  const zoomed = zoomedIn === view;

  const resetZoom = () => {
    zoomRef.current = null;
    setZoomedIn(null);
    const u = plot.current;
    if (u) u.setData(toUPlot(dataRef.current, actuators));
  };

  useEffect(() => {
    viewRef.current = view;
    zoomRef.current = null;
    const u = plot.current;
    if (u) u.setData(toUPlot(dataRef.current, actuators));
  }, [view, actuators]);

  useEffect(() => {
    markerRef.current = markers;
    bandRef.current = band;
    overlayRef.current = overlay;
    plot.current?.redraw(false);
  }, [markers, band, overlay]);

  useEffect(() => {
    dataRef.current = data;
    plot.current?.setData(toUPlot(data, actuators));
  }, [data, actuators]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const axis = css("--chart-axis");
    const gridColour = css("--chart-grid");
    const font = uiFont(13);
    const stepped = uPlot.paths.stepped?.({ align: 1 });
    const line = { points: { show: false } } as const;

    const series: uPlot.Series[] = [
      {},
      { ...line, scale: "bt", stroke: css("--chart-bt"), width: 2.25, cap: "round" },
      { ...line, scale: "ror", stroke: css("--chart-ror"), width: 1.75 },
      { ...line, scale: "bt", stroke: css("--chart-reference"), width: 1.5, dash: [8, 6] },
      { ...line, scale: "ror", stroke: css("--chart-reference-ror"), width: 1.5, dash: [5, 6] },
    ];
    if (actuators) {
      series.push(
        { ...line, scale: "pct", stroke: css("--chart-burner"), width: 1.25, paths: stepped },
        { ...line, scale: "pct", stroke: css("--chart-air"), width: 1.25, paths: stepped },
      );
    }

    const opts: uPlot.Options = {
      width: el.clientWidth,
      height: el.clientHeight,
      padding: [30, 6, 0, 0],
      legend: { show: false },
      // Dragging across the chart zooms to that span.
      cursor: { drag: { x: true, y: false, setScale: false }, points: { size: 7 } },
      scales: {
        x: {
          time: false,
          range: (_u, min, max) => {
            if (zoomRef.current) return zoomRef.current;
            const lo = Number.isFinite(min) ? min : 0;
            const hi = Number.isFinite(max) ? max : 0;
            const ahead = overlayRef.current.projected?.x ?? -Infinity;
            const end = Math.max(hi + 5, ahead + AHEAD_S);
            if (viewRef.current === "follow") {
              // Follow the latest live sample, not the end of a reference ahead of it.
              const now = overlayRef.current.now?.x ?? hi;
              const right = Math.max(now + 5, ahead + AHEAD_S);
              if (right - lo > FOLLOW_SPAN_S) return [right - FOLLOW_SPAN_S, right];
              return [Math.min(lo, 0), Math.max(lo + FOLLOW_SPAN_S, 60)];
            }
            return [Math.min(lo, 0), Math.max(end, minSpanS, 60)];
          },
        },
        bt: {
          range: (_u, min, max) => {
            const lo = Number.isFinite(min) ? min : 100;
            const hi = Number.isFinite(max) ? max : 200;
            return [
              Math.max(0, Math.floor((lo - 10) / 20) * 20),
              Math.max(240, Math.ceil((hi + 10) / 20) * 20),
            ];
          },
        },
        ror: { range: (_u, min, max) => rorRange(min, max) },
        pct: { range: [0, PCT_MAX] },
      },
      axes: [
        {
          stroke: axis,
          font,
          grid: { show: false },
          ticks: { show: false },
          gap: 10,
          size: 34,
          space: 70,
          incrs: [30, 60, 120, 300, 600],
          values: (_u, splits) => splits.map((s) => formatClock(s)),
        },
        {
          scale: "bt",
          stroke: axis,
          font,
          grid: { stroke: gridColour, width: 1 },
          ticks: { show: false },
          gap: 10,
          size: 52,
          incrs: [20, 40, 80],
          space: 44,
          values: (_u, splits) => splits.map((v) => `${v}°`),
        },
        {
          scale: "ror",
          side: 1,
          stroke: css("--chart-ror-axis"),
          font,
          grid: { show: false },
          ticks: { show: false },
          gap: 10,
          size: 40,
          splits: (u) => rorTicks(u.scales.ror.min ?? 0, u.scales.ror.max ?? 30),
        },
      ],
      series,
      hooks: {
        setSelect: [
          (u) => {
            const { left, width } = u.select;
            if (width > 8) {
              const a = u.posToVal(left, "x");
              const b = u.posToVal(left + width, "x");
              zoomRef.current = [Math.min(a, b), Math.max(a, b)];
              setZoomedIn(viewRef.current);
              u.setScale("x", { min: zoomRef.current[0], max: zoomRef.current[1] });
            }
            u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
          },
        ],
        setCursor: [
          (u) => {
            const t = tip.current;
            const i = u.cursor.idx;
            if (!t) return;
            if (i === null || i === undefined || u.cursor.left === undefined || u.cursor.left < 0) {
              t.style.opacity = "0";
              return;
            }
            const d = dataRef.current;
            const f = (v: number | null | undefined, n = 1) =>
              v === null || v === undefined ? "–" : v.toFixed(n);
            const row = (colour: string, name: string, value: string) =>
              `<span style="display:flex;justify-content:space-between;gap:12px"><span style="color:${colour}">${name}</span><span>${value}</span></span>`;
            const muted = css("--chart-axis");
            let html =
              `<b style="font-weight:600">${formatClock(d.x[i])}</b>` +
              row(css("--chart-bt"), i18n.t("Bean"), `${f(d.bt[i])}°`) +
              row(css("--chart-ror"), "RoR", f(d.ror[i]));
            if (d.refBt?.[i] !== undefined)
              html += row(muted, i18n.t("Reference"), `${f(d.refBt[i])}°`);
            if (actuators)
              html +=
                row(muted, i18n.t("Burner"), `${f(d.burner[i], 0)} %`) +
                row(muted, i18n.t("Air"), `${f(d.air[i], 0)} %`);
            t.innerHTML = html;
            const w = u.over.clientWidth;
            const left = u.cursor.left + 16 + 150 > w ? u.cursor.left - 166 : u.cursor.left + 16;
            t.style.transform = `translate(${left + u.over.offsetLeft}px, ${u.over.offsetTop + 8}px)`;
            t.style.opacity = "1";
          },
        ],
        drawClear: [
          (u) => {
            const b = bandRef.current;
            if (!b) return;
            const { top, height, left, width } = u.bbox;
            const x0 = Math.max(left, u.valToPos(b.from, "x", true));
            const x1 = Math.min(left + width, u.valToPos(b.to, "x", true));
            if (x1 <= x0) return;
            u.ctx.save();
            u.ctx.fillStyle = css("--chart-development");
            u.ctx.fillRect(x0, top, x1 - x0, height);
            u.ctx.restore();
          },
        ],
        draw: [
          (u) => {
            const ctx = u.ctx;
            const { top, height, left, width } = u.bbox;
            const dpr = uPlot.pxRatio;
            ctx.save();

            // RoR zero line, when the axis shows a negative rate of rise.
            const rorMin = u.scales.ror.min ?? 0;
            if (rorMin < 0) {
              const y = Math.round(u.valToPos(0, "ror", true));
              ctx.strokeStyle = css("--chart-ror-axis");
              ctx.globalAlpha = 0.5;
              ctx.lineWidth = dpr;
              ctx.setLineDash([2 * dpr, 4 * dpr]);
              ctx.beginPath();
              ctx.moveTo(left, y);
              ctx.lineTo(left + width, y);
              ctx.stroke();
              ctx.setLineDash([]);
              ctx.globalAlpha = 1;
            }

            // Event lines with their labels above the plot.
            ctx.font = uiFont(13 * dpr);
            ctx.textBaseline = "bottom";
            let labelEnd = -Infinity;
            for (const m of [...markerRef.current].sort((p, q) => p.x - q.x)) {
              const x = Math.round(u.valToPos(m.x, "x", true));
              if (x < left || x > left + width) continue;
              ctx.strokeStyle = css("--chart-event");
              ctx.lineWidth = dpr;
              ctx.setLineDash([3 * dpr, 5 * dpr]);
              ctx.beginPath();
              ctx.moveTo(x, top);
              ctx.lineTo(x, top + height);
              ctx.stroke();
              const w = ctx.measureText(m.label).width;
              const lx = Math.min(x + 6 * dpr, left + width - w);
              if (lx < labelEnd) continue;
              ctx.fillStyle = css("--chart-axis");
              ctx.fillText(m.label, lx, top - 8 * dpr);
              labelEnd = lx + w + 12 * dpr;
            }
            ctx.setLineDash([]);

            // Direct labels at the end of the burner and air lines.
            if (actuators) {
              const d = dataRef.current;
              ctx.font = uiFont(12 * dpr);
              ctx.textBaseline = "middle";
              ctx.textAlign = "left";
              ctx.fillStyle = css("--chart-axis");
              const ends: [string, Series][] = [
                [i18n.t("Burner"), d.burner],
                [i18n.t("Air"), d.air],
              ];
              const placed: number[] = [];
              for (const [name, s] of ends) {
                let i = s.length - 1;
                while (i >= 0 && (s[i] === null || s[i] === undefined)) i--;
                const v = s[i];
                if (i < 0 || typeof v !== "number") continue;
                const x = u.valToPos(d.x[i], "x", true);
                let y = u.valToPos(v, "pct", true);
                // Keep two labels apart when the lines are close.
                for (const p of placed) if (Math.abs(y - p) < 14 * dpr) y = p + 14 * dpr;
                placed.push(y);
                if (x < left || x > left + width) continue;
                ctx.fillText(
                  `${name} ${Math.round(v)} %`,
                  Math.min(x + 6 * dpr, left + width - 70 * dpr),
                  y,
                );
              }
            }

            // Projection: a short dashed segment from now, then the next milestone.
            const o = overlayRef.current;
            const accent = css("--chart-projection");
            if (o.now && o.projected) {
              const x0 = u.valToPos(o.now.x, "x", true);
              const y0 = u.valToPos(o.now.bt, "bt", true);
              const x1 = u.valToPos(o.projected.x, "x", true);
              const y1 = u.valToPos(o.projected.bt, "bt", true);
              ctx.strokeStyle = accent;
              ctx.lineWidth = 2.5 * dpr;
              ctx.lineCap = "round";
              ctx.setLineDash([6 * dpr, 6 * dpr]);
              ctx.beginPath();
              ctx.moveTo(x0, y0);
              ctx.lineTo(x1, y1);
              ctx.stroke();
              ctx.setLineDash([]);
            }
            if (o.now && o.target) {
              const from = u.valToPos(o.projected?.x ?? o.now.x, "x", true);
              const y = u.valToPos(o.target.bt, "bt", true);
              if (y >= top && y <= top + height && from < left + width) {
                ctx.strokeStyle = accent;
                ctx.globalAlpha = 0.6;
                ctx.lineWidth = dpr;
                ctx.beginPath();
                ctx.moveTo(from, y);
                ctx.lineTo(left + width, y);
                ctx.stroke();
                ctx.globalAlpha = 1;
                ctx.fillStyle = css("--brand-text");
                ctx.font = uiFont(13 * dpr);
                ctx.textAlign = "right";
                ctx.textBaseline = "bottom";
                ctx.fillText(o.target.label, left + width - 2 * dpr, y - 6 * dpr);
                ctx.textAlign = "left";
              }
            }
            if (o.now) {
              const x = u.valToPos(o.now.x, "x", true);
              const y = u.valToPos(o.now.bt, "bt", true);
              if (x >= left && x <= left + width && y >= top && y <= top + height) {
                ctx.fillStyle = css("--chart-now-halo");
                ctx.beginPath();
                ctx.arc(x, y, 13 * dpr, 0, Math.PI * 2);
                ctx.fill();
                ctx.fillStyle = accent;
                ctx.beginPath();
                ctx.arc(x, y, 5.5 * dpr, 0, Math.PI * 2);
                ctx.fill();
              }
            }
            ctx.restore();
          },
        ],
      },
    };
    const u = new uPlot(opts, toUPlot(dataRef.current, actuators), el);
    plot.current = u;
    // Scrolling sideways (trackpad, shift + wheel) moves the visible span
    // back and forth; it then stays put until "back" is pressed.
    const onWheel = (e: WheelEvent) => {
      const dx = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.shiftKey ? e.deltaY : 0;
      if (dx === 0) return;
      e.preventDefault();
      const min = u.scales.x.min ?? 0;
      const max = u.scales.x.max ?? 0;
      const span = max - min;
      const d = dataRef.current;
      const first = d.x[0] ?? 0;
      const last = d.x[d.x.length - 1] ?? span;
      let shift = (dx / Math.max(1, u.bbox.width / uPlot.pxRatio)) * span;
      shift = Math.max(Math.min(shift, last + AHEAD_S - max), Math.min(0, first - min));
      zoomRef.current = [min + shift, max + shift];
      setZoomedIn(viewRef.current);
      u.setScale("x", { min: zoomRef.current[0], max: zoomRef.current[1] });
    };
    u.over.addEventListener("wheel", onWheel, { passive: false });
    const ro = new ResizeObserver(() => {
      u.setSize({ width: el.clientWidth, height: el.clientHeight });
    });
    ro.observe(el);
    // Canvas text needs the web font; draw again once it is there.
    void document.fonts?.ready.then(() => plot.current === u && u.redraw(false));
    return () => {
      ro.disconnect();
      u.over.removeEventListener("wheel", onWheel);
      u.destroy();
      plot.current = null;
    };
  }, [minSpanS, actuators, theme]);

  return (
    <div className="relative size-full min-h-0 min-w-0">
      <div ref={host} className="absolute inset-0" role="img" aria-label={label} />
      {zoomed && (
        <button
          type="button"
          onClick={resetZoom}
          className="absolute top-9 right-12 z-10 rounded-pill border border-border-strong bg-popover px-3 py-1 text-[13px] text-foreground shadow-pop transition-colors hover:bg-surface-raised focus-visible:ring-3 focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          {view === "follow" ? i18n.t("Back to live") : i18n.t("Whole roast")}
        </button>
      )}
      <div
        ref={tip}
        className="pointer-events-none absolute top-0 left-0 z-10 flex w-[150px] flex-col gap-1 rounded-lg border border-border-strong bg-popover px-3 py-2.5 tabular text-[13px] text-foreground opacity-0 shadow-pop transition-opacity duration-150"
      />
    </div>
  );
}
