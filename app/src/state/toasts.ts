import { useSyncExternalStore } from "react";

export type ToastKind = "info" | "warning" | "critical" | "success";
export interface Toast {
  id: number;
  kind: ToastKind;
  text: string;
}

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export function dismissToast(id: number): void {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

export function toast(kind: ToastKind, text: string): void {
  const id = nextId++;
  toasts = [...toasts.slice(-4), { id, kind, text }];
  emit();
  setTimeout(() => dismissToast(id), kind === "critical" ? 20_000 : 7_000);
}

export function useToasts(): Toast[] {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => toasts,
  );
}
