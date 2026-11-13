import { useState } from "react";
import { BotOff, Minus, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Actuator, ActuatorSpec, MachineInfo, SafetyStatus } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import { Slider } from "@/components/ui/slider.tsx";
import { Switch } from "@/components/ui/switch.tsx";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip.tsx";
import { AUTOPILOT_HOLDER, isAiHolder, PREHEAT_HOLDER } from "@/lib/roast.ts";
import { actuatorShown } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { nudge, setActuator, useDesired } from "@/state/controls.ts";

const ACTUATOR_NAME: Record<Actuator, string> = {
  burner: "Burner",
  air: "Air",
  drum: "Drum",
  cooling: "Cooling",
  filter: "Filter",
};
/** Burner is heat (orange), air is airflow (blue); everything else stays neutral. */
const RANGE_COLOUR: Partial<Record<Actuator, string>> = {
  burner: "var(--brand)",
  air: "var(--ror)",
};
const PRIMARY: Actuator[] = ["burner", "air"];
/** Where the machine profile names none (older daemons): the heat loop. */
const DEFAULT_TAKEOVER: Actuator[] = ["burner", "air", "drum"];

/** Who a person's command on burner, air or drum takes control from. */
type Controller = "ai" | "autopilot" | null;

/**
 * While an AI or the autopilot is in control: touching this control ends it
 * (ADR 0019, ADR 0020). Filter and cooling carry no mark; the person may use
 * them freely.
 */
function TakeoverMark({ controller }: { controller: Controller }) {
  const { t } = useTranslation();
  const label =
    controller === "autopilot"
      ? t("Changing it switches the autopilot off")
      : t("Changing it ends AI control");
  return (
    <TooltipProvider delayDuration={150}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            tabIndex={0}
            role="img"
            aria-label={label}
            className="inline-flex shrink-0 self-center rounded-sm text-warning-text outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <BotOff className="size-4" aria-hidden />
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function commanded(safety: SafetyStatus, a: Actuator): number {
  return safety.actuators[a] ?? 0;
}

/** Slider state: the dragged value, then the requested value until the daemon confirms. */
function useShown(spec: ActuatorSpec, current: number) {
  const desired = useDesired()[spec.actuator];
  const [draft, setDraft] = useState<number | null>(null);
  return {
    shown: draft ?? desired ?? current,
    slider: {
      min: spec.min,
      max: spec.max,
      step: spec.step || 1,
      value: [draft ?? desired ?? current],
      onValueChange: (v: number[]) => setDraft(v[0]),
      onValueCommit: (v: number[]) => {
        setDraft(null);
        if (v[0] !== current) void setActuator(spec.actuator, v[0]);
      },
      style: {
        "--slider-range": RANGE_COLOUR[spec.actuator] ?? "var(--muted-foreground)",
      } as React.CSSProperties,
    },
  };
}

function Stepper({
  spec,
  current,
  disabled,
  sign,
}: {
  spec: ActuatorSpec;
  current: number;
  disabled: boolean;
  sign: 1 | -1;
}) {
  const { t } = useTranslation();
  const name = t(ACTUATOR_NAME[spec.actuator]);
  return (
    <Button
      variant="secondary"
      size="icon"
      disabled={disabled}
      aria-label={sign > 0 ? t("Increase {{name}}", { name }) : t("Decrease {{name}}", { name })}
      onClick={(e) => nudge(spec, current, sign * (e.shiftKey ? 5 : 1))}
    >
      {sign > 0 ? <Plus /> : <Minus />}
    </Button>
  );
}

/** Burner and air: big value, steppers and a slider. */
function MainControl({
  spec,
  current,
  disabled,
  automatic = false,
  endsAi = null,
}: {
  spec: ActuatorSpec;
  current: number;
  disabled: boolean;
  /** An automation (preheat, autopilot) is moving it; touching it hands control back. */
  automatic?: boolean;
  /** An AI or the autopilot is in control and touching this ends it. */
  endsAi?: Controller;
}) {
  const { t } = useTranslation();
  const { shown, slider } = useShown(spec, current);
  const name = t(ACTUATOR_NAME[spec.actuator]);
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between">
        <span className="flex items-baseline gap-2 text-[15px] font-medium">
          {name}
          {endsAi && <TakeoverMark controller={endsAi} />}
          {automatic && (
            <span className="text-[13px] font-normal text-brand-text">{t("automatic")}</span>
          )}
        </span>
        <span className="tabular text-value leading-none font-medium">
          {Math.round(shown)}
          <span className="ml-1 text-[15px] font-normal text-muted-foreground">%</span>
        </span>
      </div>
      <div className="flex items-center gap-3">
        <Stepper spec={spec} current={current} disabled={disabled} sign={-1} />
        <Slider {...slider} disabled={disabled} aria-label={name} />
        <Stepper spec={spec} current={current} disabled={disabled} sign={1} />
      </div>
    </div>
  );
}

/**
 * Drum, filter and other ranged actuators: one compact row with steppers only.
 * They are set once or twice per roast; the value reads in the machine's unit
 * (drum rpm) where the profile has one.
 */
function CompactControl({
  spec,
  current,
  disabled,
  endsAi = null,
}: {
  spec: ActuatorSpec;
  current: number;
  disabled: boolean;
  endsAi?: Controller;
}) {
  const { t } = useTranslation();
  const shown = useDesired()[spec.actuator] ?? current;
  const name = t(ACTUATOR_NAME[spec.actuator]);
  const unit = spec.display ? t(spec.display.unit) : "%";
  return (
    <div className="flex items-center gap-2">
      <span className="flex w-18 shrink-0 items-center gap-1.5 text-sm">
        {name}
        {endsAi && <TakeoverMark controller={endsAi} />}
      </span>
      <Stepper spec={spec} current={current} disabled={disabled} sign={-1} />
      <span className="min-w-0 flex-1 text-center tabular text-[15px] font-medium whitespace-nowrap">
        {Math.round(actuatorShown(spec.display, shown))}
        <span className="text-[13px] font-normal text-muted-foreground"> {unit}</span>
      </span>
      <Stepper spec={spec} current={current} disabled={disabled} sign={1} />
    </div>
  );
}

function SwitchControl({
  a,
  on,
  disabled,
  endsAi = null,
}: {
  a: Actuator;
  on: boolean;
  disabled: boolean;
  endsAi?: Controller;
}) {
  const { t } = useTranslation();
  return (
    <label
      className={cn(
        "flex h-touch cursor-pointer items-center justify-between gap-3 text-sm",
        disabled && "cursor-default",
      )}
    >
      <span className="flex items-center gap-1.5">
        {t(ACTUATOR_NAME[a])}
        {endsAi && <TakeoverMark controller={endsAi} />}
      </span>
      <span className="flex items-center gap-3">
        <span
          className={cn(
            "text-[13px]",
            on ? "font-medium text-foreground" : "text-muted-foreground",
          )}
        >
          {on ? t("On") : t("Off")}
        </span>
        <Switch
          checked={on}
          disabled={disabled}
          aria-label={t(ACTUATOR_NAME[a])}
          onCheckedChange={(c) => void setActuator(a, c ? 100 : 0)}
        />
      </span>
    </label>
  );
}

export function Controls({
  machine,
  safety,
  preheating = false,
  autopilot = false,
  top,
}: {
  machine: MachineInfo | null;
  safety: SafetyStatus;
  /** The preheat moves the burner (also under an AI that took over before CHARGE). */
  preheating?: boolean;
  /** The autopilot is in control (also under an AI's lease, ADR 0020). */
  autopilot?: boolean;
  /** Shown above burner and air: the preheat before a roast. */
  top?: React.ReactNode;
}) {
  const { t } = useTranslation();
  const desired = useDesired();
  const disabled = !machine || !safety.connected || safety.emergency_stop;
  if (!machine) return null;
  const controller: Controller = isAiHolder(safety.lease_holder)
    ? "ai"
    : autopilot || safety.lease_holder === AUTOPILOT_HOLDER
      ? "autopilot"
      : null;
  const takeover = machine.takeover_actuators ?? DEFAULT_TAKEOVER;
  const endsAi = (a: Actuator): Controller => (takeover.includes(a) ? controller : null);

  const main = machine.actuators.filter((s) => PRIMARY.includes(s.actuator));
  const rest = machine.actuators.filter((s) => !PRIMARY.includes(s.actuator));
  // Cooling last: it is the one switch you reach for at the end of a roast.
  const switches = [...machine.switches].sort(
    (p, q) => Number(p === "cooling") - Number(q === "cooling"),
  );

  return (
    <section
      aria-label={t("Controls")}
      className="flex flex-col gap-3 rounded-card border border-card-border bg-surface p-card"
    >
      {top}
      {main.map((spec) => (
        <MainControl
          key={spec.actuator}
          spec={spec}
          current={commanded(safety, spec.actuator)}
          disabled={disabled}
          automatic={
            spec.actuator === "burner" &&
            (preheating || autopilot || safety.lease_holder === PREHEAT_HOLDER)
          }
          endsAi={endsAi(spec.actuator)}
        />
      ))}
      {(rest.length > 0 || switches.length > 0) && (
        <div className="flex flex-col gap-1 border-t border-border pt-3">
          {rest.map((spec) => (
            <CompactControl
              key={spec.actuator}
              spec={spec}
              current={commanded(safety, spec.actuator)}
              disabled={disabled}
              endsAi={endsAi(spec.actuator)}
            />
          ))}
          {switches.map((a) => (
            <SwitchControl
              key={a}
              a={a}
              on={(desired[a] ?? commanded(safety, a)) >= 50}
              disabled={disabled}
              endsAi={endsAi(a)}
            />
          ))}
        </div>
      )}
    </section>
  );
}
