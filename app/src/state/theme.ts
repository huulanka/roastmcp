const KEY = "roastmcp.theme";

export function currentTheme(): "dark" | "light" {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

export function applyStoredTheme(): void {
  let theme = "dark";
  try {
    theme = localStorage.getItem(KEY) ?? "dark";
  } catch {
    /* ignore */
  }
  document.documentElement.classList.toggle("dark", theme !== "light");
}

export function toggleTheme(): void {
  const next = currentTheme() === "dark" ? "light" : "dark";
  document.documentElement.classList.toggle("dark", next === "dark");
  try {
    localStorage.setItem(KEY, next);
  } catch {
    /* ignore */
  }
}
