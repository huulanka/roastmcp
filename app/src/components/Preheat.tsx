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

const KEY = "roastmcp.preheatTarget";
const DEFAULT_TARGET = 180;

function storedTarget(): string {
  try {
    const v = Number(localStorage.getItem(KEY));
    return Number.isFinite(v) && v > 0 ? String(v) : String(DEFAULT_TARGET);
  } catch {
    return String(DEFAULT_TARGET);
  }
}

/**
 * Pre-roast, at the top of the controls: bring the drum to a target and hold
 * it until CHARGE (ADR 0015). Laid out like the burner below it: name and
 * state, then the target and the switch.
 */
export function PreheatCard({
  preheat,
  bt,
  disabled,
}: {
  preheat: Preheat | null;
  bt: number | null;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const [target, setTarget] = useState(storedTarget);
  const [busy, setBusy] = useState(false);
  const value = parseFloat(target.replace(",", "."));
  const valid = Number.isFinite(value) && value > 0;

  const run = async (fn: () => Promise<Parameters<typeof applySnapshot>[0]>) => {
    setBusy(true);
    await attempt(async () => applySnapshot(await fn()));
    setBusy(false);
  };
  const start = () => {
    if (!valid) return;
    try {
      localStorage.setItem(KEY, String(value));
    } catch {
      /* ignore */
    }
    void run(() => api.preheat(value));
  };

  // Short: the target is in the field beside it, the temperature above.
  const status = preheat ? (preheat.state === "heating" ? t("heating") : t("holding")) : t("off");
  const title = preheat
    ? t("heating · {{bt}} → {{target}} °C", {
        bt: formatNumber(bt, 0),
        target: formatNumber(preheat.target_c, 0),
      })
    : undefined;

  return (
    <form
      aria-label={t("Preheat")}
      className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 border-b border-border pb-3"
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
          unit="°C"
          inputMode="numeric"
          value={target}
          disabled={!!preheat}
          aria-invalid={!valid}
          onChange={(e) => setTarget(e.target.value)}
        />
      </label>
      <Button
        type="submit"
        variant={preheat ? "outline" : "secondary"}
        className="w-22"
        disabled={busy || disabled || (!preheat && !valid)}
      >
        {preheat ? t("Stop") : t("Start")}
      </Button>
    </form>
  );
}
