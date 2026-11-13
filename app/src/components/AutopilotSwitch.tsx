import { useState } from "react";
import { Bot } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api } from "@/api/client.ts";
import type { Snapshot } from "@/api/types.ts";
import { Switch } from "@/components/ui/switch.tsx";
import { autopilotStatus } from "@/lib/roast.ts";
import { cn } from "@/lib/utils.ts";
import { attempt } from "@/state/actions.ts";
import { applySnapshot } from "@/state/live.ts";

/**
 * The autopilot switch (ADR 0020), a pill in the top bar beside the other
 * "who is in control" marks, while a profile is the plan. Switched on, the
 * daemon sets the recipe's steps and keeps the roast on the profile's curve
 * with small burner corrections; the pill says what it is doing and how far
 * it moved away from the recipe, in a word or two (the tooltip has the
 * sentence). `compact` leaves only the mark and the switch, when the bar is
 * crowded.
 */
export function AutopilotSwitch({
  snapshot,
  compact = false,
}: {
  snapshot: Snapshot;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const autopilot = snapshot.autopilot ?? null;
  const status = autopilotStatus(autopilot);
  const text = t(status.text, status.values);
  const short = status.short ? t(status.short, status.values) : "";
  const disabled = !snapshot.safety.connected || snapshot.safety.emergency_stop;
  const toggle = async (on: boolean) => {
    setBusy(true);
    await attempt(async () =>
      applySnapshot(await (on ? api.startAutopilot() : api.stopAutopilot())),
    );
    setBusy(false);
  };

  return (
    <label
      className={cn(
        "flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-pill border border-border-strong bg-surface-raised pr-1 pl-3 text-[13px] whitespace-nowrap",
        disabled && "cursor-default",
      )}
      title={`${t("Autopilot")}: ${text}. ${t(
        "The autopilot sets the recipe's steps and keeps the roast on the profile's curve with small burner corrections. Changing burner, air or drum by hand switches it off.",
      )}`}
    >
      <Bot
        className={cn("size-4", autopilot ? "text-brand-text" : "text-muted-foreground")}
        aria-hidden
      />
      {!compact && (
        <>
          <span className="font-medium">{t("Autopilot")}</span>
          {short && (
            <span className="tabular text-brand-text" aria-live="polite">
              {short}
            </span>
          )}
        </>
      )}
      <Switch
        checked={autopilot !== null}
        disabled={disabled || busy}
        aria-label={t("Autopilot")}
        onCheckedChange={(on) => void toggle(on)}
      />
    </label>
  );
}
