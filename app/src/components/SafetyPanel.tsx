import { HandMetal, ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { SafetyStatus } from "@/api/types.ts";
import { Panel } from "@/components/Panel.tsx";
import { Button } from "@/components/ui/button.tsx";
import { cn } from "@/lib/utils.ts";
import { formatClock } from "@/lib/units.ts";
import { applySnapshot } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

export function SafetyPanel({ safety }: { safety: SafetyStatus }) {
  const { t } = useTranslation();
  const due = safety.presence_due_in_s;
  const aiActive = safety.lease_holder !== null;
  const urgent = (due !== null && due < 30) || aiActive;

  const call = async (fn: () => Promise<Parameters<typeof applySnapshot>[0]>) => {
    try {
      applySnapshot(await fn());
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    }
  };

  return (
    <Panel title={t("Safety and AI")}>
      <div className="flex flex-col gap-2.5 text-sm">
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">{t("AI control")}</span>
          <span className="font-medium">
            {aiActive
              ? `${safety.lease_holder} · ${formatClock(safety.lease_remaining_s)}`
              : safety.ai_suspended
                ? t("Suspended")
                : t("No AI connected")}
          </span>
        </div>
        <div className="flex items-center justify-between">
          <span className="text-muted-foreground">{t("Presence check")}</span>
          <span
            className={cn(
              "readout font-medium",
              due !== null && due < 0 && "text-status-critical",
              due !== null && due >= 0 && due < 30 && "text-status-warning",
            )}
          >
            {due === null
              ? "–"
              : due < 0
                ? `${t("overdue")} ${formatClock(-due)}`
                : formatClock(due)}
          </span>
        </div>
        <button
          onClick={() => void call(() => api.presence())}
          className={cn(
            "flex h-10 items-center justify-center gap-2 rounded-xl text-sm font-semibold transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/60",
            urgent
              ? "bg-primary text-primary-foreground shadow-[0_0_0_4px_var(--brand-soft)] motion-safe:animate-pulse"
              : "bg-secondary text-secondary-foreground hover:bg-accent",
          )}
        >
          <HandMetal className="size-4.5" />
          {t("I'm at the roaster")}
        </button>
        {safety.ai_suspended && (
          <Button variant="outline" onClick={() => void call(() => api.resumeAi())}>
            <ShieldCheck />
            {t("Allow AI again")}
          </Button>
        )}
      </div>
    </Panel>
  );
}
