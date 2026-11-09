import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { presenceGong, spaceConfirms } from "@/lib/roast.ts";
import { confirmPresence } from "@/state/actions.ts";
import { gong } from "@/state/audio.ts";
import { useLive } from "@/state/live.ts";
import { toast } from "@/state/toasts.ts";

/** Keys typed into a field stay there. */
function typing(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  return el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName);
}

/**
 * Calls a person back before a presence confirmation is due (ADR 0018): a
 * gong 60 s and 20 s before, whenever an AI or the preheat holds control,
 * and at once when an AI asks for control (ADR 0019).
 * The space bar confirms presence while the app has focus, wherever focus
 * is, except in a text field. Only a person can press it; an AI cannot.
 */
export function PresenceWatcher() {
  const { t } = useTranslation();
  const { snapshot } = useLive();
  const prevDue = useRef<number | null>(null);
  const prevAsking = useRef<string | null>(null);
  const latest = useRef(snapshot);

  // An AI asks for control: the gong at once, so the person looks up from the
  // machine (ADR 0019). The alarm's toast says who asks.
  const asking = snapshot?.control_request ?? null;
  useEffect(() => {
    if (asking && asking !== prevAsking.current) gong(true);
    prevAsking.current = asking;
  }, [asking]);

  useEffect(() => {
    latest.current = snapshot;
    const s = snapshot?.safety;
    if (!s) return;
    const mark = presenceGong(prevDue.current, s.presence_due_in_s, s.lease_holder);
    prevDue.current = s.presence_due_in_s;
    if (mark === null) return;
    gong(mark === 20);
    toast(
      "warning",
      t("Presence due in {{s}} s: press Space or “I'm at the roaster”.", { s: mark }),
    );
  }, [snapshot, t]);

  useEffect(() => {
    // A focused button would also take the space (on key up): the press is
    // ours from key down to key up.
    let ours = false;
    const isSpace = (e: KeyboardEvent) => e.key === " " || e.code === "Space";
    const down = (e: KeyboardEvent) => {
      if (!isSpace(e) || e.ctrlKey || e.metaKey || e.altKey || typing(e.target)) return;
      const s = latest.current;
      if (!s || !spaceConfirms(s)) return;
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat || ours) return;
      ours = true;
      void confirmPresence();
    };
    const up = (e: KeyboardEvent) => {
      if (!isSpace(e) || !ours) return;
      ours = false;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
    };
  }, []);

  return null;
}
