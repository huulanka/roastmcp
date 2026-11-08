import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import { Button } from "@/components/ui/button.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import { applySnapshot } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

export const inputCls =
  "h-9 w-full rounded-lg border border-input bg-input/40 px-3 text-sm outline-none transition-colors select-text placeholder:text-subtle-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/40";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5 text-xs font-medium text-muted-foreground">
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
        <label className="flex items-center gap-2.5 text-sm">
          <input
            type="checkbox"
            checked={simulator}
            onChange={(e) => setSimulator(e.target.checked)}
            className="size-4 accent-[var(--brand)]"
          />
          {t("Use simulator")}
        </label>
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
