import { useState } from "react";
import { Minus, Plus } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Actuator, ActuatorSpec, MachineInfo, SafetyStatus } from "@/api/types.ts";
import { Panel } from "@/components/Panel.tsx";
import { Button } from "@/components/ui/button.tsx";
import { Slider } from "@/components/ui/slider.tsx";
import { Switch } from "@/components/ui/switch.tsx";
import { nudge, setActuator, useDesired } from "@/state/controls.ts";

const NAMES: Record<Actuator, string> = {
  burner: "Burner",
  air: "Air",
  drum: "Drum",
  cooling: "Cooling",
  filter: "Filter",
};
const COLORS: Record<string, string> = {
  burner: "var(--series-burner)",
  air: "var(--series-air)",
};

function commanded(safety: SafetyStatus, a: Actuator): number {
  return safety.actuators[a] ?? 0;
}

function ActuatorRow({
  spec,
  current,
  disabled,
}: {
  spec: ActuatorSpec;
  current: number;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const desired = useDesired()[spec.actuator];
  const [draft, setDraft] = useState<number | null>(null);
  const shown = draft ?? desired ?? current;
  const step = spec.step || 1;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline justify-between">
        <span className="flex items-center gap-2 text-sm font-medium">
          <span
            className="size-2 rounded-full"
            style={{ background: COLORS[spec.actuator] ?? "var(--muted-foreground)" }}
          />
          {t(NAMES[spec.actuator])}
        </span>
        <span className="readout text-xl font-semibold">
          {Math.round(shown)}
          <span className="ml-0.5 text-sm font-normal text-muted-foreground">%</span>
        </span>
      </div>
      <div className="flex items-center gap-3">
        <Button
          variant="outline"
          size="icon"
          disabled={disabled}
          aria-label={`${t(NAMES[spec.actuator])} −`}
          onClick={(e) => nudge(spec, current, -(e.shiftKey ? 5 : 1) * step)}
        >
          <Minus />
        </Button>
        <Slider
          min={spec.min}
          max={spec.max}
          step={step}
          disabled={disabled}
          value={[shown]}
          aria-label={t(NAMES[spec.actuator])}
          onValueChange={(v) => setDraft(v[0])}
          onValueCommit={(v) => {
            setDraft(null);
            if (v[0] !== current) void setActuator(spec.actuator, v[0]);
          }}
          className="h-6"
        />
        <Button
          variant="outline"
          size="icon"
          disabled={disabled}
          aria-label={`${t(NAMES[spec.actuator])} +`}
          onClick={(e) => nudge(spec, current, (e.shiftKey ? 5 : 1) * step)}
        >
          <Plus />
        </Button>
      </div>
    </div>
  );
}

export function Controls({
  machine,
  safety,
}: {
  machine: MachineInfo | null;
  safety: SafetyStatus;
}) {
  const { t } = useTranslation();
  const desired = useDesired();
  const disabled = !machine || !safety.connected || safety.emergency_stop;

  return (
    <Panel title={t("Controls")}>
      {!machine ? (
        <p className="text-sm text-muted-foreground">{t("Connect a machine to control it.")}</p>
      ) : (
        <div className="flex flex-col gap-3.5">
          {machine.actuators.map((spec) => (
            <ActuatorRow
              key={spec.actuator}
              spec={spec}
              current={commanded(safety, spec.actuator)}
              disabled={disabled}
            />
          ))}
          {machine.switches.map((a) => {
            const on = (desired[a] ?? commanded(safety, a)) >= 50;
            return (
              <label key={a} className="flex items-center justify-between text-sm font-medium">
                {t(NAMES[a])}
                <Switch
                  checked={on}
                  disabled={disabled}
                  onCheckedChange={(c) => void setActuator(a, c ? 100 : 0)}
                />
              </label>
            );
          })}
          <p className="text-[11px] text-subtle-foreground">
            {t("Keys: Q/A burner ±5, W/S air ±5. Shift + button = ±5.")}
          </p>
        </div>
      )}
    </Panel>
  );
}
