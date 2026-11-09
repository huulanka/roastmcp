import { useTranslation } from "react-i18next";

import type { RoastGoal, RoastPlan } from "@/api/types.ts";
import type { PlanProgress } from "@/lib/plan.ts";
import { formatClock, formatNumber } from "@/lib/units.ts";

const GOAL_NAME: Record<RoastGoal, string> = {
  espresso: "Espresso",
  filter: "Filter",
  omni: "Omni",
  other: "Own",
};

/**
 * The plan's targets during the roast (ADR 0014): a countdown to the drop
 * temperature and the development time ratio against its target. Guidance
 * only; nothing here acts on the roaster.
 */
export function PlanCard({ plan, progress }: { plan: RoastPlan; progress: PlanProgress }) {
  const { t } = useTranslation();
  const { drop, dtr } = progress;
  if (!drop && !dtr) return null;
  return (
    <section
      aria-label={t("Target")}
      className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg border border-brand/60 px-3 py-1.5 text-[13px]"
    >
      <span className="font-semibold text-brand-text">
        {plan.goal ? t("Target · {{goal}}", { goal: t(GOAL_NAME[plan.goal]) }) : t("Target")}
      </span>
      {drop && (
        <span className="flex items-baseline gap-1.5 whitespace-nowrap">
          <span className="text-muted-foreground">
            {t("Drop at {{bt}} °C", { bt: formatNumber(drop.target, 0) })}
          </span>
          <span className="tabular text-[15px] font-medium">
            {drop.remaining <= 0
              ? t("reached")
              : drop.eta_s !== null
                ? t("in ≈ {{eta}}", { eta: formatClock(drop.eta_s) })
                : t("{{deg}} °C to go", { deg: formatNumber(drop.remaining, 1) })}
          </span>
        </span>
      )}
      {dtr && (
        <span className="flex items-center gap-2 whitespace-nowrap">
          <span className="text-muted-foreground">DTR</span>
          {dtr.now === null ? (
            <span className="text-muted-foreground">
              {t("target {{pct}} %, from first crack", { pct: formatNumber(dtr.target, 0) })}
            </span>
          ) : (
            <>
              <span
                role="img"
                aria-label={t("DTR {{now}} of {{target}} percent", {
                  now: formatNumber(dtr.now, 1),
                  target: formatNumber(dtr.target, 0),
                })}
                className="block h-1.5 w-16 overflow-hidden rounded-pill bg-surface-raised"
              >
                <span
                  className="block h-1.5 rounded-pill bg-brand"
                  style={{ width: `${Math.min(100, (dtr.now / dtr.target) * 100)}%` }}
                />
              </span>
              <span className="tabular">
                <span className="text-[15px] font-medium">{formatNumber(dtr.now, 1)}</span>
                <span className="text-muted-foreground">
                  {" "}
                  / {formatNumber(dtr.target, 0)} %
                  {dtr.at_s !== null && ` · ${formatClock(dtr.at_s)}`}
                </span>
              </span>
            </>
          )}
        </span>
      )}
    </section>
  );
}
