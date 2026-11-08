import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import { de } from "./de.ts";

const KEY = "roastmcp.lang";

export type Lang = "en" | "de";

function initial(): Lang {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "en" || saved === "de") return saved;
  } catch {
    /* ignore */
  }
  return navigator.language.toLowerCase().startsWith("de") ? "de" : "en";
}

void i18n.use(initReactI18next).init({
  lng: initial(),
  fallbackLng: "en",
  keySeparator: false,
  nsSeparator: false,
  resources: { de: { translation: de } },
  interpolation: { escapeValue: false },
  returnEmptyString: false,
});

export function setLang(lang: Lang): void {
  void i18n.changeLanguage(lang);
  try {
    localStorage.setItem(KEY, lang);
  } catch {
    /* ignore */
  }
}

export default i18n;
