import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { Snapshot } from "@/api/types.ts";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import { ActionLog } from "@/components/ActionLog.tsx";
import { CoachCard } from "@/components/CoachCard.tsx";
import { Controls } from "@/components/Controls.tsx";
import { CurveLegend, PhaseLine, RoastScaleBar, type ScaleMarker } from "@/components/Curve.tsx";
import { SavedDialog, StartDialog, StopDialog } from "@/components/Dialogs.tsx";
import { EventSegments, PrimaryButton } from "@/components/EventRow.tsx";
import { PreheatCard } from "@/components/Preheat.tsx";
import { Readouts } from "@/components/Readouts.tsx";
import { RoastChart, type ChartOverlay } from "@/components/RoastChart.tsx";
import { BeanMark } from "@/components/TopBar.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  EVENT_NAME,
  buildChartData,
  buildMarkers,
  chargeTime,
  developmentBand,
  mergeReference,
} from "@/lib/chartData.ts";
import { isRecording, latestAiNote, nextAction, rorAlert, rorDelta } from "@/lib/roast.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import {
  attempt,
  clearJustSaved,
  emergencyStop,
  markEvent,
  useJustSaved,
} from "@/state/actions.ts";
import { nudge } from "@/state/controls.ts";
import { applySnapshot, useLive } from "@/state/live.ts";
import { useReferenceCurve } from "@/state/reference.ts";

/** Enter belongs to text fields, dialogs and menus; everywhere else it is the next event. */
function enterIsOurs(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || !el.closest) return true;
  if (el.isContentEditable) return false;
  if (["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)) return false;
  if (el.closest("[role=dialog],[role=alertdialog],[data-slot=popover-content],[data-estop]"))
    return false;
  return true;
}

function Disconnected({
  snapshot,
  onConnect,
}: {
  snapshot: Snapshot | null;
  onConnect: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const connecting = snapshot?.connection === "connecting" || busy;
  const simulator = async () => {
    setBusy(true);
    await attempt(async () => applySnapshot(await api.connect(true)));
    setBusy(false);
  };
  return (
    <div className="grid flex-1 place-items-center p-gutter">
      <div className="flex max-w-md flex-col items-center gap-5 text-center">
        <BeanMark className="size-12" />
        <div className="flex flex-col gap-2">
          <h1 className="text-2xl font-semibold">{t("No roaster connected")}</h1>
          <p className="text-[15px] leading-relaxed text-muted-foreground">
            {snapshot?.connection_error ??
              t("Connect the roaster, or try everything with the built-in simulator.")}
          </p>
        </div>
        <div className="flex flex-wrap justify-center gap-3">
          <Button size="lg" disabled={connecting} onClick={onConnect}>
            {connecting ? t("Connecting…") : t("Connect roaster")}
          </Button>
          <Button
            size="lg"
            variant="secondary"
            disabled={connecting}
            onClick={() => void simulator()}
          >
            {t("Start simulator")}
          </Button>
        </div>
      </div>
    </div>
  );
}

export function RoastScreen({ onConnect }: { onConnect: () => void }) {
  const { t } = useTranslation();
  const { snapshot, points, notes, controlLog } = useLive();
  const [startOpen, setStartOpen] = useState(false);
  const [stopOpen, setStopOpen] = useState(false);
  const [logOpen, setLogOpen] = useState(false);
  const savedId = useJustSaved();

  const events = snapshot?.events;
  const base = snapshot?.charge_t ?? (events ? chargeTime(events) : null);
  const refStatus = snapshot?.reference ?? null;
  const refCurve = useReferenceCurve(refStatus?.id ?? null);
  const connected = snapshot?.connection === "connected";
  const recording = snapshot ? isRecording(snapshot.state) : false;

  const live = useMemo(() => buildChartData(points, base), [points, base]);
  const data = useMemo(
    () => (base !== null && refCurve ? mergeReference(live, refCurve.points) : live),
    [live, base, refCurve],
  );
  const markers = useMemo(
    () => buildMarkers(events ?? [], base, (k) => t(EVENT_NAME[k])),
    [events, base, t],
  );
  const band = useMemo(
    () => developmentBand(events ?? [], base, snapshot?.t ?? null),
    [events, base, snapshot?.t],
  );
  const dRor = useMemo(() => rorDelta(live.x, live.ror), [live]);
  const note = useMemo(() => latestAiNote(notes), [notes]);

  const projection = snapshot?.projection ?? null;
  // A milestone the bean temperature has already passed is no longer a target.
  const rawNext = projection?.next ?? null;
  const next =
    rawNext && !(typeof snapshot?.bt === "number" && rawNext.bt < snapshot.bt) ? rawNext : null;
  const nextLabel = next
    ? next.kind === "drop"
      ? next.eta_s !== null
        ? t("Drop {{bt}}° ~ in {{eta}}", {
            bt: formatNumber(next.bt, 0),
            eta: formatClock(next.eta_s),
          })
        : t("Drop {{bt}}°", { bt: formatNumber(next.bt, 0) })
      : next.eta_s !== null
        ? t("First crack ~ in {{eta}}", { eta: formatClock(next.eta_s) })
        : t("First crack ~ {{bt}}°", { bt: formatNumber(next.bt, 0) })
    : null;

  const overlay = useMemo<ChartOverlay>(() => {
    const t0 = snapshot?.t ?? null;
    const bt = snapshot?.bt ?? null;
    if (!recording || t0 === null || bt === null)
      return { now: null, projected: null, target: null };
    const x = t0 - (base ?? 0);
    const bt60 = base !== null ? (projection?.bt_60s ?? null) : null;
    return {
      now: { x, bt },
      projected: bt60 !== null ? { x: x + 60, bt: bt60 } : null,
      target: base !== null && next && nextLabel ? { bt: next.bt, label: nextLabel } : null,
    };
  }, [snapshot?.t, snapshot?.bt, recording, base, projection?.bt_60s, next, nextLabel]);

  const refEvents = refCurve?.events ?? refStatus?.events ?? [];
  const refDrop = refEvents.find((e) => e.kind === "drop");
  const refCharge = refEvents.find((e) => e.kind === "charge");
  const fcEv = events?.find((e) => e.kind === "first_crack_start");
  const alert = rorAlert({
    delta: dRor,
    sinceCharge: base !== null && typeof snapshot?.t === "number" ? snapshot.t - base : null,
    firstCrack: base !== null && fcEv ? fcEv.t - base : null,
    roastS: refDrop && refCharge ? refDrop.t - refCharge.t : null,
    dropped: !!events?.some((e) => e.kind === "drop"),
  });
  const scaleRef: ScaleMarker | null =
    typeof refDrop?.bt === "number"
      ? { bt: refDrop.bt, label: t("Reference drop {{bt}}°", { bt: formatNumber(refDrop.bt, 0) }) }
      : null;
  const scaleTarget: ScaleMarker | null =
    next && nextLabel ? { bt: next.bt, label: nextLabel } : null;

  const action = snapshot ? nextAction(snapshot.state, connected, snapshot.events) : null;
  const runPrimary = () => {
    if (!action) return;
    if (action.type === "start") setStartOpen(true);
    else if (action.type === "stop") setStopOpen(true);
    else void markEvent(action.kind);
  };
  const primaryRef = useRef(runPrimary);
  useEffect(() => {
    primaryRef.current = runPrimary;
  });

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        void emergencyStop();
        return;
      }
      if (e.ctrlKey || e.metaKey || e.altKey || e.repeat) return;
      if (e.key === "Enter") {
        if (!enterIsOurs(e.target)) return;
        e.preventDefault();
        primaryRef.current();
        return;
      }
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT"))
        return;
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

  if (!snapshot || (!connected && points.length === 0)) {
    return <Disconnected snapshot={snapshot} onConnect={onConnect} />;
  }

  const preRoast = connected && base === null && snapshot.preheat !== undefined;
  const chartLabel = t("Roast curve: bean {{bt}} °C at {{time}}, RoR {{ror}} °C/min", {
    bt: formatNumber(snapshot.bt),
    time: formatClock(base !== null && snapshot.t !== null ? snapshot.t - base : snapshot.t),
    ror: formatNumber(snapshot.ror),
  });

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-section overflow-y-auto p-gutter lg:flex-row lg:overflow-hidden">
      <section
        aria-label={t("Curve")}
        className="flex min-h-0 min-w-0 flex-[999_1_40rem] flex-col gap-3.5"
      >
        <CurveLegend
          reference={refStatus ? refStatus.title : null}
          projection={overlay.projected !== null}
          onClearReference={() =>
            void attempt(async () => applySnapshot(await api.setReference(null)))
          }
        />
        <div className="h-[22rem] min-h-0 lg:h-auto lg:min-h-56 lg:flex-1">
          <RoastChart
            data={data}
            markers={markers}
            band={band}
            overlay={overlay}
            minSpanS={720}
            actuators
            label={chartLabel}
          />
        </div>
        {base !== null && <PhaseLine phases={snapshot.phases} />}
        <RoastScaleBar bt={snapshot.bt} target={scaleTarget} reference={scaleRef} />
        <EventSegments snapshot={snapshot} />
        <div className="flex flex-wrap items-center justify-end gap-3">
          {preRoast && (
            <div className="mr-auto">
              <PreheatCard
                preheat={snapshot.preheat ?? null}
                bt={snapshot.bt}
                disabled={!snapshot.safety.connected || snapshot.safety.emergency_stop}
              />
            </div>
          )}
          {controlLog.length > 0 && (
            <Button variant="ghost" onClick={() => setLogOpen(true)}>
              {t("History")}
            </Button>
          )}
          {recording && action?.type !== "stop" && (
            <Button variant="ghost" onClick={() => setStopOpen(true)}>
              {t("End recording")}
            </Button>
          )}
          <PrimaryButton action={action} onRun={runPrimary} />
        </div>
      </section>
      <aside
        aria-label={t("Readings and controls")}
        className="flex min-h-0 flex-[1_1_20rem] flex-col gap-5 lg:max-w-[23rem] lg:overflow-y-auto"
      >
        <Readouts snapshot={snapshot} rorDelta={dRor} alert={alert} />
        <Controls machine={snapshot.machine} safety={snapshot.safety} />
        <CoachCard snapshot={snapshot} note={note} />
      </aside>
      <Dialog open={logOpen} onOpenChange={setLogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("History")}</DialogTitle>
            <DialogDescription>
              {t("Every change to burner, air and drum: who asked, and what the machine did.")}
            </DialogDescription>
          </DialogHeader>
          <ActionLog entries={controlLog} base={base} />
        </DialogContent>
      </Dialog>
      <SavedDialog key={savedId ?? "none"} roastId={savedId} onClose={clearJustSaved} />
      <StartDialog open={startOpen} onOpenChange={setStartOpen} />
      <StopDialog open={stopOpen} onOpenChange={setStopOpen} />
    </main>
  );
}
