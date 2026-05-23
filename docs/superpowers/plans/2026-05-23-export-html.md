# Export as HTML — Implementation Plan (Plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "File ▸ Export as HTML…" action that writes the active document as a single, self-contained, always-light `.html` file (math, Mermaid, code, tables, and local images all inlined).

**Architecture:** A new pure-helper module `ui/export.js` (no DOM/Tauri imports, unit-tested under `node --test`, mirroring `ui/search.js`) holds filename derivation, CSS light-forcing, font/url inlining, and HTML-document assembly. The impure orchestrator lives in `ui/app.js`: it snapshots view state, re-renders the active tab in **light** theme through the existing `renderActive`/`postRender` pipeline, serializes the rendered `#preview` subtree (stripping injected buttons, inlining images and CSS), writes it via the existing `save_export` Rust command, then restores the prior state. A new File-menu item in `src-tauri/src/menu.rs` emits an `export` Tauri event that the frontend listens for.

**Tech Stack:** Vanilla ES modules (no build step for JS), Tauri 2 (Rust), `node --test` for JS unit tests, `comrak`/`syntect` (already render the HTML), vendored `github-markdown.css` + `katex`.

**Scope note:** This is Plan 1 (HTML) of the two-plan split from the design spec `docs/superpowers/specs/2026-05-23-export-document-design.md`. PDF is Plan 2 and is **out of scope here** — but the orchestrator and menu wiring are written so Plan 2 only adds a branch, not a rewrite.

---

## Reference facts (verified against the codebase)

- `ui/app.js` top: `import { findMatches } from "./search.js";` — add a sibling import from `./export.js`.
- Globals already present: `preview` (`#preview` article), `previewScroll`, `currentTheme`, `activeTab()`, `initMermaid()`, `renderActive({ scrollLock, forceMermaid })`, `showError(...)`, `blobToBase64(blob)` (returns **raw base64, no `data:` prefix**), `dialogApi.save(...)`, `invoke(...)`.
- `renderActive` reads the **global** `currentTheme` and the active tab's `.raw` — so forcing light = set `currentTheme = "light"`, `t.raw = false`, `initMermaid()`, then `await renderActive(...)`.
- `resolveImages` sets local image `src` to a macOS `asset://localhost/...` URL (NOT `http(s):`), so "inline everything that isn't `http(s):`/`data:`" correctly targets local images and leaves remote alone.
- `save_export(path, data, base64_encoded)` already exists in `src-tauri/src/commands.rs` and is registered. HTML uses `base64Encoded: false`.
- `ui/github-markdown.css` defines light color variables inside `@media (prefers-color-scheme: light)` and dark inside `@media (prefers-color-scheme: dark)`.
- `ui/katex/katex.min.css` references `url(fonts/KaTeX_*.woff2)` (20 fonts) plus `.woff`/`.ttf` fallbacks; only `.woff2` files exist on disk under `ui/katex/fonts/`. Inline only the `.woff2` refs.
- JS tests run with `node --test ui/*.test.js` (see `.github/workflows/ci.yml:46`). Run a single file with `node --test ui/export.test.js`.
- Commit style: imperative subject, **no** `feat:` prefix, **no** `Co-Authored-By` trailer (matches repo history and global CLAUDE.md).
- Frontend changes are bundled at compile time, so manual verification requires `cd src-tauri && cargo build` (or `cargo run`) before launching.

---

## Task 1: Create `ui/export.js` with filename helpers

**Files:**
- Create: `ui/export.js`
- Test: `ui/export.test.js`

- [ ] **Step 1: Write the failing test**

Create `ui/export.test.js`:

```js
import { test } from "node:test";
import assert from "node:assert/strict";
import { baseName, exportFilename } from "./export.js";

test("baseName returns the final path segment", () => {
  assert.equal(baseName("/a/b/README.md"), "README.md");
  assert.equal(baseName("README.md"), "README.md");
  assert.equal(baseName("/a/b/"), "/a/b/"); // trailing slash → fall back to input
});

test("exportFilename replaces the last extension", () => {
  assert.equal(exportFilename("/a/b/README.md", "html"), "README.html");
  assert.equal(exportFilename("/a/notes.tar.gz", "pdf"), "notes.tar.pdf");
});

test("exportFilename appends when there is no extension", () => {
  assert.equal(exportFilename("/a/Makefile", "html"), "Makefile.html");
});

test("exportFilename keeps a leading-dot name intact", () => {
  assert.equal(exportFilename("/a/.env", "html"), ".env.html");
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test ui/export.test.js`
Expected: FAIL — `Cannot find module './export.js'` (or "export ... not found").

- [ ] **Step 3: Write the minimal implementation**

Create `ui/export.js`:

```js
// Pure helpers for document export. No DOM or Tauri imports, so this runs under
// `node --test` as well as in the WebView (mirrors search.js).

/** Final path segment of a Unix or Windows path. Falls back to the whole
 *  input when the path ends in a separator (no usable basename). */
export function baseName(path) {
  const parts = String(path).split(/[\\/]/);
  const last = parts[parts.length - 1];
  return last || String(path);
}

/** `srcPath`'s basename with its extension replaced by `ext` (or `ext`
 *  appended when there is no extension). A leading-dot name (".env") is treated
 *  as having no extension. */
export function exportFilename(srcPath, ext) {
  const name = baseName(srcPath);
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  return `${stem}.${ext}`;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test ui/export.test.js`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add ui/export.js ui/export.test.js
git commit -m "Add export filename helpers"
```

---

## Task 2: `documentNeedsKatex` feature detection

**Files:**
- Modify: `ui/export.js`
- Test: `ui/export.test.js`

- [ ] **Step 1: Write the failing test**

Append to `ui/export.test.js`:

```js
import { documentNeedsKatex } from "./export.js";

test("documentNeedsKatex detects rendered KaTeX markup", () => {
  assert.equal(documentNeedsKatex('<span class="katex">x</span>'), true);
  assert.equal(
    documentNeedsKatex('<span class="katex-display"><span class="katex">y</span></span>'),
    true,
  );
});

test("documentNeedsKatex is false without math", () => {
  assert.equal(documentNeedsKatex("<p>no math here</p>"), false);
  assert.equal(documentNeedsKatex(""), false);
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test ui/export.test.js`
Expected: FAIL — `documentNeedsKatex is not a function` / not exported.

- [ ] **Step 3: Write the minimal implementation**

Append to `ui/export.js`:

```js
/** True if the rendered HTML contains KaTeX output, meaning the export must
 *  embed the KaTeX stylesheet + fonts. KaTeX wraps every formula in an element
 *  with class "katex" (display math adds a "katex-display" wrapper). */
export function documentNeedsKatex(html) {
  return String(html).includes('class="katex');
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test ui/export.test.js`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/export.js ui/export.test.js
git commit -m "Add KaTeX feature detection for export"
```

---

## Task 3: `inlineFontUrls` — rewrite font references to data URLs

**Files:**
- Modify: `ui/export.js`
- Test: `ui/export.test.js`

- [ ] **Step 1: Write the failing test**

Append to `ui/export.test.js`:

```js
import { inlineFontUrls } from "./export.js";

test("inlineFontUrls replaces mapped url() references", () => {
  const css =
    "@font-face{src:url(fonts/A.woff2) format('woff2'),url(fonts/A.woff) format('woff')}";
  const out = inlineFontUrls(css, { "fonts/A.woff2": "data:font/woff2;base64,XX" });
  assert.ok(out.includes("url(data:font/woff2;base64,XX)"));
  assert.ok(out.includes("url(fonts/A.woff)")); // unmapped refs untouched
});

test("inlineFontUrls handles multiple distinct fonts", () => {
  const css = "url(fonts/A.woff2) url(fonts/B.woff2)";
  const out = inlineFontUrls(css, {
    "fonts/A.woff2": "data:font/woff2;base64,AA",
    "fonts/B.woff2": "data:font/woff2;base64,BB",
  });
  assert.equal(out, "url(data:font/woff2;base64,AA) url(data:font/woff2;base64,BB)");
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test ui/export.test.js`
Expected: FAIL — `inlineFontUrls is not a function`.

- [ ] **Step 3: Write the minimal implementation**

Append to `ui/export.js`:

```js
/** Replace `url(<ref>)` occurrences with `url(<dataUrl>)` for each entry in
 *  `fontMap` (keyed by the exact ref text as it appears in the CSS). Plain
 *  string replacement avoids regex-escaping the path. */
export function inlineFontUrls(cssText, fontMap) {
  let out = cssText;
  for (const [ref, dataUrl] of Object.entries(fontMap)) {
    out = out.split(`url(${ref})`).join(`url(${dataUrl})`);
  }
  return out;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test ui/export.test.js`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/export.js ui/export.test.js
git commit -m "Add font-url inlining for export"
```

---

## Task 4: `forceLightCss` — make light styles unconditional

**Files:**
- Modify: `ui/export.js`
- Test: `ui/export.test.js`

**Why:** `github-markdown.css` gates light color variables behind `@media (prefers-color-scheme: light)` and dark behind `@media (prefers-color-scheme: dark)`. An exported file must look light on **any** viewer OS, so we remove the dark block and unwrap the light block (its rules apply unconditionally).

- [ ] **Step 1: Write the failing test**

Append to `ui/export.test.js`:

```js
import { forceLightCss } from "./export.js";

test("forceLightCss removes dark blocks and unwraps light blocks", () => {
  const css = [
    ".a{color:red}",
    "@media (prefers-color-scheme: dark){ .a{color:white} }",
    "@media (prefers-color-scheme: light){ .a{--x: black} }",
    ".b{margin:0}",
  ].join("\n");
  const out = forceLightCss(css);
  assert.ok(!out.includes("color:white"), "dark rules removed");
  assert.ok(out.includes("--x: black"), "light rules kept");
  assert.ok(!out.includes("prefers-color-scheme"), "no media wrappers remain");
  assert.ok(out.includes(".a{color:red}"));
  assert.ok(out.includes(".b{margin:0}"));
});

test("forceLightCss ignores braces inside strings and comments", () => {
  const css =
    "@media (prefers-color-scheme: dark){ .a::before{content:'{'} /* } */ .b{x:1} }\n.c{y:2}";
  const out = forceLightCss(css);
  assert.ok(!out.includes("x:1"), "whole dark block removed despite stray braces");
  assert.ok(out.includes(".c{y:2}"), "content after the block survives");
});

test("forceLightCss tolerates whitespace variations", () => {
  const css = "@media(prefers-color-scheme:light){.a{color:green}}";
  const out = forceLightCss(css);
  assert.ok(out.includes(".a{color:green}"));
  assert.ok(!out.includes("prefers-color-scheme"));
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test ui/export.test.js`
Expected: FAIL — `forceLightCss is not a function`.

- [ ] **Step 3: Write the minimal implementation**

Append to `ui/export.js`:

```js
/** Advance past a quoted string starting at `i` (the opening quote). Returns
 *  the index of the closing quote (or end of input). Handles backslash escapes. */
function skipString(s, i) {
  const quote = s[i];
  i++;
  while (i < s.length) {
    if (s[i] === "\\") {
      i += 2;
      continue;
    }
    if (s[i] === quote) return i;
    i++;
  }
  return i;
}

/** Advance past a `/* ... *\/` comment starting at `i`. Returns the index of
 *  the closing slash (or end of input). */
function skipComment(s, i) {
  i += 2;
  while (i < s.length && !(s[i] === "*" && s[i + 1] === "/")) i++;
  return i + 1;
}

/** Index of the `}` matching the `{` at `openBraceIdx`, ignoring braces that
 *  appear inside CSS strings or comments. -1 if unbalanced. */
function matchingBraceEnd(s, openBraceIdx) {
  let depth = 0;
  for (let i = openBraceIdx; i < s.length; i++) {
    const c = s[i];
    if (c === '"' || c === "'") {
      i = skipString(s, i);
      continue;
    }
    if (c === "/" && s[i + 1] === "*") {
      i = skipComment(s, i);
      continue;
    }
    if (c === "{") depth++;
    else if (c === "}") {
      depth--;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** For each `@media (...) {` matched by `headerRe` (which must be global and end
 *  at the `{`), either drop the whole block (keepInner=false) or splice in its
 *  inner rules without the wrapper (keepInner=true). */
function transformMediaBlocks(css, headerRe, keepInner) {
  let result = "";
  let pos = 0;
  for (const m of css.matchAll(headerRe)) {
    const headerStart = m.index;
    if (headerStart < pos) continue; // already inside a consumed block
    const braceIdx = headerStart + m[0].length - 1;
    const end = matchingBraceEnd(css, braceIdx);
    if (end === -1) break;
    result += css.slice(pos, headerStart);
    if (keepInner) result += css.slice(braceIdx + 1, end);
    pos = end + 1;
  }
  result += css.slice(pos);
  return result;
}

/** Force a prefers-color-scheme stylesheet to its light variant: remove dark
 *  media blocks entirely, unwrap light media blocks so their rules always
 *  apply. */
export function forceLightCss(cssText) {
  const dark = /@media\s*\(\s*prefers-color-scheme\s*:\s*dark\s*\)\s*\{/gi;
  const light = /@media\s*\(\s*prefers-color-scheme\s*:\s*light\s*\)\s*\{/gi;
  let out = transformMediaBlocks(cssText, dark, false);
  out = transformMediaBlocks(out, light, true);
  return out;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test ui/export.test.js`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add ui/export.js ui/export.test.js
git commit -m "Force exported CSS to its light variant"
```

---

## Task 5: `buildHtmlDocument` — assemble the standalone file

**Files:**
- Modify: `ui/export.js`
- Test: `ui/export.test.js`

- [ ] **Step 1: Write the failing test**

Append to `ui/export.test.js`:

```js
import { buildHtmlDocument } from "./export.js";

test("buildHtmlDocument wraps body and inlines css", () => {
  const doc = buildHtmlDocument({ title: "T", css: ".a{}", bodyHtml: "<p>x</p>" });
  assert.ok(doc.startsWith("<!doctype html>"));
  assert.ok(doc.includes("<title>T</title>"));
  assert.ok(doc.includes('name="color-scheme" content="light"'));
  assert.ok(doc.includes("<style>.a{}</style>"));
  assert.ok(doc.includes('<article class="markdown-body">'));
  assert.ok(doc.includes("<p>x</p>"));
});

test("buildHtmlDocument escapes the title", () => {
  const doc = buildHtmlDocument({ title: "a<b>&c", css: "", bodyHtml: "" });
  assert.ok(doc.includes("<title>a&lt;b&gt;&amp;c</title>"));
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `node --test ui/export.test.js`
Expected: FAIL — `buildHtmlDocument is not a function`.

- [ ] **Step 3: Write the minimal implementation**

Append to `ui/export.js`:

```js
function escapeHtml(s) {
  return String(s).replace(
    /[&<>]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c],
  );
}

/** Assemble a complete, standalone HTML document. `css` is the already-prepared
 *  stylesheet text (light-forced, fonts inlined); `bodyHtml` is the serialized
 *  rendered content. The content is wrapped in an `article.markdown-body` so the
 *  GitHub stylesheet applies and the page CSS can center it. */
export function buildHtmlDocument({ title, css, bodyHtml }) {
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="color-scheme" content="light">
<title>${escapeHtml(title)}</title>
<style>${css}</style>
</head>
<body>
<article class="markdown-body">
${bodyHtml}
</article>
</body>
</html>
`;
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `node --test ui/export.test.js`
Expected: PASS. Also run the whole suite: `node --test ui/*.test.js` → all green (search + export).

- [ ] **Step 5: Commit**

```bash
git add ui/export.js ui/export.test.js
git commit -m "Add standalone HTML document assembly"
```

---

## Task 6: Add the File-menu item and emit the `export` event

**Files:**
- Modify: `src-tauri/src/menu.rs` (item in `rebuild`, ~line 89-95; handler in `install`, ~line 18-54)

- [ ] **Step 1: Add the menu item to the File submenu**

In `src-tauri/src/menu.rs`, inside `rebuild`, build the item just before `let file_menu = ...` (alongside the other `MenuItemBuilder` calls):

```rust
    let export_html =
        MenuItemBuilder::with_id("export-html", "Export as HTML…").build(app)?;
```

Then change the `file_menu` builder to insert it after Open Recent:

```rust
    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&open_file)
        .item(&open_folder)
        .item(&recent_submenu)
        .separator()
        .item(&export_html)
        .separator()
        .close_window()
        .build()?;
```

- [ ] **Step 2: Handle the menu event**

In `install`, add a match arm in the `on_menu_event` handler (next to `"edit-copy" => ...`):

```rust
            "export-html" => {
                let _ = app.emit("export", "html");
            }
```

- [ ] **Step 3: Build to verify it compiles, and check lint**

Run:
```bash
cd src-tauri && cargo build && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```
Expected: builds clean, fmt clean, clippy clean. (The emitted `export` event has no listener yet — that's Task 7. Clicking the item is a harmless no-op for now.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/menu.rs
git commit -m "Add Export as HTML menu item"
```

---

## Task 7: Wire the frontend export pipeline

**Files:**
- Modify: `ui/app.js` (import at line 1; new `listen` near line 178; new export section)

This task makes the feature end-to-end and is verified by running the app (no unit test — DOM + Tauri + fetch).

- [ ] **Step 1: Import the pure helpers**

At the top of `ui/app.js`, replace the first import line:

```js
import { findMatches } from "./search.js";
```
with:
```js
import { findMatches } from "./search.js";
import {
  exportFilename,
  baseName,
  documentNeedsKatex,
  inlineFontUrls,
  forceLightCss,
  buildHtmlDocument,
} from "./export.js";
```

- [ ] **Step 2: Register the `export` event listener**

In the listener-registration area (just after the `await listen("edit-action", ...)` block near line 180), add:

```js
  await listen("export", async (ev) => {
    await onExport(ev.payload);
  });
```

- [ ] **Step 3: Add the export implementation**

Add a new section near the existing Mermaid export code (e.g., just before `function addMermaidExportButtons()`). Paste the whole block:

```js
/* ---- Document export ---- */

let exportInProgress = false;

const EXPORT_PAGE_CSS = `
html { color-scheme: light; }
body { margin: 0; background: #ffffff; }
.markdown-body { box-sizing: border-box; min-width: 200px; max-width: 980px; margin: 0 auto; padding: 32px 24px; }
`;

/** Menu entry point: pick a destination, then export. */
async function onExport(format) {
  const t = activeTab();
  if (!t) {
    showError("Open a document before exporting.");
    return;
  }
  const ext = format === "pdf" ? "pdf" : "html";
  const filters =
    ext === "pdf"
      ? [{ name: "PDF document", extensions: ["pdf"] }]
      : [{ name: "HTML document", extensions: ["html"] }];
  const path = await dialogApi.save({
    defaultPath: exportFilename(t.path, ext),
    filters,
  });
  if (!path) return;
  await exportDocument(format, path);
}

/** Snapshot view state, force a light rendered view, run the format-specific
 *  export, then restore. The light re-render reuses the real renderActive
 *  pipeline so math/Mermaid/code come out light and faithful. */
async function exportDocument(format, path) {
  if (exportInProgress) return;
  const t = activeTab();
  if (!t) return;
  exportInProgress = true;
  const prevTheme = currentTheme;
  const prevRaw = t.raw;
  const prevScroll = previewScroll.scrollTop;
  try {
    currentTheme = "light";
    t.raw = false;
    initMermaid();
    await renderActive({ scrollLock: false, forceMermaid: true });

    if (format === "html") {
      await exportHtml(path);
    }
    // PDF is Plan 2.
  } catch (e) {
    console.error("export failed", e);
    showError("Export failed: " + e);
  } finally {
    currentTheme = prevTheme;
    t.raw = prevRaw;
    initMermaid();
    await renderActive({ scrollLock: false, forceMermaid: true });
    previewScroll.scrollTop = prevScroll;
    exportInProgress = false;
  }
}

/** Serialize the (already light-rendered) preview into one standalone HTML file
 *  and write it via the save_export command. */
async function exportHtml(path) {
  const clone = preview.cloneNode(true);
  // Drop UI chrome injected after render (copy buttons, mermaid export buttons).
  clone
    .querySelectorAll(".export-btn-group, .copy-btn")
    .forEach((el) => el.remove());
  await inlineImages(clone);
  const bodyHtml = clone.innerHTML;

  let css = forceLightCss(await fetchText("github-markdown.css"));
  if (documentNeedsKatex(bodyHtml)) {
    let katexCss = await fetchText("katex/katex.min.css");
    katexCss = inlineFontUrls(katexCss, await buildKatexFontMap(katexCss));
    css += "\n" + katexCss;
  }
  css += "\n" + EXPORT_PAGE_CSS;

  const html = buildHtmlDocument({
    title: baseName(activeTab().path),
    css,
    bodyHtml,
  });
  await invoke("save_export", { path, data: html, base64Encoded: false });
}

async function fetchText(url) {
  const res = await fetch(url);
  return await res.text();
}

/** Replace local (asset:// or relative) <img> sources with data: URLs so the
 *  exported file is standalone. Remote (http/https) and existing data: srcs are
 *  left as-is. Per-image failures are logged and skipped (the original src
 *  stays, still valid online). macOS asset URLs use the asset:// scheme, so the
 *  http(s) check below correctly leaves only true remote images alone. */
async function inlineImages(root) {
  const imgs = [...root.querySelectorAll("img")];
  await Promise.all(
    imgs.map(async (img) => {
      const src = img.getAttribute("src") || "";
      if (!src || src.startsWith("data:") || /^https?:/i.test(src)) return;
      try {
        const blob = await (await fetch(src)).blob();
        const mime = blob.type || "image/png";
        img.setAttribute(
          "src",
          `data:${mime};base64,` + (await blobToBase64(blob)),
        );
      } catch (e) {
        console.warn("image inline failed:", src, e);
      }
    }),
  );
}

/** Build { "fonts/X.woff2": "data:font/woff2;base64,…" } for every woff2 the
 *  KaTeX CSS references. Only woff2 exists on disk; woff/ttf fallbacks are left
 *  untouched (browsers prefer the inlined woff2 via its format() hint). */
async function buildKatexFontMap(katexCss) {
  const refs = [
    ...new Set(
      [...katexCss.matchAll(/url\((fonts\/[^)]+\.woff2)\)/g)].map((m) => m[1]),
    ),
  ];
  const map = {};
  await Promise.all(
    refs.map(async (ref) => {
      const blob = await (await fetch("katex/" + ref)).blob();
      map[ref] = "data:font/woff2;base64," + (await blobToBase64(blob));
    }),
  );
  return map;
}
```

- [ ] **Step 4: Build the app**

Run:
```bash
cd src-tauri && cargo build
```
Expected: builds clean (frontend is bundled into the binary at compile time).

- [ ] **Step 5: Create a test document**

Run this (note: the heredoc body avoids nested triple backticks so the shell copy is clean):

```bash
printf '%s\n' \
'# Export check' '' \
'Some **bold** text and `inline code`.' '' \
'| A | B |' '|---|---|' '| 1 | 2 |' '' \
'Inline math $a^2 + b^2 = c^2$ and display:' '' \
'$$E = mc^2$$' '' \
'~~~rust' 'fn main() { println!("hi"); }' '~~~' '' \
'~~~mermaid' 'graph TD; A-->B; B-->C;' '~~~' \
> /tmp/export-check.md
# Then change the ~~~ fences to triple backticks, or just author the file by hand
# with ```rust and ```mermaid fences — comrak only treats backtick/tilde fences.
```

(Tilde `~~~` fences are valid GFM and render identically, so the file works as-is; use backticks if you prefer.)

- [ ] **Step 6: Manual verification**

Run: `cd src-tauri && cargo run -- /tmp/export-check.md`

Then in the app:
1. The document loads as the initial file.
2. Menu **File ▸ Export as HTML…**, save to `/tmp/export-check.html`.
3. The preview should briefly flash to light and then restore to your OS theme.

Verify the output — put your Mac in **dark mode** first, then open the file:
```bash
open /tmp/export-check.html
```
Confirm:
- The page is **light** (white background, dark text) even though the OS is dark — proves `forceLightCss`.
- The table, bold, inline code, and the Rust code block (with syntax-color highlighting) all render.
- Both math expressions render as formulas (not raw `$…$`) — proves KaTeX CSS + font inlining.
- The Mermaid diagram renders as a diagram (an inline `<svg>`).
- The file is self-contained: `grep -c "data:font/woff2" /tmp/export-check.html` is ≥ 1, and there are no `<link rel="stylesheet">` tags (`grep -c "<link" /tmp/export-check.html` is 0).

If something fails, `cargo run` is a debug build — use the WebView console (right-click ▸ Inspect Element) and read `console.warn`/`console.error`.

- [ ] **Step 7: Commit**

```bash
git add ui/app.js
git commit -m "Wire HTML export pipeline in the frontend"
```

---

## Task 8: Document the feature and run the full verification

**Files:**
- Modify: `CLAUDE.md` (Architecture quick-tour + the "Things that took hours" section)

- [ ] **Step 1: Document the export pipeline**

In `CLAUDE.md`, under "Architecture quick-tour", add a bullet after the Math/KaTeX bullet (before `postRender()`):

```markdown
- **Export (HTML)**: `ui/export.js` holds pure helpers (filename derivation,
  `forceLightCss`, `inlineFontUrls`, `buildHtmlDocument`, `documentNeedsKatex`),
  unit-tested under `node --test`. The frontend's `exportDocument()` snapshots
  view state, forces a **light** render through `renderActive` (so code/math/
  Mermaid are light and theme-stable), serializes `#preview` (stripping injected
  buttons, inlining local images and the github-markdown/KaTeX CSS + woff2 fonts
  as `data:` URLs), and writes via the existing `save_export` command. Triggered
  by **File ▸ Export as HTML…** (`menu.rs` emits an `export` event with the
  format). PDF is a planned second format (see `docs/superpowers/specs/`).
```

Also add to the "Things that took hours and shouldn't again" section:

```markdown
- **Export must force light CSS**: `github-markdown.css` gates its light color
  variables behind `@media (prefers-color-scheme: light)`. Simply deleting the
  dark block leaves a dark-OS viewer with *no* variables (broken colors). The
  export's `forceLightCss` both removes the dark block AND unwraps the light
  block so its rules apply unconditionally. KaTeX fonts must be inlined as
  `data:` URLs too, or the `.html` references font files that don't travel with
  it.
```

- [ ] **Step 2: Run the complete verification suite**

Run all of:
```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build
```
Expected: JS tests pass; fmt/clippy clean; Rust tests pass; build succeeds.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "Document HTML export pipeline"
```

---

## Self-review (completed during planning)

- **Spec coverage (HTML slice of the spec):** light re-render orchestrator (Task 7) ✓; HTML serialize + skeleton (`buildHtmlDocument`, Task 5; `exportHtml`, Task 7) ✓; conditional KaTeX CSS + font inlining (`documentNeedsKatex` Task 2, `inlineFontUrls` Task 3, `buildKatexFontMap` Task 7) ✓; image data-URLs (`inlineImages`, Task 7) ✓; Mermaid SVG carried verbatim (clone in Task 7) ✓; reuse `save_export` ✓; File-menu item + `export` event (Task 6) + listener (Task 7) ✓; pure-helper unit tests (Tasks 1-5) ✓; CLAUDE.md docs (Task 8) ✓. The "always light" requirement is covered by BOTH the light re-render AND `forceLightCss` (the latter defends against the *viewer's* OS theme). `documentNeedsMermaid` from the spec is intentionally **not** implemented: a rendered Mermaid SVG is self-contained (its styles live inside the SVG), so HTML export needs nothing Mermaid-specific to inline.
- **Placeholder scan:** none — every code step contains complete code; the only "Plan 2" reference is a comment marking the deliberate PDF seam, not missing work.
- **Type/name consistency:** helper names match between `export.js` definitions, the `app.js` import list, and call sites (`exportFilename`, `baseName`, `documentNeedsKatex`, `inlineFontUrls`, `forceLightCss`, `buildHtmlDocument`); impure helpers (`exportDocument`, `exportHtml`, `onExport`, `fetchText`, `inlineImages`, `buildKatexFontMap`, `EXPORT_PAGE_CSS`) are all defined in Task 7; reused existing globals (`blobToBase64`, `renderActive`, `initMermaid`, `currentTheme`, `previewScroll`, `activeTab`, `showError`, `dialogApi`, `invoke`) verified present in `app.js`.
