import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import type { Actuator, PlannedAction } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import { nextRecipeCue } from "@/lib/profile.ts";
import { formatClock } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { beep } from "@/state/audio.ts";
import { setActuator } from "@/state/controls.ts";

const NAME: Partial<Record<Actuator, string>> = { burner: "Burner", air: "Air", drum: "Drum" };
/** Announce a step this long before its time, seconds. */
const AHEAD_S = 10;

/**
 * During a roast with a profile, at the top of the controls: the next recipe
 * step, its countdown, a beep ten seconds before, and a button that sets it.
 * The person decides; nothing changes on its own.
 */
export function RecipeCue({
  recipe,
  sinceCharge,
  current,
  disabled,
}: {
  recipe: readonly PlannedAction[];
  sinceCharge: number;
  current: Partial<Record<Actuator, number>>;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const cue = nextRecipeCue(recipe, sinceCharge, current);
  const beeped = useRef<number | null>(null);
  const inS = cue ? cue.at - sinceCharge : null;

  useEffect(() => {
    if (cue && inS !== null && inS <= AHEAD_S && beeped.current !== cue.at) {
      beeped.current = cue.at;
      beep(false);
    }
  }, [cue, inS]);

  if (!cue || inS === null) return null;
  const now = inS <= AHEAD_S;
  const what = cue.steps
    .map((s) => `${t(NAME[s.actuator] ?? s.actuator)} ${Math.round(s.value)} %`)
    .join(" · ");

  return (
    <div
      aria-label={t("Recipe")}
      className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 border-b border-border pb-3"
    >
      <div className="flex min-w-0 flex-col">
        <span className="text-[15px] font-medium">{t("Recipe")}</span>
        <span
          className={cn(
            "tabular text-[13px] leading-tight",
            now ? "font-medium text-brand-text" : "text-muted-foreground",
          )}
          aria-live="polite"
        >
          {inS > 0 ? t("in {{t}}", { t: formatClock(inS) }) : t("now")}
        </span>
      </div>
      <span className={cn("truncate tabular text-[15px]", now && "font-medium")}>{what}</span>
      <Button
        variant={now ? "default" : "secondary"}
        disabled={disabled}
        onClick={() => {
          for (const s of cue.steps) void setActuator(s.actuator, s.value);
        }}
      >
        {t("Set")}
      </Button>
    </div>
  );
}
