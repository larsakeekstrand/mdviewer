// Pure preset/settings helpers for PDF export. No DOM or Tauri imports, so this
// runs under `node --test` as well as in the WebView (mirrors export.js).

const FONT_SANS =
  '-apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif';
const FONT_SERIF = 'Georgia, "Iowan Old Style", "Times New Roman", Times, serif';
const FONT_MONO =
  'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace';

export const PRESETS = {
  clean: {
    label: "Clean",
    bodyFont: FONT_SANS,
    headingFont: FONT_SANS,
    baseSize: 11,
    lineHeight: 1.55,
    headingScale: 1.0,
    accent: "#0969da",
    margins: "normal",
    paper: "a4",
    justify: false,
    pageNumbers: "bottom-center",
  },
  report: {
    label: "Report",
    bodyFont: FONT_SERIF,
    headingFont: FONT_SANS,
    baseSize: 11,
    lineHeight: 1.6,
    headingScale: 1.05,
    accent: "#1f2328",
    margins: "wide",
    paper: "a4",
    justify: true,
    pageNumbers: "bottom-center",
  },
  compact: {
    label: "Compact",
    bodyFont: FONT_SANS,
    headingFont: FONT_SANS,
    baseSize: 9.5,
    lineHeight: 1.4,
    headingScale: 0.95,
    accent: "#0969da",
    margins: "narrow",
    paper: "a4",
    justify: false,
    pageNumbers: "bottom-right",
  },
};

const MONO = FONT_MONO;

const MARGINS = {
  narrow: { top: 12, right: 12, bottom: 12, left: 12 },
  normal: { top: 18, right: 18, bottom: 18, left: 18 },
  wide: { top: 25, right: 25, bottom: 25, left: 25 },
};

const PAPER = {
  a4: { w: 210, h: 297 },
  letter: { w: 215.9, h: 279.4 },
  legal: { w: 215.9, h: 355.6 },
};

// The persisted/exchanged settings keys (the subset the UI controls).
function settingsFromPreset(id) {
  const p = PRESETS[id] || PRESETS.clean;
  return {
    preset: PRESETS[id] ? id : "clean",
    baseSize: p.baseSize,
    paper: p.paper,
    margins: p.margins,
    pageNumbers: p.pageNumbers,
  };
}

export function presetIds() {
  return Object.keys(PRESETS);
}

export function presetDefaults(id) {
  return settingsFromPreset(id);
}

export function defaultSettings() {
  return presetDefaults("clean");
}

export function mergeSettings(base, overrides) {
  const out = { ...base };
  for (const [k, v] of Object.entries(overrides || {})) {
    if (v !== undefined && v !== null) out[k] = v;
  }
  return out;
}

export function clampBaseSize(pt) {
  const n = Number(pt);
  if (!Number.isFinite(n)) return defaultSettings().baseSize;
  return Math.min(16, Math.max(9, n));
}

export function marginMm(name) {
  return { ...(MARGINS[name] || MARGINS.normal) };
}

export function paperMm(name) {
  return { ...(PAPER[name] || PAPER.a4) };
}

// Look up the full preset record behind a settings object (for fonts/accent
// that aren't user-exposed knobs).
function presetRecord(settings) {
  return PRESETS[settings.preset] || PRESETS.clean;
}

/** CSS scoped to `.markdown-body`, applied both to the standalone HTML preview
 *  and (injected as a <style>) to the live #preview during the in-app PDF
 *  print. Typography + accent + left/right margins live here; paper size and
 *  top/bottom margins + page numbers are applied natively (not CSS). */
export function settingsToCss(settings) {
  const p = presetRecord(settings);
  const size = clampBaseSize(settings.baseSize);
  const m = marginMm(settings.margins);
  const justify = p.justify ? "\n  text-align: justify;" : "";
  return `.markdown-body {
  --pdf-accent: ${p.accent};
  font-family: ${p.bodyFont};
  font-size: ${size}pt;
  line-height: ${p.lineHeight};
  box-sizing: border-box;
  max-width: none;
  padding-left: ${m.left}mm;
  padding-right: ${m.right}mm;${justify}
}
.markdown-body h1, .markdown-body h2, .markdown-body h3,
.markdown-body h4, .markdown-body h5, .markdown-body h6 {
  font-family: ${p.headingFont};
  color: var(--pdf-accent);
  line-height: 1.25;
}
.markdown-body h1 { font-size: ${(2.0 * p.headingScale).toFixed(3)}em; }
.markdown-body h2 { font-size: ${(1.6 * p.headingScale).toFixed(3)}em; }
.markdown-body h3 { font-size: ${(1.3 * p.headingScale).toFixed(3)}em; }
.markdown-body a { color: var(--pdf-accent); text-decoration: underline; }
.markdown-body table th { background: color-mix(in srgb, var(--pdf-accent) 12%, transparent); }
.markdown-body pre, .markdown-body code { font-family: ${MONO}; }
.markdown-body pre { font-size: 0.85em; }
`;
}
