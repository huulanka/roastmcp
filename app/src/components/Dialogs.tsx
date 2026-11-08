import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import { formatNumber, weightLossPct } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { applySnapshot } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

export const inputCls =
  "h-touch w-full rounded-lg border border-border-strong bg-background px-3 text-[15px] text-foreground outline-none transition-colors select-text placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/40 disabled:opacity-60 aria-invalid:border-danger-text";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-2 text-[13px] text-muted-foreground">
      {label}
      {children}
    </label>
  );
}

export function ConnectDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useTranslation();
  const [simulator, setSimulator] = useState(false);
  const [ports, setPorts] = useState<string[]>([]);
  const [chosen, setChosen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    let alive = true;
    api
      .ports()
      .then((p) => alive && setPorts(p))
      .catch((e) => toast("warning", errText(e)));
    return () => {
      alive = false;
    };
  }, [open]);

  const preferred = ports.find((p) => p.toUpperCase().includes("AKROASTER")) ?? ports[0];
  const port = chosen ?? preferred;

  const connect = async () => {
    setBusy(true);
    try {
      applySnapshot(await api.connect(simulator, simulator ? undefined : port));
      onOpenChange(false);
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Connect to roaster")}</DialogTitle>
          <DialogDescription>
            {t("Pick the serial port of the roaster, or use the built-in simulator.")}
          </DialogDescription>
        </DialogHeader>
        <div
          role="radiogroup"
          aria-label={t("Connection")}
          className="grid grid-cols-2 gap-1 rounded-lg bg-segment-track p-1"
        >
          {[false, true].map((sim) => (
            <button
              key={String(sim)}
              type="button"
              role="radio"
              aria-checked={simulator === sim}
              onClick={() => setSimulator(sim)}
              className={cn(
                "h-touch cursor-pointer rounded-md text-[15px] transition-colors duration-150 outline-none focus-visible:ring-3 focus-visible:ring-ring/50",
                simulator === sim
                  ? "bg-surface-selected font-medium text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {sim ? t("Simulator") : t("Roaster")}
            </button>
          ))}
        </div>
        {!simulator && (
          <Field label={t("Serial port")}>
            <select
              className={inputCls}
              value={port ?? ""}
              onChange={(e) => setChosen(e.target.value)}
            >
              {ports.length === 0 && <option value="">{t("No serial ports found")}</option>}
              {ports.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          </Field>
        )}
        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("Cancel")}
          </Button>
          <Button disabled={busy || (!simulator && !port)} onClick={() => void connect()}>
            {busy ? t("Connecting…") : t("Connect")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function StartDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useTranslation();
  const [title, setTitle] = useState("");
  const [weight, setWeight] = useState("");
  const [beans, setBeans] = useState("");
  const [busy, setBusy] = useState(false);

  const start = async () => {
    setBusy(true);
    try {
      const w = parseFloat(weight.replace(",", "."));
      applySnapshot(
        await api.startRecording({
          title: title.trim() || undefined,
          green_weight_g: Number.isFinite(w) ? w : undefined,
          beans: beans.trim() || undefined,
        }),
      );
      onOpenChange(false);
      setTitle("");
      setWeight("");
      setBeans("");
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            void start();
          }}
        >
          <DialogHeader>
            <DialogTitle>{t("Start recording")}</DialogTitle>
            <DialogDescription>{t("All fields are optional.")}</DialogDescription>
          </DialogHeader>
          <Field label={t("Title")}>
            <input className={inputCls} value={title} onChange={(e) => setTitle(e.target.value)} />
          </Field>
          <Field label={t("Green weight (g)")}>
            <input
              className={inputCls}
              inputMode="decimal"
              value={weight}
              onChange={(e) => setWeight(e.target.value)}
            />
          </Field>
          <Field label={t("Beans")}>
            <input className={inputCls} value={beans} onChange={(e) => setBeans(e.target.value)} />
          </Field>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t("Cancel")}
            </Button>
            <Button type="submit" disabled={busy}>
              {t("Start")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function StopDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
}) {
  const { t } = useTranslation();
  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t("Stop recording?")}</AlertDialogTitle>
          <AlertDialogDescription>
            {t(
              "The roast will be saved to the archive. Make sure the beans have been dropped and cooled.",
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{t("Cancel")}</AlertDialogCancel>
          <AlertDialogAction
            onClick={async () => {
              try {
                const r = await api.stopRecording();
                toast("success", t("Roast saved ({{id}})", { id: r.id }));
              } catch (e) {
                toast("critical", errText(e));
              }
            }}
          >
            {t("Stop and save")}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

/** Shown right after the drop: the roast is saved; the roasted weight is the one thing still to add. */
export function SavedDialog({ roastId, onClose }: { roastId: string | null; onClose: () => void }) {
  const { t } = useTranslation();
  const [title, setTitle] = useState("");
  const [green, setGreen] = useState<number | null>(null);
  const [weight, setWeight] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!roastId) return;
    let cancelled = false;
    void api
      .roast(roastId)
      .then((r) => {
        if (cancelled) return;
        setTitle(r.title ?? "");
        setGreen(r.batch?.green_weight_g ?? null);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [roastId]);

  const value = parseFloat(weight.replace(",", "."));
  const valid = Number.isFinite(value) && value > 0;
  const loss = valid ? weightLossPct(green, value) : null;

  const save = async () => {
    if (!roastId || !valid) return;
    setBusy(true);
    try {
      await api.patchRoast(roastId, { roasted_weight_g: value });
      toast("success", t("Weight saved"));
      onClose();
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={roastId !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("Roast saved")}</DialogTitle>
          <DialogDescription>
            {title}
            {" · "}
            {t("The recording ended at the drop. Cooling runs from the controls.")}
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <Field label={t("Roasted weight (g)")}>
            <input
              className={inputCls}
              inputMode="decimal"
              autoFocus
              value={weight}
              onChange={(e) => setWeight(e.target.value)}
            />
          </Field>
          {loss !== null && (
            <p className="tabular text-[15px] text-muted-foreground">
              {t("Weight loss")}: {formatNumber(loss, 1)} %
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              {t("Later")}
            </Button>
            <Button type="submit" disabled={!valid || busy}>
              {t("Save weight")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
