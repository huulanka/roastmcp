import type { ChartBand, ChartData, ChartMarker } from "@/components/RoastChart.tsx";
import type { EventKind, Point, RoastEvent } from "@/api/types.ts";
import { computeRor } from "./ror.ts";

export const EVENT_LABEL: Record<EventKind, string> = {
  charge: "CHARGE",
  turning_point: "TP",
  dry_end: "DE",
  first_crack_start: "FCs",
  first_crack_end: "FCe",
  second_crack_start: "SCs",
  second_crack_end: "SCe",
  drop: "DROP",
  cool_end: "COOL",
};

export function chargeTime(events: readonly RoastEvent[]): number | null {
  return events.find((e) => e.kind === "charge")?.t ?? null;
}

export function buildChartData(points: readonly Point[], base: number | null): ChartData {
  const off = base ?? 0;
  const x = points.map((p) => p.t - off);
  const bt = points.map((p) => p.bt);
  const hasRor = points.some((p) => p.ror !== null && p.ror !== undefined);
  const ror = hasRor ? points.map((p) => p.ror ?? null) : computeRor(x, bt);
  return {
    x,
    bt,
    ror,
    burner: points.map((p) => p.actuators?.burner ?? null),
    air: points.map((p) => p.actuators?.air ?? null),
  };
}

export function buildMarkers(events: readonly RoastEvent[], base: number | null): ChartMarker[] {
  const off = base ?? 0;
  return events.map((e) => ({ x: e.t - off, label: EVENT_LABEL[e.kind] }));
}

export function buildBands(
  events: readonly RoastEvent[],
  base: number | null,
  latestT: number | null,
): ChartBand[] {
  if (base === null) return [];
  const at = (k: EventKind) => events.find((e) => e.kind === k)?.t;
  const de = at("dry_end");
  const fc = at("first_crack_start");
  const end = at("drop") ?? latestT ?? undefined;
  const bands: ChartBand[] = [];
  const add = (a: number | undefined, b: number | undefined, color: string) => {
    if (a !== undefined && b !== undefined && b > a)
      bands.push({ from: a - base, to: b - base, color });
  };
  add(base, de ?? fc ?? end, "rgba(220,190,90,0.055)");
  add(de, fc ?? end, "rgba(214,140,70,0.07)");
  add(fc, end, "rgba(150,90,50,0.14)");
  return bands;
}
