/**
 * Theme selection: system, light or dark.
 *
 * Two layers cooperate, because neither covers everything:
 *
 * - The **page** owns the colours. It sets `data-theme` on `<html>`, which the token blocks in
 *   `styles.css` key off. This is the only mechanism that reaches every webview: the account
 *   webviews are separate views that load WhatsApp Web, and the overlay is a third one, and there
 *   is no cross-platform way to set a theme on a webview individually.
 * - The **native window** theme is set as well, so the title bar, scrollbars and native form
 *   controls match. It is best-effort: where the platform does not support it the page still looks
 *   right.
 *
 * With the choice set to `system`, the CSS media query in `styles.css` does the work on the first
 * paint — before this module runs — and this module only re-applies when the OS preference changes.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";

export type ThemeChoice = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

const LIGHT_QUERY = "(prefers-color-scheme: light)";

/** Only present inside Tauri; in a plain browser preview there is no native window to talk to. */
const inTauri = "isTauri" in window;

export function isThemeChoice(value: unknown): value is ThemeChoice {
  return value === "system" || value === "light" || value === "dark";
}

/** The theme actually in effect, resolving `system` against the OS preference. */
export function resolveTheme(choice: ThemeChoice): ResolvedTheme {
  if (choice !== "system") return choice;
  return window.matchMedia(LIGHT_QUERY).matches ? "light" : "dark";
}

/**
 * Applies a choice to the document, and mirrors it onto the native window.
 *
 * The attribute is removed entirely for `system` rather than set to the resolved value: that keeps
 * the CSS media query in charge, so the OS switching while the app runs is picked up by the
 * stylesheet itself instead of waiting for this module.
 */
export function applyTheme(choice: ThemeChoice): void {
  const root = document.documentElement;
  if (choice === "system") {
    root.removeAttribute("data-theme");
  } else {
    root.dataset.theme = choice;
  }

  if (!inTauri) return;
  // `null` tells the window to follow the system theme again.
  void getCurrentWindow()
    .setTheme(choice === "system" ? null : choice)
    .catch((error) => {
      // Unsupported on some platforms; the page colours are already applied, so this is not fatal.
      console.debug("[theme] native theme not applied", error);
    });
}

/**
 * Keeps the document in sync with the OS preference while `system` is selected.
 *
 * Returns an unsubscribe function. Re-subscribing on every change is avoided by listening once for
 * the lifetime of the page and reading the current choice from the callback.
 */
export function watchSystemTheme(current: () => ThemeChoice, onChange: () => void): () => void {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const listener = () => {
    if (current() === "system") onChange();
  };
  media.addEventListener("change", listener);
  return () => media.removeEventListener("change", listener);
}

/** Cycles system → light → dark → system, for the dock button. */
export function nextThemeChoice(choice: ThemeChoice): ThemeChoice {
  return choice === "system" ? "light" : choice === "light" ? "dark" : "system";
}

/** Human label for the current choice, used in the dock button's accessible name. */
export function themeLabel(choice: ThemeChoice): string {
  if (choice === "system") return `follow system (currently ${resolveTheme(choice)})`;
  return choice;
}
