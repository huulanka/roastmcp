import { useCallback, useEffect, useState } from "react";
import i18n from "i18next";
import { Check, Pencil, Plus, Trash2, Undo2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { api, ApiError } from "@/api/client.ts";
import type { ChecklistItem, MaintenanceLogEntry, MaintenanceTask } from "@/api/types.ts";
import { Field, inputCls, UnitInput } from "@/components/Dialogs.tsx";
import { Button } from "@/components/ui/button.tsx";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog.tsx";
import {
  checklistText,
  intervalText,
  parseInterval,
  remaining,
  sinceText,
  SUGGESTED_TASKS,
} from "@/lib/care.ts";
import { cn } from "@/lib/utils.ts";
import { setCareDue } from "@/state/care.ts";
import { toast } from "@/state/toasts.ts";

function errText(e: unknown): string {
  return e instanceof ApiError ? e.message : String(e);
}

function shortDate(iso: string): string {
  return new Intl.DateTimeFormat(i18n.language, { day: "numeric", month: "short" }).format(
    new Date(iso),
  );
}

function dateTime(iso: string): string {
  return new Intl.DateTimeFormat(i18n.language, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(iso));
}

const chipCls =
  "inline-flex h-6.5 shrink-0 items-center rounded-pill px-2.5 text-[13px] whitespace-nowrap";

/** One maintenance task: what it is, how far it is, and "done". */
function TaskCard({
  task,
  busy,
  onDone,
  onEdit,
}: {
  task: MaintenanceTask;
  busy: boolean;
  onDone: () => void;
  onEdit: () => void;
}) {
  const { t } = useTranslation();
  const left = remaining(task, t);
  return (
    <li
      className={cn(
        "flex flex-col gap-3 rounded-card border bg-surface p-card",
        task.due ? "border-brand/40" : "border-card-border",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="truncate text-[15px] font-semibold">{task.name}</span>
          <span className="truncate text-[13px] text-muted-foreground">
            {[intervalText(task, t), task.description].filter(Boolean).join(" · ")}
          </span>
        </div>
        <span
          className={cn(
            chipCls,
            task.due ? "bg-brand/12 text-brand-text" : "bg-surface-raised text-muted-foreground",
          )}
        >
          {task.due ? t("due") : (left?.text ?? t("logged only"))}
        </span>
      </div>
      <div className="flex items-center justify-between gap-3">
        {left ? (
          <span className="flex min-w-0 flex-1 items-center gap-3">
            <span className="h-2 flex-1 overflow-hidden rounded-pill bg-surface-raised">
              <span
                className="block h-full rounded-pill bg-muted-foreground"
                style={{ width: `${Math.round(left.share * 100)}%` }}
              />
            </span>
            <span className="shrink-0 tabular text-[13px] text-muted-foreground">{left.count}</span>
          </span>
        ) : (
          <span className="truncate text-[13px] text-muted-foreground">
            {sinceText(task, t, shortDate)}
          </span>
        )}
        <span className="flex shrink-0 gap-1">
          <Button
            variant="ghost"
            size="icon"
            aria-label={t("Change {{name}}", { name: task.name })}
            onClick={onEdit}
          >
            <Pencil />
          </Button>
          <Button
            variant={task.due ? "default" : "secondary"}
            size="sm"
            disabled={busy}
            onClick={onDone}
          >
            <Check />
            {t("Done")}
          </Button>
        </span>
      </div>
    </li>
  );
}

/** Adding a task, or changing one. */
function TaskDialog({
  task,
  onClose,
  onSaved,
}: {
  /** The task to change; `null` adds one. */
  task: MaintenanceTask | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(task?.name ?? "");
  const [description, setDescription] = useState(task?.description ?? "");
  const [roasts, setRoasts] = useState(task?.interval_roasts ? String(task.interval_roasts) : "");
  const [days, setDays] = useState(task?.interval_days ? String(task.interval_days) : "");
  const [busy, setBusy] = useState(false);

  const valid = (text: string) => text.trim() === "" || parseInterval(text) !== null;
  const canSave = name.trim() !== "" && valid(roasts) && valid(days);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await action();
      onSaved();
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  const save = () => {
    const body = {
      name: name.trim(),
      description: description.trim(),
      // 0 removes an interval.
      interval_roasts: parseInterval(roasts) ?? 0,
      interval_days: parseInterval(days) ?? 0,
    };
    return run(() =>
      task ? api.patchMaintenanceTask(task.id, body) : api.addMaintenanceTask(body),
    );
  };

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[32rem]">
        <form
          className="flex flex-col gap-5"
          onSubmit={(e) => {
            e.preventDefault();
            if (canSave) void save();
          }}
        >
          <DialogHeader>
            <DialogTitle>{task ? t("Change task") : t("New maintenance task")}</DialogTitle>
            <DialogDescription>
              {t(
                "Due after the roasts or the days, whichever comes first. Without either it is only logged.",
              )}
            </DialogDescription>
          </DialogHeader>
          <Field label={t("Task")}>
            <input
              className={inputCls}
              value={name}
              autoFocus
              placeholder={t("Empty the chaff collector")}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>
          <Field label={t("Note")}>
            <input
              className={inputCls}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Field>
          <div className="grid grid-cols-2 gap-3">
            <Field label={t("Due after")}>
              <UnitInput
                unit={t("roasts")}
                inputMode="numeric"
                value={roasts}
                aria-invalid={!valid(roasts)}
                onChange={(e) => setRoasts(e.target.value)}
              />
            </Field>
            <Field label={t("or after")}>
              <UnitInput
                unit={t("days")}
                inputMode="numeric"
                value={days}
                aria-invalid={!valid(days)}
                onChange={(e) => setDays(e.target.value)}
              />
            </Field>
          </div>
          <DialogFooter className="items-center sm:justify-between">
            {task ? (
              <Button
                type="button"
                variant="ghost"
                disabled={busy}
                onClick={() =>
                  void run(() => api.patchMaintenanceTask(task.id, { archived: true }))
                }
              >
                {t("Remove task")}
              </Button>
            ) : (
              <Button type="button" variant="ghost" onClick={onClose}>
                {t("Cancel")}
              </Button>
            )}
            <Button type="submit" disabled={busy || !canSave}>
              {t("Save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The person's checklist items: remove one, add one. */
function ChecklistEditor({
  items,
  onChange,
}: {
  items: ChecklistItem[];
  onChange: (items: ChecklistItem[]) => void;
}) {
  const { t } = useTranslation();
  const [adding, setAdding] = useState(false);
  const [text, setText] = useState("");

  const add = () => {
    const trimmed = text.trim();
    if (trimmed) onChange([...items, { text: trimmed }]);
    setText("");
    setAdding(false);
  };

  return (
    <section aria-label={t("Check before roasting")} className="flex min-h-0 flex-col gap-3.5">
      <div className="flex h-touch items-center justify-between gap-3">
        <h2 className="text-[15px] font-medium text-muted-foreground">
          {t("Check before roasting")}
        </h2>
        <Button variant="secondary" size="sm" onClick={() => setAdding(true)}>
          <Plus />
          {t("Item")}
        </Button>
      </div>
      <ul className="flex min-h-0 flex-col gap-2 overflow-y-auto">
        {items.map((item, i) => (
          <li
            key={`${item.key ?? ""}:${item.text}`}
            className="flex min-h-13 items-center gap-2 rounded-xl border border-border py-1 pr-1 pl-3.5 text-[15px]"
          >
            <span className="min-w-0 flex-1">{checklistText(item, t)}</span>
            <Button
              variant="ghost"
              size="icon"
              aria-label={t("Remove “{{text}}”", { text: checklistText(item, t) })}
              onClick={() => onChange(items.filter((_, j) => j !== i))}
            >
              <Trash2 />
            </Button>
          </li>
        ))}
        {adding && (
          <li>
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                add();
              }}
            >
              <input
                className={inputCls}
                value={text}
                autoFocus
                aria-label={t("New item")}
                placeholder={t("Window open")}
                onChange={(e) => setText(e.target.value)}
                onKeyDown={(e) => e.key === "Escape" && setAdding(false)}
              />
              <Button type="submit" variant="secondary">
                {t("Add")}
              </Button>
            </form>
          </li>
        )}
      </ul>
      <p className="text-[13px] text-muted-foreground">
        {t(
          "The start dialog shows these items before every roast. Maintenance that is due is added there by itself.",
        )}
      </p>
    </section>
  );
}

/** The Care screen (its own tab): maintenance tasks, their log and the checklist before a roast. */
export function CareScreen() {
  const { t } = useTranslation();
  const [tasks, setTasks] = useState<MaintenanceTask[] | null>(null);
  const [log, setLog] = useState<MaintenanceLogEntry[]>([]);
  const [items, setItems] = useState<ChecklistItem[]>([]);
  const [busy, setBusy] = useState(false);
  /** The task being changed, or "new". */
  const [editing, setEditing] = useState<MaintenanceTask | "new" | null>(null);

  /** Counts up to read everything again. */
  const [tick, setTick] = useState(0);
  const load = useCallback(() => setTick((n) => n + 1), []);

  useEffect(() => {
    let alive = true;
    Promise.all([api.maintenance(), api.maintenanceLog(), api.checklist()])
      .then(([tasksNow, logNow, checklist]) => {
        if (!alive) return;
        setTasks(tasksNow);
        setLog(logNow);
        setItems(checklist.items);
        setCareDue(tasksNow.filter((x) => x.due).length);
      })
      .catch((e) => {
        if (!alive) return;
        setTasks((now) => now ?? []);
        toast("critical", errText(e));
      });
    return () => {
      alive = false;
    };
  }, [tick]);

  const run = async (action: () => Promise<unknown>, done?: string) => {
    setBusy(true);
    try {
      await action();
      if (done) toast("success", done);
      load();
    } catch (e) {
      toast("critical", errText(e));
    } finally {
      setBusy(false);
    }
  };

  const saveItems = (next: ChecklistItem[]) => {
    setItems(next);
    void run(() => api.setChecklist(next));
  };

  return (
    <main className="grid min-h-0 flex-1 gap-section overflow-y-auto p-gutter lg:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)_minmax(0,1fr)] lg:overflow-hidden">
      <section aria-label={t("Maintenance")} className="flex min-h-0 flex-col gap-3.5">
        <div className="flex h-touch items-center justify-between gap-3">
          <h2 className="text-[15px] font-medium text-muted-foreground">{t("Maintenance")}</h2>
          <Button variant="secondary" size="sm" onClick={() => setEditing("new")}>
            <Plus />
            {t("Task")}
          </Button>
        </div>
        {tasks === null ? (
          <p className="text-[15px] text-muted-foreground">{t("Loading…")}</p>
        ) : tasks.length === 0 ? (
          <div className="flex flex-col items-start gap-4 rounded-card border border-dashed border-border-strong p-6 text-[15px]">
            <span className="text-muted-foreground">
              {t("No maintenance tasks yet. A start for most drum roasters:")}
            </span>
            <ul className="flex flex-col gap-1.5">
              {SUGGESTED_TASKS.map((s) => (
                <li key={s.name}>
                  <span className="font-medium">{t(s.name)}</span>
                  <span className="text-muted-foreground"> · {intervalText(s, t)}</span>
                </li>
              ))}
            </ul>
            <Button
              disabled={busy}
              onClick={() =>
                void run(async () => {
                  for (const s of SUGGESTED_TASKS) {
                    await api.addMaintenanceTask({ ...s, name: t(s.name) });
                  }
                })
              }
            >
              {t("Take these over")}
            </Button>
          </div>
        ) : (
          <ul className="flex min-h-0 flex-col gap-3.5 overflow-y-auto">
            {tasks.map((task) => (
              <TaskCard
                key={task.id}
                task={task}
                busy={busy}
                onEdit={() => setEditing(task)}
                onDone={() =>
                  void run(
                    () => api.maintenanceDone(task.id),
                    t("Noted: {{name}}", { name: task.name }),
                  )
                }
              />
            ))}
          </ul>
        )}
      </section>

      <section aria-label={t("Log")} className="flex min-h-0 flex-col gap-3.5">
        <h2 className="flex h-touch items-center text-[15px] font-medium text-muted-foreground">
          {t("Log")}
        </h2>
        {log.length === 0 ? (
          <p className="text-[15px] text-muted-foreground">
            {t("Nothing logged yet. “Done” on a task writes it down here.")}
          </p>
        ) : (
          <ul className="flex min-h-0 flex-col overflow-y-auto rounded-card border border-card-border bg-surface px-card py-1.5">
            {log.map((entry) => (
              <li
                key={entry.id}
                className="flex items-center gap-2 border-b border-border py-2.5 last:border-b-0"
              >
                <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="truncate text-[15px]">{entry.task_name}</span>
                  <span className="truncate tabular text-[13px] text-muted-foreground">
                    {[dateTime(entry.done_at), entry.note].filter(Boolean).join(" · ")}
                  </span>
                </span>
                <Button
                  variant="ghost"
                  size="icon"
                  disabled={busy}
                  aria-label={t("Take back: {{name}}, {{when}}", {
                    name: entry.task_name,
                    when: dateTime(entry.done_at),
                  })}
                  title={t("Take back")}
                  onClick={() => void run(() => api.deleteMaintenanceLog(entry.id))}
                >
                  <Undo2 />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <ChecklistEditor items={items} onChange={saveItems} />

      {editing && (
        <TaskDialog
          task={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            load();
          }}
        />
      )}
    </main>
  );
}
