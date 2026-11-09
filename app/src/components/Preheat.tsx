import { Play, Square } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { Preheat } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import { UnitInput } from "@/components/Dialogs.tsx";
import { cn } from "@/lib/utils.ts";
import { formatNumber } from "@/lib/units.ts";
import { attempt } from "@/state/actions.ts";
import { applySnapshot } from "@/state/live.ts";

const TARGET_KEY = "roastmcp.preheatTarget";
const AIR_KEY = "roastmcp.preheatAir";
const DEFAULT_TARGET = 200;
/** The roaster's practice: air 20 % before and at CHARGE (ADR 0018). */
const DEFAULT_AIR = 20;

function stored(key: string, fallback: number, min: number): string {
  try {
    const raw = localStorage.getItem(key);
    const v = raw === null ? NaN : Number(raw);
    return Number.isFinite(v) && v >= min ? String(v) : String(fallback);
  } catch {
    return String(fallback);
  }
}

function remember(key: string, value: number): void {
  try {
    localStorage.setItem(key, String(value));
  } catch {
    /* private window: not remembered */
  }
}

/**
 * Pre-roast, at the top of the controls: bring the drum to a target and hold
 * it until CHARGE (ADR 0015), at the air the roast starts with (ADR 0018).
 * Laid out like the burner below it: name and state, then target, air and
 * the switch. The plan fills in target and air; without one, the last values.
 */
export function PreheatCard({
  preheat,
  bt,
  disabled,
  plan,
}: {
  preheat: Preheat | null;
  bt: number | null;
  disabled: boolean;
  /** The plan's bean temperature and air at CHARGE, where it has them. */
  plan: { target: number | null; air: number | null };
}) {
  const { t } = useTranslation();
  const [target, setTarget] = useState(() =>
    plan.target !== null ? String(plan.target) : stored(TARGET_KEY, DEFAULT_TARGET, 1),
  );
  const [air, setAir] = useState(() =>
    plan.air !== null ? String(plan.air) : stored(AIR_KEY, DEFAULT_AIR, 0),
  );
  const [busy, setBusy] = useState(false);

  // A new plan fills in its values; the person may still change them.
  const [seen, setSeen] = useState(plan);
  if (plan.target !== seen.target || plan.air !== seen.air) {
    setSeen(plan);
    if (plan.target !== null && plan.target !== seen.target) setTarget(String(plan.target));
    if (plan.air !== null && plan.air !== seen.air) setAir(String(plan.air));
  }

  const value = parseFloat(target.replace(",", "."));
  const airValue = parseFloat(air.replace(",", "."));
  const valid = Number.isFinite(value) && value > 0;
  const airValid = Number.isFinite(airValue) && airValue >= 0 && airValue <= 100;
  const fromPlan =
    (plan.target !== null && String(plan.target) === target) ||
    (plan.air !== null && String(plan.air) === air);

  const run = async (fn: () => Promise<Parameters<typeof applySnapshot>[0]>) => {
    setBusy(true);
    await attempt(async () => applySnapshot(await fn()));
    setBusy(false);
  };
  const start = () => {
    if (!valid || !airValid) return;
    remember(TARGET_KEY, value);
    remember(AIR_KEY, airValue);
    void run(() => api.preheat(value, airValue));
  };

  // Short: the values are in the fields beside it, the temperature above.
  const status = preheat
    ? preheat.state === "heating"
      ? t("heating")
      : t("holding")
    : fromPlan
      ? t("per plan")
      : t("off");
  const title = preheat
    ? t("heating · {{bt}} → {{target}} °C", {
        bt: formatNumber(bt, 0),
        target: formatNumber(preheat.target_c, 0),
      })
    : undefined;

  return (
    <form
      aria-label={t("Preheat")}
      className="grid grid-cols-[auto_minmax(0,0.85fr)_minmax(0,1.15fr)_auto] items-center gap-1.5 border-b border-border pb-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (preheat) void run(() => api.stopPreheat());
        else start();
      }}
    >
      <div className="flex min-w-0 flex-col" title={title}>
        <span className="text-[15px] font-medium">{t("Preheat")}</span>
        <span
          className={cn(
            "truncate text-[13px] leading-tight",
            preheat ? "text-brand-text" : "text-muted-foreground",
          )}
          aria-live="polite"
        >
          {status}
        </span>
      </div>
      <label>
        <span className="sr-only">{t("Target temperature")}</span>
        <UnitInput
          compact
          unit="°C"
          inputMode="numeric"
          value={target}
          disabled={!!preheat}
          aria-invalid={!valid}
          onChange={(e) => setTarget(e.target.value)}
        />
      </label>
      <label title={t("Air while preheating: the air the roast starts with")}>
        <span className="sr-only">{t("Air")}</span>
        <UnitInput
          compact
          unit={t("% air")}
          inputMode="numeric"
          value={air}
          disabled={!!preheat}
          aria-invalid={!airValid}
          onChange={(e) => setAir(e.target.value)}
        />
      </label>
      {/* An icon like the burner's −/+ below: the row has no room for words. */}
      <Button
        type="submit"
        size="icon"
        variant={preheat ? "outline" : "secondary"}
        className="size-touch"
        aria-label={preheat ? t("Stop preheat") : t("Start preheat")}
        title={preheat ? t("Stop preheat") : t("Start preheat")}
        disabled={busy || disabled || (!preheat && !(valid && airValid))}
      >
        {preheat ? <Square className="size-4" /> : <Play className="size-4" />}
      </Button>
    </form>
  );
}
