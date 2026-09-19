// Pure helpers for per-window behavior in a multi-window app (unit-tested).

export function anyDirty(tabs) {
  return tabs.some((t) => t.dirty);
}

// A `storage` event fires in every OTHER same-origin window when one window
// writes localStorage — that is how a theme toggle reaches the rest.
export function themeFromStorageEvent(e, key, isValidTheme) {
  if (e.key !== key || !isValidTheme(e.newValue)) return null;
  return e.newValue;
}
