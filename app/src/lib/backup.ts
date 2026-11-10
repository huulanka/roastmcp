import type { TFunction } from "i18next";

import type { BackupStatus } from "@/api/types.ts";
import { formatNumber } from "@/lib/units.ts";

/** How the last backup went, in one line. */
export function backupLine(status: BackupStatus | null, t: TFunction, language: string): string {
  if (!status?.dir) return t("No backup folder set.");
  const last = status.last;
  if (!last) return t("No backup made yet.");
  const when = new Date(last.at).toLocaleString(language, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  if (!last.ok) return t("Last backup failed ({{when}}): {{error}}", { when, error: last.error });
  return t("Last backup: {{when}} · {{size}} MB", {
    when,
    size: formatNumber((last.bytes ?? 0) / 1_000_000),
  });
}
