import { useEffect, useState } from "react";
import { Mic, MicOff, X } from "lucide-react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { AudioSettings, AudioStatus, AudioView, Sensitivity, Snapshot } from "@/api/types.ts";
import { Segmented } from "@/components/TopBar.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Switch } from "@/components/ui/switch.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select.tsx";
import { EVENT_NAME } from "@/lib/chartData.ts";
import { FC_POPS, micLook, type HeardBanner } from "@/lib/hearing.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { attempt, markEvent } from "@/state/actions.ts";
import { applySnapshot } from "@/state/live.ts";
import { useSound } from "@/state/sound.ts";
import { toast } from "@/state/toasts.ts";

/** Five little bars: the crack-band pops of the last 20 s against the threshold. */
function PopBars({ count, lit }: { count: number; lit: boolean }) {
  const heights = [12, 7, 14, 9, 13];
  return (
    <span aria-hidden="true" className="flex h-3.5 items-end gap-0.5">
      {heights.map((h, i) => (
        <i
          key={i}
          style={{ height: `${h}px` }}
          className={cn(
            "block w-[3px] rounded-[1px]",
            lit || i < count ? "bg-brand" : "bg-border-strong",
          )}
        />
      ))}
    </span>
  );
}

/** The microphone above the curve: whether it listens, the pops, what it heard. */
export function MicPill({
  snapshot,
  onOpen,
}: {
  snapshot: Pick<Snapshot, "audio" | "events">;
  onOpen: () => void;
}) {
  const { t } = useTranslation();
  const audio = snapshot.audio;
  const look = micLook(audio, snapshot.events);
  const n = audio?.crack_20s ?? 0;
  const text = {
    off: t("Microphone off"),
    idle: t("Listens while recording"),
    starting: t("Microphone…"),
    listening: n > 0 ? t("{{n}} pops", { n }) : t("Listening"),
    heard: t("First crack"),
    problem: audio?.state === "silent" ? t("Microphone silent") : t("Microphone error"),
  }[look];
  const label = {
    off: t("Microphone off: open the settings"),
    idle: t("Microphone: listens while recording. Open the settings"),
    starting: t("Microphone starting. Open the settings"),
    listening: t("Microphone: {{n}} pops in 20 s. Open the settings", { n }),
    heard: t("Microphone: first crack heard. Open the settings"),
    problem: t("Microphone: {{text}}. Open the settings", { text }),
  }[look];
  return (
    <Button
      variant="outline"
      size="sm"
      onClick={onOpen}
      aria-label={label}
      className={cn(
        "-my-2 gap-2",
        look === "heard" && "border-brand text-brand-text",
        look === "problem" && "border-warning-text/60 text-warning-text",
        (look === "off" || look === "idle") && "text-muted-foreground",
      )}
    >
      {look === "off" ? <MicOff className="size-4" /> : <Mic className="size-4" />}
      <span>{text}</span>
      {(look === "listening" || look === "heard") && (
        <PopBars count={Math.min(n, FC_POPS)} lit={look === "heard"} />
      )}
    </Button>
  );
}

/** "First crack heard and marked: 7:41 · 195.1 °C" with Undo and Mark now. */
export function HeardBannerBar({
  banner,
  base,
  onDismiss,
}: {
  banner: HeardBanner;
  /** Recording time of CHARGE. */
  base: number | null;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();
  const name = t(EVENT_NAME[banner.kind]);
  const at = [
    formatClock(banner.t - (base ?? 0)),
    banner.bt !== null ? `${formatNumber(banner.bt, 1)} °C` : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const undo = () =>
    void attempt(async () => {
      applySnapshot(await api.deleteEvent(banner.kind));
      onDismiss();
    });
  return (
    <div
      role="status"
      className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-2xl border-[1.5px] border-brand bg-brand/8 py-2 pr-2 pl-4"
    >
      <Mic className="size-5 shrink-0 text-brand-text" aria-hidden="true" />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="font-semibold">
          {banner.marked
            ? t("{{name}} heard and marked: {{at}}", { name, at })
            : t("Sounds like {{name}}: {{at}}", { name, at })}
        </span>
        <span className="text-[13px] text-muted-foreground">
          {banner.marked
            ? t("{{n}} pops in 20 s. Not right? Your mark replaces the heard one.", {
                n: banner.pops,
              })
            : t("{{n}} pops in 20 s. Listening only: mark it if you hear it too.", {
                n: banner.pops,
              })}
        </span>
      </div>
      {banner.marked && (
        <Button variant="ghost" onClick={undo}>
          {t("Undo")}
        </Button>
      )}
      <Button variant="secondary" onClick={() => void markEvent(banner.kind)}>
        {t("Mark now")}
        {banner.enter && (
          <kbd className="rounded-sm bg-surface-selected px-1.5 font-sans text-[13px]">↵</kbd>
        )}
      </Button>
      <Button variant="ghost" size="icon" onClick={onDismiss} aria-label={t("Dismiss")}>
        <X className="size-4" />
      </Button>
    </div>
  );
}

/** A level bar, −100 … −30 dBFS. */
function Level({ label, db }: { label: string; db: number | null }) {
  const x = db === null ? 0 : Math.min(1, Math.max(0, (db + 100) / 70));
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex justify-between text-[13px]">
        <span className="text-muted-foreground">{label}</span>
        <span className="tabular">{db === null ? "–" : `${formatNumber(db, 0)} dBFS`}</span>
      </div>
      <div className="h-2 overflow-hidden rounded-pill bg-segment-track">
        <div className="h-full rounded-pill bg-brand" style={{ width: `${x * 100}%` }} />
      </div>
    </div>
  );
}

function statusLine(t: TFunction, s: AudioStatus): string {
  switch (s.state) {
    case "off":
      return t("Off: the microphone stays closed.");
    case "idle":
      return t("Listens from the start of a recording to its end.");
    case "starting":
      return t("Opening the microphone…");
    case "listening":
      return s.simulated
        ? t("Listening to the simulator's roaster.")
        : t("Listening: {{device}}, {{rate}} kHz.", {
            device: s.device ?? "",
            rate: formatNumber((s.rate ?? 0) / 1000, 0),
          });
    case "silent":
      return t(
        "Only silence: allow RoastMCP to use the microphone in System Settings → Privacy & Security → Microphone.",
      );
    case "error":
      return s.error ?? t("The microphone does not work.");
  }
}

const DEFAULT_DEVICE = "__default__";

function MicForm({ view, onView }: { view: AudioView; onView: (v: AudioView) => void }) {
  const { t } = useTranslation();
  const sound = useSound();
  const [busy, setBusy] = useState(false);
  const save = async (patch: Partial<AudioSettings>) => {
    setBusy(true);
    try {
      onView(await api.setAudio({ ...view.settings, ...patch }));
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const s = view.settings;
  const listening = view.status.state === "listening";
  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-2">
        <span className="text-[13px] text-muted-foreground">{t("When it hears a crack")}</span>
        <Segmented
          label={t("When it hears a crack")}
          value={s.mode}
          options={[
            { value: "mark", label: t("Mark it") },
            { value: "show", label: t("Only show") },
            { value: "off", label: t("Off") },
          ]}
          onChange={(mode) => void save({ mode })}
        />
        <p className="text-[13px] text-muted-foreground">
          {s.mode === "mark"
            ? t(
                "First and second crack are marked when heard, between 170 and 205 °C after dry end. Your own mark always wins.",
              )
            : s.mode === "show"
              ? t("The pops and the rate are shown; you mark the cracks yourself.")
              : t("Nothing is heard.")}
        </p>
      </div>
      <div className="flex flex-col gap-2">
        <span className="text-[13px] text-muted-foreground">{t("Microphone")}</span>
        <Select
          value={s.device ?? DEFAULT_DEVICE}
          disabled={busy || s.mode === "off"}
          onValueChange={(v) => void save({ device: v === DEFAULT_DEVICE ? null : v })}
        >
          <SelectTrigger aria-label={t("Microphone")} className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={DEFAULT_DEVICE}>{t("System default")}</SelectItem>
            {view.devices.map((d) => (
              <SelectItem key={d} value={d}>
                {d}
              </SelectItem>
            ))}
            {s.device !== null && !view.devices.includes(s.device) && (
              <SelectItem value={s.device}>
                {t("{{name}} (not found)", { name: s.device })}
              </SelectItem>
            )}
          </SelectContent>
        </Select>
      </div>
      <div className="flex flex-col gap-2">
        <span className="text-[13px] text-muted-foreground">{t("Sensitivity")}</span>
        <Segmented<Sensitivity>
          label={t("Sensitivity")}
          value={s.sensitivity}
          options={[
            { value: "low", label: t("Low") },
            { value: "normal", label: t("Normal") },
            { value: "high", label: t("High") },
          ]}
          onChange={(sensitivity) => void save({ sensitivity })}
        />
      </div>
      <div className="flex flex-col gap-3 rounded-xl border border-border p-4">
        <p
          className={cn(
            "text-[13px]",
            view.status.state === "silent" || view.status.state === "error"
              ? "text-warning-text"
              : "text-muted-foreground",
          )}
        >
          {statusLine(t, view.status)}
        </p>
        {listening && (
          <>
            <Level label={t("Crack band 0.4–4 kHz")} db={sound.crackDb} />
            <Level label={t("High band 6–16 kHz")} db={sound.highDb} />
          </>
        )}
      </div>
      {(view.status.noise?.length ?? 0) > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-[13px] text-muted-foreground">
            {t("The fan, learned per air level")}
          </span>
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 tabular text-[13px]">
            {view.status.noise?.map((n) => (
              <div key={n.air} className="contents">
                <dt className="text-muted-foreground">{t("Air {{n}} %", { n: n.air })}</dt>
                <dd>
                  {formatNumber(n.crack_db, 0)} dBFS
                  {typeof n.high_db === "number" && ` · ${formatNumber(n.high_db, 0)} dBFS`}
                </dd>
              </div>
            ))}
          </dl>
        </div>
      )}
      {view.status.model && (
        <p className="text-[13px] text-muted-foreground">
          {view.status.model.decides
            ? t(
                "The trained model decides first crack: {{m}} of {{n}} marks hit, the rules {{r}}.",
                {
                  m: view.status.model.evaluation.model_within,
                  n: view.status.model.evaluation.roasts,
                  r: view.status.model.evaluation.rules_within,
                },
              )
            : t("A model is trained; the rules decide until it hits your marks better.")}
        </p>
      )}
      <label className="flex items-start justify-between gap-4">
        <span className="flex flex-col gap-0.5">
          <span className="text-sm">{t("Keep the sound of each roast")}</span>
          <span className="text-[13px] text-muted-foreground">
            {t(
              "As FLAC beside the roast, about 35 MB. Stays on this computer; it makes the detector better.",
            )}
          </span>
        </span>
        <Switch
          checked={s.record}
          disabled={busy || s.mode === "off"}
          onCheckedChange={(record) => void save({ record })}
          aria-label={t("Keep the sound of each roast")}
        />
      </label>
      <p className="text-[13px] text-muted-foreground">
        {s.record
          ? t(
              "The roast keeps the times of the pops, a level a second and, as you chose, its sound.",
            )
          : t(
              "The sound stays in memory and only while recording. The roast keeps the times of the pops and a level a second, never the sound.",
            )}
      </p>
    </div>
  );
}

/** Microphone settings (ADR 0021). */
export function MicDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useTranslation();
  const [view, setView] = useState<AudioView | null>(null);
  useEffect(() => {
    if (!open) return;
    let alive = true;
    const load = () =>
      api
        .audio()
        .then((v) => alive && setView(v))
        .catch(() => undefined);
    void load();
    // The state and the levels change while it listens.
    const timer = setInterval(() => void load(), 2000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [open]);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Microphone")}</DialogTitle>
          <DialogDescription>
            {t("RoastMCP listens for first and second crack while a roast is recorded.")}
          </DialogDescription>
        </DialogHeader>
        {view ? (
          <MicForm view={view} onView={setView} />
        ) : (
          <p className="text-sm text-muted-foreground">{t("Loading…")}</p>
        )}
      </DialogContent>
    </Dialog>
  );
}
