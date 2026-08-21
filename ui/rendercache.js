// Per-tab render cache: rendered HTML keyed by (path, theme, raw view), paired
// with the backend's file stamp so a revisit can ask "still this version?" and
// skip the markdown/syntax-highlight pass entirely. DOM-free and IPC-free.
//
// Bounded by total HTML size, not entry count: a single large document's HTML
// carries syntect's inline styles and can run to megabytes, so counting tabs
// would be a poor proxy for memory. Eviction is least-recently-used, relying on
// Map preserving insertion order — a read re-inserts to move the entry to the
// young end.
//
// The budget is measured in CHARACTERS (`String.length`, UTF-16 code units),
// not bytes — an O(1) property of the string, where a true byte count would
// mean encoding every render just to size it. A JS engine stores those at up to
// two bytes each, so the resident cost of a full cache is up to ~2x the budget;
// size the constant with that in mind.

export class RenderCache {
  constructor(maxChars) {
    this.maxChars = maxChars;
    this.chars = 0;
    this.entries = new Map();
  }

  static key(path, theme, raw) {
    return `${theme}|${raw ? 1 : 0}|${path}`;
  }

  get(path, theme, raw) {
    const k = RenderCache.key(path, theme, raw);
    const hit = this.entries.get(k);
    if (!hit) return null;
    this.entries.delete(k);
    this.entries.set(k, hit);
    return { html: hit.html, stamp: hit.stamp };
  }

  set(path, theme, raw, html, stamp) {
    const k = RenderCache.key(path, theme, raw);
    const existing = this.entries.get(k);
    if (existing) {
      this.entries.delete(k);
      this.chars -= existing.html.length;
    }
    // A document that can never coexist with anything else isn't worth
    // emptying the cache for.
    if (html.length > this.maxChars) return;
    this.entries.set(k, { path, html, stamp });
    this.chars += html.length;
    this.evict();
  }

  deleteByPath(path) {
    for (const [k, entry] of this.entries) {
      if (entry.path === path) {
        this.entries.delete(k);
        this.chars -= entry.html.length;
      }
    }
  }

  clear() {
    this.entries.clear();
    this.chars = 0;
  }

  evict() {
    for (const [k, entry] of this.entries) {
      if (this.chars <= this.maxChars) return;
      this.entries.delete(k);
      this.chars -= entry.html.length;
    }
  }
}
