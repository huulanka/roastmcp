import { useState } from "react";
import { Flame, Moon, Octagon, Sun } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { RoastState, Snapshot } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog.tsx";
import { StartDialog } from "@/components/Dialogs.tsx";
import { setLang } from "@/i18n/index.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock } from "@/lib/units.ts";
import { applySnapshot } from "@/state/live.ts";
import { roastClock, stateLabel, type Screen } from "@/state/helpers.ts";
import { emergencyStop } from "@/state/actions.ts";
import { currentTheme, toggleTheme } from "@/state/theme.ts";
import { toast } from "@/state/toasts.ts";

const STATE_COLOR: Record<RoastState, string> = {
  disconnected: "bg-muted text-muted-foreground",
  connected: "bg-status-info/15 text-status-info",
  recording: "bg-estop/15 text-status-critical",
  charged: "bg-brand-soft text-brand",
  dropped: "bg-status-warning/15 text-status-warning",
  cooling: "bg-status-info/15 text-status-info",
  finished: "bg-status-good/15 text-status-good",
};

export function TopBar({
  snapshot,
  screen,
  onScreen,
  onConnect,
}: {
  snapshot: Snapshot | null;
  screen: Screen;
  onScreen: (s: Screen) => void;
  onConnect: () => void;
}) {
  const { t, i18n } = useTranslation();
  const [startOpen, setStartOpen] = useState(false);
  const [stopOpen, setStopOpen] = useState(false);
  const [, setRerender] = useState(0);

  const state: RoastState = snapshot?.state ?? "disconnected";
  const machine = snapshot?.machine;
  const connected = snapshot?.connection === "connected";
  const recording =
    state === "recording" || state === "charged" || state === "dropped" || state === "cooling";
  const canStart = connected && !recording;
  const clock = roastClock(snapshot);
  const hasCharge = (snapshot?.charge_t ?? null) !== null;

  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    }
  };

  return (
    <header className="sticky top-0 z-30 flex flex-wrap items-center gap-x-5 gap-y-2 border-b border-border bg-background/70 px-4 py-2.5 backdrop-blur-xl">
      <div className="flex items-center gap-2 font-semibold tracking-tight">
        <span className="grid size-8 place-items-center rounded-xl bg-brand-soft text-brand">
          <Flame className="size-4.5" />
        </span>
        <span className="hidden sm:inline">RoastMCP</span>
      </div>
      <nav className="flex gap-1 rounded-xl bg-muted/60 p-1" aria-label="Screens">
        {(["roast", "archive"] as const).map((s) => (
          <button
            key={s}
            onClick={() => onScreen(s)}
            aria-current={screen === s}
            className={cn(
              "rounded-lg px-3 py-1 text-sm font-medium transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
              screen === s
                ? "bg-surface-raised text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {s === "roast" ? t("Roast") : t("Archive")}
          </button>
        ))}
      </nav>

      <div className="flex items-center gap-2.5">
        <span className="flex items-center gap-2 text-sm">
          <span
            className={cn(
              "size-2 rounded-full",
              connected
                ? "bg-status-good"
                : snapshot?.connection === "error"
                  ? "bg-status-critical"
                  : "bg-subtle-foreground",
            )}
          />
          <span className="text-foreground">
            {connected && machine
              ? machine.name
              : snapshot?.connection === "connecting"
                ? t("Connecting…")
                : t("No machine")}
          </span>
          {machine?.simulated && (
            <span className="rounded-md bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
              {t("simulator")}
            </span>
          )}
        </span>
        {connected ? (
          <Button
            size="sm"
            variant="ghost"
            disabled={recording}
            onClick={() => void run(async () => applySnapshot(await api.disconnect()))}
          >
            {t("Disconnect")}
          </Button>
        ) : (
          <Button size="sm" variant="secondary" onClick={onConnect}>
            {t("Connect")}
          </Button>
        )}
        <span className={cn("rounded-full px-2.5 py-1 text-xs font-medium", STATE_COLOR[state])}>
          {stateLabel(t, state)}
        </span>
      </div>

      <div className="mx-auto flex items-baseline gap-2" aria-live="off">
        <span className="text-xs text-muted-foreground">
          {hasCharge ? t("Roast time") : t("Recording time")}
        </span>
        <span className="readout text-4xl font-semibold">
          {recording || hasCharge ? formatClock(clock) : "–:––"}
        </span>
      </div>

      <div className="flex items-center gap-2">
        {recording ? (
          <Button variant="outline" onClick={() => setStopOpen(true)}>
            <span className="size-2.5 rounded-sm bg-status-critical" />
            {t("Stop recording")}
          </Button>
        ) : (
          <Button disabled={!canStart} onClick={() => setStartOpen(true)}>
            <span className="size-2.5 rounded-full bg-primary-foreground" />
            {t("Start recording")}
          </Button>
        )}
        <Button
          variant="ghost"
          size="sm"
          onClick={() => setLang(i18n.language.startsWith("de") ? "en" : "de")}
          aria-label="Language"
        >
          {i18n.language.startsWith("de") ? "DE" : "EN"}
        </Button>
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("Toggle theme")}
          onClick={() => {
            toggleTheme();
            setRerender((n) => n + 1);
          }}
        >
          {currentTheme() === "dark" ? <Sun /> : <Moon />}
        </Button>
        <button
          onClick={() => void emergencyStop()}
          className="flex h-11 items-center gap-2 rounded-xl bg-estop px-4 text-sm font-bold tracking-wide text-estop-foreground uppercase shadow-[0_0_0_4px_var(--estop-ring),0_8px_24px_-6px_var(--estop)] transition-transform duration-150 outline-none hover:brightness-110 focus-visible:ring-4 focus-visible:ring-white/70 active:scale-[0.97]"
          aria-label="Emergency stop (Escape)"
        >
          <Octagon className="size-5" />
          {t("Emergency stop")}
          <kbd className="rounded bg-black/25 px-1.5 py-0.5 font-sans text-[10px] font-medium normal-case">
            Esc
          </kbd>
        </button>
      </div>

      <StartDialog open={startOpen} onOpenChange={setStartOpen} />
      <AlertDialog open={stopOpen} onOpenChange={setStopOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("Stop recording?")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t(
                "The roast will be saved to the archive. Make sure the beans have been dropped and cooled.",
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() =>
                void run(async () => {
                  const r = await api.stopRecording();
                  toast("success", t("Roast saved ({{id}})", { id: r.id }));
                })
              }
            >
              {t("Stop and save")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </header>
  );
}
