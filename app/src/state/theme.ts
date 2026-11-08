import { useSyncExternalStore } from "react";

/** The user's choice; "system" follows the operating system. */
export type ThemeMode = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const KEY = "roastmcp.theme";
const listeners = new Set<() => void>();
let mode: ThemeMode = "system";
let resolved: ResolvedTheme = "dark";

function systemTheme(): ResolvedTheme {
  try {
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  } catch {
    return "dark";
  }
}

function apply(): void {
  resolved = mode === "system" ? systemTheme() : mode;
  document.documentElement.classList.toggle("dark", resolved === "dark");
  listeners.forEach((l) => l());
}

/** Reads the stored choice, applies it and follows system changes while on "system". */
export function initTheme(): void {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "light" || saved === "dark" || saved === "system") mode = saved;
  } catch {
    /* ignore */
  }
  apply();
  try {
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      if (mode === "system") apply();
    });
  } catch {
    /* no media queries */
  }
}

export function setThemeMode(next: ThemeMode): void {
  mode = next;
  try {
    localStorage.setItem(KEY, next);
  } catch {
    /* ignore */
  }
  apply();
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function useThemeMode(): ThemeMode {
  return useSyncExternalStore(subscribe, () => mode);
}

/** The theme actually shown; components that paint with tokens on a canvas redraw on change. */
export function useResolvedTheme(): ResolvedTheme {
  return useSyncExternalStore(subscribe, () => resolved);
}
