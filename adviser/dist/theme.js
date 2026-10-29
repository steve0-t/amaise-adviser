"use strict";

// Run before the stylesheet so a saved dark theme is applied before first paint.
(() => {
  const storageKey = "ameise-adviser-theme";
  const root = document.documentElement;
  const systemTheme = window.matchMedia ? window.matchMedia("(prefers-color-scheme: dark)") : null;
  const validTheme = (value) => value === "light" || value === "dark" ? value : null;
  let preference = null;
  let toggle;

  try { preference = validTheme(window.localStorage.getItem(storageKey)); } catch { /* Storage can be unavailable in private or restricted browsing. */ }

  function applyTheme() {
    const theme = preference || (systemTheme?.matches ? "dark" : "light");
    root.dataset.theme = theme;
    document.querySelector('meta[name="theme-color"]')?.setAttribute("content", theme === "dark" ? "#121316" : "#f2f1ee");
    if (toggle) {
      toggle.setAttribute("aria-pressed", String(theme === "dark"));
      toggle.setAttribute("title", `Switch to ${theme === "dark" ? "light" : "dark"} theme`);
    }
  }

  function connectToggle() {
    toggle = document.getElementById("theme-toggle");
    if (!toggle) return;
    toggle.addEventListener("click", () => {
      preference = root.dataset.theme === "dark" ? "light" : "dark";
      try { window.localStorage.setItem(storageKey, preference); } catch { /* Keep the choice for this page even when it cannot be saved. */ }
      applyTheme();
    });
    applyTheme();
  }

  applyTheme();
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", connectToggle, { once: true });
  else connectToggle();

  if (systemTheme?.addEventListener) systemTheme.addEventListener("change", applyTheme);
  else if (systemTheme?.addListener) systemTheme.addListener(applyTheme);

  window.addEventListener("storage", (event) => {
    if (event.key === storageKey || event.key === null) {
      // Ignore sessionStorage and unrelated storage areas.
      try { if (event.storageArea && event.storageArea !== window.localStorage) return; } catch { return; }
      preference = validTheme(event.newValue);
      applyTheme();
    }
  });
})();
