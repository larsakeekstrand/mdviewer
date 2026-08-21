// Pure file-type detection (no DOM) so it's unit-testable under `node --test`.

export const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif|bmp|ico|svg)$/i;
export const MARKDOWN_EXT = /\.(md|markdown|mdown|mkd|mkdn)$/i;

export const PDF_EXT = /\.pdf$/i;
export const SHEET_EXT = /\.(xlsx|xlsm|xlsb|xls|ods)$/i;

export function isImagePath(path) {
  return IMAGE_EXT.test(path || "");
}

export function isMarkdownPath(path) {
  return MARKDOWN_EXT.test(path || "");
}

export function isPdfPath(path) {
  return PDF_EXT.test(path || "");
}

export function isSheetPath(path) {
  return SHEET_EXT.test(path || "");
}

// A "code view" tab is any file that isn't markdown and isn't an image —
// it renders as syntax-highlighted, line-numbered text.
export function isCodeView(path) {
  return !isImagePath(path) && !isMarkdownPath(path);
}

/** How a file is shown. `code` is the fallback on purpose: an extension we
 *  don't recognize degrades to source view instead of failing to open. */
export function viewKind(path) {
  const p = path || "";
  if (MARKDOWN_EXT.test(p)) return "markdown";
  if (IMAGE_EXT.test(p)) return "image";
  if (PDF_EXT.test(p)) return "pdf";
  if (SHEET_EXT.test(p)) return "sheet";
  return "code";
}

// One row per kind, one column per question a call site asks. Adding a view
// type is a row here; it is not an edit at every gate in app.js.
const CAPABILITIES = {
  markdown: {
    editable: true, splitPreview: true, rawToggle: true, annotatable: true,
    retainable: true, exportable: true, ownRenderPath: false, cacheBust: false,
  },
  code: {
    editable: true, splitPreview: false, rawToggle: false, annotatable: false,
    retainable: true, exportable: false, ownRenderPath: false, cacheBust: false,
  },
  image: {
    editable: false, splitPreview: false, rawToggle: false, annotatable: false,
    retainable: false, exportable: false, ownRenderPath: true, cacheBust: true,
  },
  pdf: {
    editable: false, splitPreview: false, rawToggle: false, annotatable: false,
    retainable: false, exportable: false, ownRenderPath: true, cacheBust: true,
  },
  sheet: {
    editable: false, splitPreview: false, rawToggle: false, annotatable: false,
    retainable: true, exportable: true, ownRenderPath: false, cacheBust: false,
  },
};

function cap(kind, name) {
  const row = CAPABILITIES[kind] || CAPABILITIES.code;
  return row[name] === true;
}

export const isEditable = (kind) => cap(kind, "editable");
export const hasSplitPreview = (kind) => cap(kind, "splitPreview");
export const hasRawToggle = (kind) => cap(kind, "rawToggle");
export const isAnnotatable = (kind) => cap(kind, "annotatable");
export const isRetainable = (kind) => cap(kind, "retainable");
export const isExportable = (kind) => cap(kind, "exportable");
export const rendersFromDisk = (kind) => cap(kind, "ownRenderPath");
export const bustsCacheOnChange = (kind) => cap(kind, "cacheBust");
