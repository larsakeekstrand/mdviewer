// Per-tab retained render: the detached child nodes of #preview, so returning
// to a tab reattaches an already-painted document instead of repainting it.
// DOM-free — values are opaque, which is what keeps the policy unit-testable.
//
// Bounded by entry count rather than bytes: detached nodes have no cheap size
// measure, and the cost that matters (live listeners, SVG, KaTeX spans) tracks
// documents, not characters.

export class DomCache {
  constructor(maxEntries) {
    this.maxEntries = maxEntries;
    this.entries = new Map();
  }

  get(path) {
    const hit = this.entries.get(path);
    if (!hit) return null;
    this.entries.delete(path);
    this.entries.set(path, hit);
    return hit;
  }

  set(path, value) {
    this.entries.delete(path);
    this.entries.set(path, value);
    for (const key of this.entries.keys()) {
      if (this.entries.size <= this.maxEntries) break;
      this.entries.delete(key);
    }
  }

  delete(path) {
    this.entries.delete(path);
  }

  clear() {
    this.entries.clear();
  }
}

/** Whether what is currently painted may be kept for `tab`.
 *
 *  `live` describes what the preview actually shows — set by the paint path,
 *  cleared by the image/error/empty paths. Requiring it to match the tab is
 *  what stops a half-finished raw toggle, an editor-buffer preview, or an
 *  error panel from being retained as if it were the document. */
export function canRetain({ tab, live, theme, exporting }) {
  if (!tab || !live || exporting) return false;
  if (tab.editing) return false;
  if (!live.fromDisk) return false;
  return live.path === tab.path && live.raw === tab.raw && live.theme === theme;
}

/** Whether a stored entry can be reattached for the current view. */
export function entryUsable(entry, { raw, theme }) {
  if (!entry || !entry.fragment) return false;
  return entry.raw === raw && entry.theme === theme;
}
