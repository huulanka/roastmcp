import { useState } from "react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { BackupStatus } from "@/api/types.ts";
import { Field, inputCls } from "@/components/Dialogs.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import { backupLine } from "@/lib/backup.ts";
import { cn } from "@/lib/utils.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function BackupForm({
  status,
  onStatus,
}: {
  status: BackupStatus | null;
  onStatus: (s: BackupStatus) => void;
}) {
  const { t, i18n } = useTranslation();
  const [dir, setDir] = useState(status?.dir ?? "");
  const [busy, setBusy] = useState(false);

  const changed = dir.trim() !== (status?.dir ?? "");
  const run = async (action: () => Promise<BackupStatus>, done: string) => {
    setBusy(true);
    try {
      onStatus(await action());
      toast("success", done);
    } catch (e) {
      toast("critical", errText(e));
      // A failed backup is recorded: show it.
      void api
        .backup()
        .then(onStatus)
        .catch(() => undefined);
    }
    setBusy(false);
  };
  const failed = status?.last ? !status.last.ok : false;

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        void run(() => api.setBackup({ dir: dir.trim() || null }), t("Saved"));
      }}
    >
      <Field label={t("Backup folder")}>
        <input
          className={inputCls}
          value={dir}
          placeholder="/Volumes/NAS/RoastMCP"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onChange={(e) => setDir(e.target.value)}
        />
      </Field>
      <p className={cn("text-[13px]", failed ? "text-danger-text" : "text-muted-foreground")}>
        {backupLine(status, t, i18n.language)}
      </p>
      <DialogFooter>
        <Button
          type="button"
          variant="secondary"
          disabled={busy || changed || !status?.dir}
          onClick={() => void run(() => api.runBackup(), t("Backup written"))}
        >
          {t("Back up now")}
        </Button>
        <Button type="submit" disabled={busy || !changed}>
          {t("Save")}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** Where the database is backed up to, and a backup right now (ADR 0003). */
export function BackupDialog({
  open,
  onOpenChange,
  status,
  onStatus,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  status: BackupStatus | null;
  onStatus: (s: BackupStatus) => void;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Backup")}</DialogTitle>
          <DialogDescription>
            {t(
              "A checked copy of all roasts, beans and profiles goes to this folder after every roast and once a day. It can be on a network drive.",
            )}
          </DialogDescription>
        </DialogHeader>
        {/* Mounted with the dialog, so the field starts from the stored folder. */}
        {open && <BackupForm status={status} onStatus={onStatus} />}
      </DialogContent>
    </Dialog>
  );
}
