import { useEffect, useMemo } from "react";
import { PlugZap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Controls } from "@/components/Controls.tsx";
import { EventButtons } from "@/components/EventButtons.tsx";
import { Panel } from "@/components/Panel.tsx";
import { Readouts } from "@/components/Readouts.tsx";
import { RoastChart } from "@/components/RoastChart.tsx";
import { SafetyPanel } from "@/components/SafetyPanel.tsx";
import { Button } from "@/components/ui/button.tsx";
import { buildBands, buildChartData, buildMarkers, chargeTime } from "@/lib/chartData.ts";
import { emergencyStop } from "@/state/actions.ts";
import { nudge } from "@/state/controls.ts";
import { useLive } from "@/state/live.ts";

export function RoastScreen({ onConnect }: { onConnect: () => void }) {
  const { t } = useTranslation();
  const { snapshot, points } = useLive();
  const events = snapshot?.events;
  const base = snapshot?.charge_t ?? (events ? chargeTime(events) : null);

  const data = useMemo(() => buildChartData(points, base), [points, base]);
  const markers = useMemo(() => buildMarkers(events ?? [], base), [events, base]);
  const bands = useMemo(
    () => buildBands(events ?? [], base, snapshot?.t ?? null),
    [events, base, snapshot?.t],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (e.key === "Escape") {
        void emergencyStop();
        return;
      }
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT"))
        return;
      if (e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
      const map: Record<string, ["burner" | "air", number]> = {
        q: ["burner", 5],
        a: ["burner", -5],
        w: ["air", 5],
        s: ["air", -5],
      };
      const hit = map[e.key.toLowerCase()];
      const snap = snapshot;
      if (!hit || !snap?.machine || !snap.safety.connected || snap.safety.emergency_stop) return;
      const spec = snap.machine.actuators.find((x) => x.actuator === hit[0]);
      if (spec) nudge(spec, snap.safety.actuators[hit[0]] ?? 0, hit[1]);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [snapshot]);

  const connected = snapshot?.connection === "connected";

  return (
    <div className="grid min-h-0 flex-1 gap-4 overflow-y-auto p-4 lg:grid-cols-[minmax(0,1fr)_22rem]">
      <div className="flex min-w-0 flex-col gap-4">
        <Panel className="relative h-[min(52vh,560px)] min-h-72 p-2 pt-3">
          <RoastChart data={data} markers={markers} bands={bands} minSpanS={600} />
          {!connected && points.length === 0 && (
            <div className="absolute inset-0 grid place-items-center rounded-2xl bg-surface/80 backdrop-blur-sm">
              <div className="flex max-w-xs flex-col items-center gap-3 text-center">
                <span className="grid size-12 place-items-center rounded-2xl bg-brand-soft text-brand">
                  <PlugZap />
                </span>
                <h3 className="text-base font-semibold">{t("No machine connected")}</h3>
                <p className="text-sm text-muted-foreground">
                  {snapshot?.connection_error ??
                    t("Connect the roaster or the simulator to see the live curve.")}
                </p>
                <Button onClick={onConnect}>{t("Connect")}</Button>
              </div>
            </div>
          )}
        </Panel>
        {snapshot && <Readouts snapshot={snapshot} />}
        {snapshot && <EventButtons snapshot={snapshot} />}
      </div>
      <div className="flex min-w-0 flex-col gap-4">
        {snapshot && <Controls machine={snapshot.machine} safety={snapshot.safety} />}
        {snapshot && <SafetyPanel safety={snapshot.safety} />}
      </div>
    </div>
  );
}
