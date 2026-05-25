// Pure theme helpers (no DOM / localStorage access) so they're unit-testable
// under `node --test`. The DOM/storage wiring lives in app.js.

export const THEME_KEY = "mdviewer.theme";

export function isValidTheme(value) {
  return value === "light" || value === "dark";
}

// Stored preference wins when valid; otherwise fall back to the OS theme.
export function resolveTheme(stored, osTheme) {
  return isValidTheme(stored) ? stored : osTheme;
}

export function nextTheme(current) {
  return current === "dark" ? "light" : "dark";
}

// Button face shows the theme a click switches TO, matching the Raw button's
// "label = what clicking does" convention.
export function themeButtonFace(theme) {
  return theme === "dark"
    ? { icon: "☀", label: "Switch to light theme" }
    : { icon: "☾", label: "Switch to dark theme" };
}
