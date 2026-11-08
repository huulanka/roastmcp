import { useEffect, useRef } from "react";
import i18n from "i18next";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

import type { ChartBand, ChartData, ChartMarker } from "@/lib/chartData.ts";
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

export const ROR_MIN = 0;
export const ROR_MAX = 30;
const PCT_MAX = 300;
const NO_OVERLAY: ChartOverlay = { now: null, projected: null, target: null };

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "gray";
}

function uiFont(px: number): string {
  return `${px}px ${getComputedStyle(document.body).fontFamily}`;
}

function toUPlot(d: ChartData, actuators: boolean): uPlot.AlignedData {
  const empty = () => d.x.map(() => undefined);
  const ror = d.ror.map((v) =>
    v === null || v === undefined ? v : Math.min(ROR_MAX + 2, Math.max(ROR_MIN - 2, v)),
  );
  const cols = [d.x, d.bt, ror, d.refBt ?? empty(), d.refRor ?? empty()];
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
      cursor: { drag: { x: false, y: false }, points: { size: 7 } },
      scales: {
        x: {
          time: false,
          range: (_u, min, max) => {
            const lo = Number.isFinite(min) ? min : 0;
            const hi = Number.isFinite(max) ? max : 0;
            const ahead = overlayRef.current.projected?.x ?? -Infinity;
            return [Math.min(lo, 0), Math.max(hi + 5, ahead + 45, minSpanS, 60)];
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
        ror: { range: [ROR_MIN, ROR_MAX] },
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
          splits: () => [5, 10, 15, 20, 25],
        },
      ],
      series,
      hooks: {
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
    const ro = new ResizeObserver(() => {
      u.setSize({ width: el.clientWidth, height: el.clientHeight });
    });
    ro.observe(el);
    // Canvas text needs the web font; draw again once it is there.
    void document.fonts?.ready.then(() => plot.current === u && u.redraw(false));
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
  }, [minSpanS, actuators, theme]);

  return (
    <div className="relative size-full min-h-0 min-w-0" role="img" aria-label={label}>
      <div ref={host} className="absolute inset-0" />
      <div
        ref={tip}
        className="pointer-events-none absolute top-0 left-0 z-10 flex w-[150px] flex-col gap-1 rounded-lg border border-border-strong bg-popover px-3 py-2.5 tabular text-[13px] text-foreground opacity-0 shadow-pop transition-opacity duration-150"
      />
    </div>
  );
}
