import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Note, Snapshot } from "@/api/types.ts";
import { Button } from "@/components/ui/button.tsx";
import { AUTOPILOT_HOLDER, isAiHolder, PREHEAT_HOLDER, presenceView } from "@/lib/roast.ts";
import { cn } from "@/lib/utils.ts";
import { formatClock } from "@/lib/units.ts";
import { confirmPresence } from "@/state/actions.ts";
import { applySnapshot } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

/** The AI coach: its latest note, who is in control, and the presence check. */
export function CoachCard({ snapshot, note }: { snapshot: Snapshot; note: Note | null }) {
  const { t } = useTranslation();
  const safety = snapshot.safety;
  const holder = safety.lease_holder;
  const presence = presenceView(safety);
  const base = snapshot.charge_t;

  const resume = async () => {
    try {
      applySnapshot(await api.resumeAi());
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    }
  };

  return (
    <section
      aria-label={t("Coach")}
      className="flex flex-col gap-2.5 rounded-card border border-border-strong p-card"
    >
      <div className="flex items-baseline justify-between gap-3 text-[13px] text-muted-foreground">
        <span>
          {holder === PREHEAT_HOLDER
            ? t("Coach · preheating")
            : holder === AUTOPILOT_HOLDER
              ? t("Coach · autopilot")
              : holder
                ? t("{{holder}} · in control", { holder })
                : safety.ai_suspended
                  ? t("Coach · AI paused")
                  : t("Coach")}
        </span>
        {note && base !== null && <span className="tabular">{formatClock(note.t - base)}</span>}
      </div>
      <p className={cn("text-[15px] leading-snug", !note && "text-muted-foreground")}>
        {note ? note.text : isAiHolder(holder) ? t("No notes yet.") : t("No AI connected.")}
      </p>
      {presence.due !== null && (
        <p
          className={cn(
            "tabular text-[13px]",
            presence.urgent && presence.overdue
              ? "font-medium text-danger-text"
              : "text-muted-foreground",
          )}
          aria-live="polite"
        >
          {presence.overdue
            ? t("Presence not confirmed · overdue {{t}}", { t: formatClock(-presence.due) })
            : t("Presence confirmed · next in {{t}}", { t: formatClock(presence.due) })}
        </p>
      )}
      {(presence.ask || safety.ai_suspended) && (
        <div className="flex flex-wrap gap-2 pt-1">
          {presence.ask && (
            <Button
              variant={presence.urgent ? "default" : "secondary"}
              onClick={() => void confirmPresence()}
            >
              {t("I'm at the roaster")}
            </Button>
          )}
          {safety.ai_suspended && (
            <Button variant="outline" onClick={() => void resume()}>
              {t("Allow AI again")}
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
