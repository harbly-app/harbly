import { setTheme as setAppTheme } from "@tauri-apps/api/app";

/** Appearance preference: "system" follows the OS setting live */
export type ThemePref = "system" | "light" | "dark";

const KEY = "harbly.theme";
const media = window.matchMedia("(prefers-color-scheme: dark)");

export function initialThemePref(): ThemePref {
  const saved = localStorage.getItem(KEY);
  return saved === "light" || saved === "dark" ? saved : "system";
}

let current: ThemePref = "system";

function apply(pref: ThemePref) {
  const dark = pref === "dark" || (pref === "system" && media.matches);
  document.documentElement.classList.toggle("dark", dark);
  // Keep native chrome (window appearance, dialogs, menus) in sync; no-op outside Tauri
  setAppTheme(pref === "system" ? null : pref).catch(() => {});
}

/** Apply a preference to the DOM + native window. Persistence lives in the store (harbly.theme). */
export function applyThemePref(pref: ThemePref) {
  current = pref;
  apply(pref);
}

// Follow live OS appearance changes while in "system" mode
media.addEventListener("change", () => {
  if (current === "system") apply(current);
});
