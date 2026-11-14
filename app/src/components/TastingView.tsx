import { useState } from "react";
import { Ellipsis, Pencil, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { Cupping } from "@/api/types.ts";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu.tsx";
import { meanScore, rated } from "@/lib/tasting.ts";
import { formatNumber } from "@/lib/units.ts";
import { toast } from "@/state/toasts.ts";

function tastedOn(iso: string, lang: string): string {
  const d = new Date(iso);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return "today";
  return d.toLocaleDateString(lang, { dateStyle: "medium" });
}

/** One tasting: when, how brewed, the total, the scores given, the words. */
function TastingCard({
  cupping,
  onEdit,
  onDelete,
}: {
  cupping: Cupping;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { t, i18n } = useTranslation();
  const scores = rated(cupping.scores);
  const mean = meanScore(cupping.scores);
  const on = tastedOn(cupping.cupped_at, i18n.language);
  const big =
    typeof cupping.overall_score === "number"
      ? formatNumber(cupping.overall_score, cupping.overall_score % 1 ? 1 : 0)
      : mean !== null
        ? formatNumber(mean)
        : null;
  return (
    <li className="flex flex-col gap-4 rounded-card border border-card-border bg-surface p-card">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-[15px] font-semibold">
            {t("{{date}} · day {{n}}", {
              date: on === "today" ? t("Today") : on,
              n: cupping.rest_days,
            })}
          </span>
          <span className="truncate text-[13px] text-muted-foreground">
            {cupping.brew_method ?? " "}
          </span>
        </div>
        <div className="flex shrink-0 items-start gap-1">
          {big !== null && (
            <span className="flex flex-col items-end">
              <span className="font-display text-[44px] leading-none">{big}</span>
              <span className="text-[13px] text-muted-foreground">
                {typeof cupping.overall_score === "number" ? t("Total") : t("Mean")}
              </span>
            </span>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" aria-label={t("More")}>
                <Ellipsis />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onSelect={onEdit}>
                <Pencil />
                {t("Change…")}
              </DropdownMenuItem>
              <DropdownMenuItem danger onSelect={onDelete}>
                <Trash2 />
                {t("Delete…")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      {scores.length > 0 && (
        <div className="grid grid-cols-[7.5rem_minmax(0,1fr)_2rem] items-center gap-x-3 gap-y-2 text-[14px]">
          {scores.map((s) => (
            <div key={s.key} className="contents">
              <span className="truncate text-muted-foreground">{t(s.name)}</span>
              <span className="h-2 overflow-hidden rounded-pill bg-surface-raised">
                <span
                  className="block h-full rounded-pill bg-brand"
                  style={{ width: `${s.value * 10}%` }}
                />
              </span>
              <span className="text-right tabular">{formatNumber(s.value)}</span>
            </div>
          ))}
        </div>
      )}
      {cupping.descriptors.length > 0 && (
        <ul className="flex flex-wrap gap-1.5" aria-label={t("Flavours")}>
          {cupping.descriptors.map((d) => (
            <li
              key={d}
              className="inline-flex h-7 items-center rounded-pill bg-surface-raised px-2.5 text-[14px]"
            >
              {d}
            </li>
          ))}
        </ul>
      )}
      {cupping.notes && <p className="text-[14px] text-muted-foreground">{cupping.notes}</p>}
    </li>
  );
}

/** A roast's tastings, newest first; changing and deleting them. */
export function TastingView({
  cuppings,
  onEdit,
  onChanged,
}: {
  cuppings: Cupping[];
  onEdit: (c: Cupping) => void;
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [deleting, setDeleting] = useState<Cupping | null>(null);

  if (cuppings.length === 0) {
    return (
      <div className="grid min-h-48 flex-1 place-items-center rounded-card border border-dashed border-border-strong p-10 text-center text-[15px] text-muted-foreground">
        {t("Not tasted yet. Taste it in its window and write down what you find.")}
      </div>
    );
  }

  const remove = async (c: Cupping) => {
    try {
      await api.deleteCupping(c.id);
      onChanged();
    } catch (e) {
      toast("critical", e instanceof ApiError ? e.message : String(e));
    }
  };

  return (
    <>
      <ul className="grid min-h-0 flex-1 grid-cols-2 content-start items-start gap-5 overflow-y-auto">
        {cuppings.map((c) => (
          <TastingCard
            key={c.id}
            cupping={c}
            onEdit={() => onEdit(c)}
            onDelete={() => setDeleting(c)}
          />
        ))}
      </ul>
      <AlertDialog open={deleting !== null} onOpenChange={(o) => !o && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("Delete tasting?")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("The scores, flavours and the note of this tasting are removed.")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => deleting && void remove(deleting)}
            >
              {t("Delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
