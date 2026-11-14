import { useEffect, useState } from "react";
import { Settings } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { BackupStatus, Snapshot } from "@/api/types.ts";
import { AutopilotSwitch } from "@/components/AutopilotSwitch.tsx";
import { BackupDialog } from "@/components/BackupDialog.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Kbd } from "@/components/ui/kbd.tsx";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover.tsx";
import { setLang } from "@/i18n/index.ts";
import { backupLine } from "@/lib/backup.ts";
import {
  AUTOPILOT_HOLDER,
  autopilotOffered,
  isAiHolder,
  isRecording,
  PREHEAT_HOLDER,
  presenceView,
} from "@/lib/roast.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";
import { attempt, confirmPresence, emergencyStop } from "@/state/actions.ts";
import { useCareDue } from "@/state/care.ts";
import { applySnapshot } from "@/state/live.ts";
import { stateLabel, type Screen } from "@/state/helpers.ts";
import { setThemeMode, useThemeMode, type ThemeMode } from "@/state/theme.ts";

/** The RoastMCP mark: a roasted bean. */
export function BeanMark({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 30 30" aria-hidden="true" className={className}>
      <ellipse cx="15" cy="15" rx="9" ry="12.5" transform="rotate(-30 15 15)" fill="var(--brand)" />
      <path
        d="M10.5 5.5 C 16 11, 13 17, 19.5 24.5"
        fill="none"
        stroke="var(--background)"
        strokeWidth="2.2"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** A small segmented choice (navigation, settings). */
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  className,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  className?: string;
}) {
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className={cn("flex gap-1 rounded-lg bg-segment-track p-1", className)}
    >
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            "h-touch flex-1 cursor-pointer rounded-md px-3 text-sm whitespace-nowrap transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
            value === o.value
              ? "bg-surface-selected font-medium text-foreground"
              : "text-muted-foreground hover:text-foreground",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function SettingsMenu() {
  const { t, i18n } = useTranslation();
  const theme = useThemeMode();
  const [backup, setBackup] = useState<BackupStatus | null>(null);
  const [backupOpen, setBackupOpen] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  // A failed backup must not go unseen: look now and then, and on opening.
  useEffect(() => {
    const load = () =>
      void api
        .backup()
        .then(setBackup)
        .catch(() => undefined);
    load();
    const timer = setInterval(load, 10 * 60_000);
    return () => clearInterval(timer);
  }, [menuOpen]);
  const backupFailed = backup?.last ? !backup.last.ok : false;
  const keys: [string, string][] = [
    ["↵", t("Next event")],
    ["Esc", t("Emergency stop")],
    ["Q / A", t("Burner ±5")],
    ["W / S", t("Air ±5")],
  ];
  return (
    <>
      <Popover open={menuOpen} onOpenChange={setMenuOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="relative"
            aria-label={backupFailed ? t("Settings: the last backup failed") : t("Settings")}
          >
            <Settings />
            {backupFailed && (
              <span className="absolute top-1.5 right-1.5 size-2.5 rounded-full bg-destructive ring-2 ring-background" />
            )}
          </Button>
        </PopoverTrigger>
        <PopoverContent align="end" className="w-80 gap-4 p-4">
          <div className="flex flex-col gap-2">
            <span className="text-[13px] text-muted-foreground">{t("Language")}</span>
            <Segmented
              label={t("Language")}
              value={i18n.language.startsWith("de") ? "de" : "en"}
              options={[
                { value: "de", label: "Deutsch" },
                { value: "en", label: "English" },
              ]}
              onChange={(v) => setLang(v)}
            />
          </div>
          <div className="flex flex-col gap-2">
            <span className="text-[13px] text-muted-foreground">{t("Theme")}</span>
            <Segmented<ThemeMode>
              label={t("Theme")}
              value={theme}
              options={[
                { value: "system", label: t("System") },
                { value: "light", label: t("Light") },
                { value: "dark", label: t("Dark") },
              ]}
              onChange={setThemeMode}
            />
          </div>
          <div className="flex flex-col gap-2 border-t border-border pt-3">
            <span className="text-[13px] text-muted-foreground">{t("Backup")}</span>
            <div className="flex items-center justify-between gap-3">
              <span
                className={cn(
                  "min-w-0 text-[13px]",
                  backupFailed ? "text-danger-text" : "text-foreground",
                )}
              >
                {backupLine(backup, t, i18n.language)}
              </span>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  setMenuOpen(false);
                  setBackupOpen(true);
                }}
              >
                {t("Set up")}
              </Button>
            </div>
          </div>
          <div className="flex flex-col gap-2 border-t border-border pt-3">
            <span className="text-[13px] text-muted-foreground">{t("Keyboard")}</span>
            <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-sm">
              {keys.map(([k, v]) => (
                <div key={k} className="contents">
                  <dt>
                    <kbd className="rounded-sm border border-border-strong px-1.5 py-0.5 font-sans text-[13px]">
                      {k}
                    </kbd>
                  </dt>
                  <dd>{v}</dd>
                </div>
              ))}
            </dl>
            <p className="text-[13px] text-muted-foreground">
              {t("Shift + button = ±5 on − and +.")}
            </p>
          </div>
        </PopoverContent>
      </Popover>
      <BackupDialog
        open={backupOpen}
        onOpenChange={setBackupOpen}
        status={backup}
        onStatus={setBackup}
      />
    </>
  );
}

function MachineStatus({
  snapshot,
  onConnect,
}: {
  snapshot: Snapshot | null;
  onConnect: () => void;
}) {
  const { t } = useTranslation();
  const machine = snapshot?.machine;
  const connection = snapshot?.connection ?? "disconnected";
  if (connection !== "connected" || !machine) {
    return (
      <span className="flex items-center gap-3 text-[13px] text-muted-foreground">
        <span className="flex items-center gap-2">
          <span
            className={cn(
              "size-2 rounded-pill",
              connection === "error" ? "bg-danger-text" : "bg-border-strong",
            )}
          />
          {connection === "connecting" ? t("Connecting…") : t("No roaster")}
        </span>
        {connection !== "connecting" && (
          <Button variant="secondary" size="sm" onClick={onConnect}>
            {t("Connect")}
          </Button>
        )}
      </span>
    );
  }
  const linkOk = snapshot?.safety.connected ?? false;
  const recording = snapshot ? isRecording(snapshot.state) : false;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          // Never squeezed away: whether this is the simulator or a real roaster
          // must stay in view. The full name is in the popover.
          className="flex h-touch min-w-24 shrink-0 cursor-pointer items-center gap-2 rounded-lg px-2 text-[13px] text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-3 focus-visible:ring-ring/50"
        >
          <span className={cn("size-2 rounded-pill", linkOk ? "bg-ok" : "bg-danger-text")} />
          <span className="max-w-48 min-w-0 truncate">
            {machine.simulated ? t("Simulator") : machine.name}
          </span>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 gap-3 p-4">
        <div className="flex flex-col gap-1">
          <span className="text-[15px] font-medium">{machine.name}</span>
          <span className="text-[13px] text-muted-foreground">
            {machine.simulated ? t("Simulator") : (machine.port ?? "")}
            {" · "}
            {t("max. {{bt}} °C", { bt: formatNumber(machine.max_bt_c, 0) })}
          </span>
        </div>
        <Button
          variant="outline"
          disabled={recording}
          onClick={() => void attempt(async () => applySnapshot(await api.disconnect()))}
        >
          {t("Disconnect")}
        </Button>
        {recording && (
          <p className="text-[13px] text-muted-foreground">
            {t("Stop the recording before disconnecting.")}
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
}

/**
 * Who is in control: the autopilot switch while a profile is the plan (ADR
 * 0020), and a pill while an AI or the preheat holds control or an AI is
 * paused by a person. An AI asking for control needs "I'm at the roaster"
 * first; this is the one place that offers it in view (the alarm toast says
 * who asks).
 */
function AiStatus({ snapshot }: { snapshot: Snapshot }) {
  const { t } = useTranslation();
  const s = snapshot.safety;
  const presence = presenceView(s);
  const autopilot = autopilotOffered(snapshot);
  const autopilotOn = autopilot && !!snapshot.autopilot;
  // While the preheat or the autopilot holds, an AI may ask to take over
  // (ADR 0019, ADR 0020).
  const asking = isAiHolder(s.lease_holder) ? null : (snapshot.control_request ?? null);
  if (asking) {
    return (
      <span className="flex items-center gap-2">
        {autopilot && <AutopilotSwitch snapshot={snapshot} compact />}
        <Button
          className="shrink-0"
          title={t("{{holder}} asks for control.", { holder: asking })}
          onClick={() => void confirmPresence()}
        >
          {t("I'm at the roaster")}
        </Button>
      </span>
    );
  }
  // The autopilot's pill says it all while the daemon holds a lease of its
  // own: its own, or the preheat's while it is armed.
  const leasePill =
    (s.lease_holder !== null || s.ai_suspended) &&
    s.lease_holder !== AUTOPILOT_HOLDER &&
    !(autopilotOn && s.lease_holder === PREHEAT_HOLDER);
  const askPresence = s.lease_holder !== null && presence.ask;
  if (!autopilot && !leasePill) return null;
  // The daemon renews the leases of its own controllers: no countdown to show.
  const left = isAiHolder(s.lease_holder) ? s.lease_remaining_s : null;
  return (
    <span className="flex items-center gap-2">
      {/* Beside an AI's pill the bar is crowded: the mark and the switch stay. */}
      {autopilot && <AutopilotSwitch snapshot={snapshot} compact={isAiHolder(s.lease_holder)} />}
      {leasePill && (
        <span className="flex h-9 shrink-0 items-center gap-2 rounded-pill border border-border-strong bg-surface-raised px-3.5 text-[13px] whitespace-nowrap">
          {s.lease_holder ? (
            <>
              {s.lease_holder === PREHEAT_HOLDER
                ? t("Preheating")
                : t("{{holder}} in control", { holder: s.lease_holder })}
              {left !== null && (
                <span className="tabular text-muted-foreground">
                  ·{" "}
                  {t("{{t}} left", {
                    t: left < 60 ? `${Math.max(0, Math.round(left))} s` : formatClock(left),
                  })}
                </span>
              )}
            </>
          ) : (
            t("AI paused")
          )}
        </span>
      )}
      {askPresence && (
        <Button
          variant={presence.urgent ? "default" : "secondary"}
          title={t("Confirms that you are at the roaster. Space does the same.")}
          onClick={() => void confirmPresence()}
        >
          {t("I'm at the roaster")}
          {/* Beside the autopilot's pill the bar has no room for the key. */}
          {!autopilot && <Kbd className="ml-1 hidden sm:inline-flex">{t("Space")}</Kbd>}
        </Button>
      )}
    </span>
  );
}

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
  const { t } = useTranslation();
  const careDue = useCareDue();
  const rec = snapshot?.recording ?? null;
  const recording = snapshot ? isRecording(snapshot.state) : false;
  const title = recording ? rec?.title || t("Untitled roast") : "RoastMCP";
  const meta = [
    recording ? rec?.beans : null,
    recording && rec?.green_weight_g ? `${formatNumber(rec.green_weight_g, 0)} g` : null,
    stateLabel(t, snapshot?.state ?? "disconnected"),
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    // One line from 1280 px on: a second line would push the controls (and
    // the cooling switch) out of view during a roast; the title gives way.
    <header className="flex flex-wrap items-center gap-x-7 gap-y-3 border-b border-border px-gutter py-3.5 xl:flex-nowrap">
      <div className="flex min-w-0 flex-[1_1_18rem] items-center gap-3.5">
        <BeanMark className="size-7.5 shrink-0" />
        <div className="flex min-w-0 flex-col">
          <span className="truncate text-[17px] font-semibold">{title}</span>
          <span className="truncate text-[13px] text-muted-foreground">{meta}</span>
        </div>
      </div>
      <nav aria-label={t("Screens")} className="flex gap-1 rounded-lg bg-segment-track p-1">
        {(["roast", "inventory", "archive", "profiles", "care"] as const).map((s) => (
          <button
            key={s}
            type="button"
            onClick={() => onScreen(s)}
            aria-current={screen === s ? "page" : undefined}
            className={cn(
              "inline-flex h-touch cursor-pointer items-center rounded-md px-4.5 text-sm transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
              screen === s
                ? "bg-surface-selected font-medium text-foreground"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {s === "roast"
              ? t("Roast")
              : s === "inventory"
                ? t("Inventory")
                : s === "archive"
                  ? t("Archive")
                  : s === "profiles"
                    ? t("Profiles")
                    : t("Care")}
            {s === "care" && careDue > 0 && (
              <span
                aria-label={careDue === 1 ? t("1 task due") : t("{{n}} tasks due", { n: careDue })}
                className="ml-2 inline-flex h-5.5 min-w-5.5 items-center justify-center rounded-pill bg-brand/12 px-1.5 tabular text-[13px] font-medium text-brand-text"
              >
                {careDue}
              </span>
            )}
          </button>
        ))}
      </nav>
      <div className="flex min-w-fit flex-[1_1_18rem] items-center justify-end gap-3">
        <MachineStatus snapshot={snapshot} onConnect={onConnect} />
        {snapshot && <AiStatus snapshot={snapshot} />}
        <SettingsMenu />
        <button
          type="button"
          data-estop
          onClick={() => void emergencyStop()}
          aria-label={t("Emergency stop (Escape)")}
          title={t("Esc in this window · ⌘⇧⎋ anywhere on the Mac")}
          className="flex h-touch shrink-0 cursor-pointer items-center gap-2.5 rounded-lg bg-estop px-4.5 text-[15px] font-semibold whitespace-nowrap text-estop-foreground transition-colors duration-150 outline-none hover:bg-estop-hover focus-visible:ring-4 focus-visible:ring-ring/60 active:translate-y-px"
        >
          {t("Emergency stop")}
          <kbd className="rounded-sm bg-estop-foreground/20 px-2 py-0.5 font-sans text-[13px] font-semibold">
            Esc
          </kbd>
        </button>
      </div>
    </header>
  );
}
