import { useEffect, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";

import { formatClock } from "@/lib/units.ts";

export interface ChartData {
  /** Seconds relative to CHARGE (or recording start). */
  x: number[];
  bt: (number | null)[];
  ror: (number | null)[];
  burner: (number | null)[];
  air: (number | null)[];
}
export interface ChartMarker {
  x: number;
  label: string;
}

export interface ChartBand {
  from: number;
  to: number;
  color: string;
}

export const ROR_MIN = -10;
export const ROR_MAX = 40;
/** The percent scale is stretched so 0–100 % fills only the lower third of the plot. */
const PCT_MAX = 300;

function css(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888";
}

function toUPlot(d: ChartData): uPlot.AlignedData {
  return [
    d.x,
    d.bt,
    d.ror.map((v) => (v === null ? null : Math.min(ROR_MAX, Math.max(ROR_MIN, v)))),
    d.burner,
    d.air,
  ];
}

export function RoastChart({
  data,
  markers,
  bands = [],
  minSpanS = 0,
}: {
  data: ChartData;
  markers: ChartMarker[];
  bands?: ChartBand[];
  minSpanS?: number;
}) {
  const host = useRef<HTMLDivElement>(null);
  const plot = useRef<uPlot | null>(null);
  const markerRef = useRef<ChartMarker[]>(markers);
  const dataRef = useRef<ChartData>(data);
  const bandRef = useRef<ChartBand[]>(bands);
  const tip = useRef<HTMLDivElement>(null);

  useEffect(() => {
    markerRef.current = markers;
    bandRef.current = bands;
    plot.current?.redraw(false);
  }, [markers, bands]);

  useEffect(() => {
    dataRef.current = data;
    plot.current?.setData(toUPlot(data));
  }, [data]);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const axisColor = css("--chart-axis");
    const grid = { stroke: css("--chart-grid"), width: 1 };
    const font = "12px " + getComputedStyle(document.body).fontFamily;
    const stepped = uPlot.paths.stepped?.({ align: 1 });
    const opts: uPlot.Options = {
      width: el.clientWidth,
      height: el.clientHeight,
      padding: [18, 4, 0, 0],
      legend: { show: false },
      cursor: { drag: { x: false, y: false }, points: { size: 6 } },
      scales: {
        x: {
          time: false,
          range: (_u, min, max) => {
            const lo = Number.isFinite(min) ? min : 0;
            const hi = Number.isFinite(max) ? max : 0;
            return [Math.min(lo, 0), Math.max(hi + 5, minSpanS, 60)];
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
          stroke: axisColor,
          font,
          grid,
          ticks: { stroke: grid.stroke },
          incrs: [15, 30, 60, 120, 300],
          values: (_u, splits) => splits.map((s) => formatClock(s)),
        },
        { scale: "bt", stroke: axisColor, font, grid, ticks: { stroke: grid.stroke }, size: 48 },
        {
          scale: "ror",
          side: 1,
          stroke: css("--series-ror"),
          font,
          grid: { show: false },
          ticks: { stroke: grid.stroke },
          size: 40,
          splits: () => [0, 10, 20, 30, 40],
        },
        {
          scale: "pct",
          side: 1,
          stroke: axisColor,
          font,
          grid: { show: false },
          ticks: { stroke: grid.stroke },
          size: 44,
          splits: () => [0, 50, 100],
          values: (_u, splits) => splits.map((v) => `${v}%`),
        },
      ],
      series: [
        {},
        {
          scale: "bt",
          stroke: css("--series-bt"),
          width: 2.5,
          points: { show: false },
          fill: (u) => {
            const g = u.ctx.createLinearGradient(0, u.bbox.top, 0, u.bbox.top + u.bbox.height);
            g.addColorStop(0, css("--series-bt") + "40");
            g.addColorStop(1, css("--series-bt") + "00");
            return g;
          },
        },
        { scale: "ror", stroke: css("--series-ror"), width: 1.5, points: { show: false } },
        {
          scale: "pct",
          stroke: css("--series-burner"),
          width: 1.5,
          paths: stepped,
          points: { show: false },
        },
        {
          scale: "pct",
          stroke: css("--series-air"),
          width: 1.5,
          paths: stepped,
          points: { show: false },
        },
      ],
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
            t.innerHTML =
              `<b>${formatClock(d.x[i])}</b>` +
              `<span style="color:${css("--series-bt")}">BT ${f(d.bt[i])}°C</span>` +
              `<span style="color:${css("--series-ror")}">RoR ${f(d.ror[i])}</span>` +
              `<span style="color:${css("--series-burner")}">Burner ${f(d.burner[i], 0)}%</span>` +
              `<span style="color:${css("--series-air")}">Air ${f(d.air[i], 0)}%</span>`;
            const w = u.over.clientWidth;
            const left = u.cursor.left + 14 + 130 > w ? u.cursor.left - 144 : u.cursor.left + 14;
            t.style.transform = `translate(${left}px, 8px)`;
            t.style.opacity = "1";
          },
        ],
        drawClear: [
          (u) => {
            const ctx = u.ctx;
            const { top, height, left, width } = u.bbox;
            ctx.save();
            for (const b of bandRef.current) {
              const x0 = Math.max(left, u.valToPos(b.from, "x", true));
              const x1 = Math.min(left + width, u.valToPos(b.to, "x", true));
              if (x1 <= x0) continue;
              ctx.fillStyle = b.color;
              ctx.fillRect(x0, top, x1 - x0, height);
            }
            ctx.restore();
          },
        ],
        draw: [
          (u) => {
            const ctx = u.ctx;
            const { top, height } = u.bbox;
            const dpr = devicePixelRatio;
            ctx.save();
            ctx.font = `${11 * dpr}px ${getComputedStyle(document.body).fontFamily}`;
            ctx.textBaseline = "top";
            const rowEnd = [-1e9, -1e9, -1e9];
            const sorted = [...markerRef.current].sort((p, q) => p.x - q.x);
            for (const m of sorted) {
              const x = Math.round(u.valToPos(m.x, "x", true));
              if (x < u.bbox.left || x > u.bbox.left + u.bbox.width) continue;
              ctx.strokeStyle = css("--chart-event");
              ctx.fillStyle = css("--chart-axis");
              ctx.lineWidth = dpr;
              ctx.setLineDash([4 * dpr, 4 * dpr]);
              ctx.beginPath();
              ctx.moveTo(x, top);
              ctx.lineTo(x, top + height);
              ctx.stroke();
              ctx.setLineDash([]);
              const w = ctx.measureText(m.label).width;
              const row = rowEnd.findIndex((e) => x + 3 * dpr >= e);
              if (row < 0) continue;
              rowEnd[row] = x + 3 * dpr + w + 6 * dpr;
              ctx.fillText(m.label, x + 3 * dpr, top + (2 + row * 14) * dpr);
            }
            // top of the 0-100 % band
            const y100 = Math.round(u.valToPos(100, "pct", true));
            ctx.strokeStyle = css("--chart-grid");
            ctx.lineWidth = dpr;
            ctx.beginPath();
            ctx.moveTo(u.bbox.left, y100);
            ctx.lineTo(u.bbox.left + u.bbox.width, y100);
            ctx.stroke();
            ctx.restore();
          },
        ],
      },
    };
    const u = new uPlot(opts, toUPlot(dataRef.current), el);
    plot.current = u;
    const ro = new ResizeObserver(() => {
      u.setSize({ width: el.clientWidth, height: el.clientHeight });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      u.destroy();
      plot.current = null;
    };
  }, [minSpanS]);

  return (
    <div className="relative size-full min-h-0 min-w-0">
      <div ref={host} className="size-full" />
      <div
        ref={tip}
        className="pointer-events-none absolute top-0 left-0 flex w-[130px] flex-col gap-0.5 rounded-lg border border-border-strong bg-popover/90 px-2.5 py-2 tabular text-xs opacity-0 shadow-lg backdrop-blur transition-opacity duration-150"
        style={{ marginLeft: 48 }}
      />
    </div>
  );
}
