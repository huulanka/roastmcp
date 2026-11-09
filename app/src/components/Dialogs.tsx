import { useEffect, useState } from "react";
import { Copy } from "lucide-react";
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
import type { RoastGoal } from "@/api/types.ts";
import { labelText } from "@/lib/batch.ts";
import { bluetoothName, lastPort, preferredPort, rememberPort, usefulPorts } from "@/lib/ports.ts";
import { formatNumber, weightLossPct } from "@/lib/units.ts";
import { cn } from "@/lib/utils.ts";
import { applySnapshot } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

export const inputCls =
  "h-touch w-full rounded-lg border border-border-strong bg-background px-3 text-[15px] text-foreground outline-none transition-colors select-text placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/40 disabled:opacity-60 aria-invalid:border-danger-text";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

/** A number field with its unit inside, on the right: "200 g", "210 °C". */
export function UnitInput({
  unit,
  className,
  ...props
}: React.ComponentProps<"input"> & { unit: string }) {
  return (
    <span className="flex h-touch items-center gap-2 rounded-lg border border-border-strong bg-background pr-3 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/40">
      <input
        className={cn(
          "h-full w-full min-w-0 rounded-lg bg-transparent pl-3 text-right tabular text-[15px] text-foreground outline-none select-text",
          className,
        )}
        {...props}
      />
      <span className="shrink-0 text-[14px] text-muted-foreground">{unit}</span>
    </span>
  );
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
      .then((p) => alive && setPorts(usefulPorts(p)))
      .catch((e) => toast("warning", errText(e)));
    return () => {
      alive = false;
    };
  }, [open]);

  const port = chosen ?? preferredPort(ports, lastPort());

  const connect = async () => {
    setBusy(true);
    try {
      applySnapshot(await api.connect(simulator, simulator ? undefined : port));
      if (!simulator && port) rememberPort(port);
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
            {t(
              "Pick how to reach the roaster (its serial port or, on a Mac, Bluetooth direct), or use the built-in simulator.",
            )}
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
          <Field label={t("Port")}>
            <select
              className={inputCls}
              value={port ?? ""}
              onChange={(e) => setChosen(e.target.value)}
            >
              {ports.length === 0 && <option value="">{t("No serial ports found")}</option>}
              {ports.map((p) => {
                const bt = bluetoothName(p);
                return (
                  <option key={p} value={p}>
                    {bt ? t("{{name}} · Bluetooth direct", { name: bt }) : p}
                  </option>
                );
              })}
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
const GOAL_LABEL: Record<RoastGoal, string> = {
  espresso: "Espresso",
  filter: "Filter",
  omni: "Omni",
  other: "Own",
};

export function SavedDialog({ roastId, onClose }: { roastId: string | null; onClose: () => void }) {
  const { t } = useTranslation();
  const [title, setTitle] = useState("");
  const [green, setGreen] = useState<number | null>(null);
  const [label, setLabel] = useState<string | null>(null);
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
        // The text for the bag's label, when the roast has a batch number.
        setLabel(
          r.batch?.batch_no
            ? labelText(
                {
                  batchNo: r.batch.batch_no,
                  beans: r.batch.beans ?? r.title,
                  goal: r.plan?.goal ?? null,
                  roastedAt: r.roasted_at ?? new Date().toISOString(),
                },
                t,
                (g) => t(GOAL_LABEL[g]),
              )
            : null,
        );
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [roastId, t]);

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
          {label && (
            <div className="flex items-start justify-between gap-3 rounded-lg bg-surface-raised px-3.5 py-3">
              <div className="flex min-w-0 flex-col gap-1">
                <span className="text-[13px] text-muted-foreground">{t("Label")}</span>
                <span className="tabular text-[15px] whitespace-pre-line">{label}</span>
              </div>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() =>
                  void navigator.clipboard
                    .writeText(label)
                    .then(() => toast("success", t("Label copied")))
                    .catch((e) => toast("critical", errText(e)))
                }
              >
                <Copy />
                {t("Copy")}
              </Button>
            </div>
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
