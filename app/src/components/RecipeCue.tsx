import { useEffect, useRef } from "react";
import { Bot } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { Actuator, NextStep } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import { formatClock } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { beep } from "@/state/audio.ts";
import { setActuator } from "@/state/controls.ts";

const NAME: Partial<Record<Actuator, string>> = { burner: "Burner", air: "Air", drum: "Drum" };
/** Announce a step this long before its time, seconds. */
const AHEAD_S = 10;

/**
 * During a roast with a profile, at the top of the controls: the next recipe
 * step from the daemon, its countdown and its trigger ("FC +0:30").
 *
 * Without an AI the person decides: a beep ten seconds before, and a button
 * that applies the step. While an AI holds control the daemon sets the steps
 * itself (ADR 0019): the button gives way to a quiet "automatic" mark, since
 * pressing anything here would be a takeover.
 */
export function RecipeCue({ next, disabled }: { next: NextStep; disabled: boolean }) {
  const { t } = useTranslation();
  const beeped = useRef<string | null>(null);
  const inS = next.in_s;
  const automatic = next.automatic === true;
  const key = `${next.at_s ?? "?"}:${next.set.map((s) => s.step ?? s.actuator).join(",")}`;

  useEffect(() => {
    if (automatic || inS === null || inS > AHEAD_S || beeped.current === key) return;
    beeped.current = key;
    beep(false);
  }, [automatic, inS, key]);

  if (inS === null) return null;
  const now = inS <= AHEAD_S;
  const what = next.set
    .map((s) => `${t(NAME[s.actuator] ?? s.actuator)} ${Math.round(s.value)} %`)
    .join(" · ");
  const when =
    inS <= 0
      ? t("now")
      : next.projected
        ? t("about in {{t}}", { t: formatClock(inS) })
        : t("in {{t}}", { t: formatClock(inS) });

  return (
    <div
      aria-label={t("Recipe")}
      className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-3 border-b border-border pb-3"
    >
      <div className="flex min-w-0 flex-col">
        <span className="text-[15px] font-medium">{t("Recipe")}</span>
        <span
          className={cn(
            "tabular text-[13px] leading-tight",
            now && !automatic ? "font-medium text-brand-text" : "text-muted-foreground",
          )}
          aria-live="polite"
        >
          {when}
        </span>
      </div>
      <div className="flex min-w-0 flex-col">
        <span className={cn("truncate tabular text-[15px]", now && "font-medium")}>{what}</span>
        {next.label && (
          <span className="truncate tabular text-[13px] leading-tight text-muted-foreground">
            {next.label}
          </span>
        )}
      </div>
      {automatic ? (
        <span
          className="flex h-9 shrink-0 items-center gap-1.5 rounded-pill border border-border-strong bg-surface-raised px-3 text-[13px] text-muted-foreground"
          title={t("The AI is in control: the recipe's steps are set automatically.")}
        >
          <Bot className="size-4 text-brand-text" aria-hidden />
          {t("automatic")}
        </span>
      ) : (
        <Button
          variant={now ? "default" : "secondary"}
          disabled={disabled}
          onClick={() => {
            for (const s of next.set) void setActuator(s.actuator, s.value);
          }}
        >
          {t("Apply")}
        </Button>
      )}
    </div>
  );
}
