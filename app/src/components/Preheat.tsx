import { useState } from "react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { Preheat } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import { inputCls } from "@/components/Dialogs.tsx";
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

/** Pre-roast: bring the drum to a target temperature and hold it there until CHARGE. */
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

  const status = preheat
    ? preheat.state === "heating"
      ? t("heating · {{bt}} → {{target}} °C", {
          bt: formatNumber(bt, 0),
          target: formatNumber(preheat.target_c, 0),
        })
      : t("holding {{target}} °C", { target: formatNumber(preheat.target_c, 0) })
    : t("off");

  return (
    <form
      aria-label={t("Preheat")}
      className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-card-border bg-surface py-2 pr-2 pl-4"
      onSubmit={(e) => {
        e.preventDefault();
        if (preheat) void run(() => api.stopPreheat());
        else start();
      }}
    >
      <span className="text-[15px] font-medium">{t("Preheat")}</span>
      <label className="flex items-center gap-2 text-sm text-muted-foreground">
        <span className="sr-only">{t("Target temperature")}</span>
        <input
          className={cn(inputCls, "w-20 text-right tabular")}
          inputMode="numeric"
          value={target}
          disabled={!!preheat}
          aria-invalid={!valid}
          onChange={(e) => setTarget(e.target.value)}
        />
        °C
      </label>
      <Button
        type="submit"
        variant={preheat ? "outline" : "secondary"}
        disabled={busy || disabled || (!preheat && !valid)}
      >
        {preheat ? t("Stop") : t("Start")}
      </Button>
      <span
        className={cn("tabular text-[13px]", preheat ? "text-brand-text" : "text-muted-foreground")}
        aria-live="polite"
      >
        {status}
      </span>
    </form>
  );
}
