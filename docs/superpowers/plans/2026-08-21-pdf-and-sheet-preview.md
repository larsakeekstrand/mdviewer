# PDF and Spreadsheet Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Render PDF files in the webview's native viewer and Excel workbooks as HTML data tables, behind a render-dispatch seam that makes each new view type a table row rather than a fourteen-site edit.

**Architecture:** `ui/filetype.js` gains `viewKind(path)` (five kinds) plus eight named capability predicates derived from a single table; `app.js`'s fourteen type checks migrate onto those predicates and `isCodeView` is deleted. PDF is frontend-only — `renderActive` short-circuits to `renderPdf` before the `render_file` IPC and frames the file via `convertFileSrc`. Excel is a new Rust module hooked into `render_file` ahead of the `is_binary` check, emitting ordinary HTML so it inherits find, export, print, and the render cache unmodified.

**Tech Stack:** Vanilla ES modules + `node --test` (frontend); Rust 2021 + `calamine` 0.36 (workbook parsing) + `rust_xlsxwriter` 0.98 (dev-only, test fixtures); Tauri 2.11 asset protocol.

**Spec:** `docs/superpowers/specs/2026-08-21-pdf-and-sheet-preview-design.md`

## Global Constraints

- **MSRV becomes 1.88** (was 1.80), Rust edition 2021. Task 6 bumps `rust-version` in `src-tauri/Cargo.toml`. Rationale and cost are in the ledger under Ruling P3 — in short, 1.80 was declared but never tested (both workflows install `stable`; there is no MSRV job and no toolchain file), and no calamine version satisfies it transitively.
- **Lint/test gate for every task:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (all from `src-tauri/`), and `node --test ui/*.test.js` (from the repo root). CI runs clippy with `-D warnings`, so a slip blocks merge.
- **Frontend edits need `cargo build`.** Tauri bundles `frontendDist` at compile time via `tauri-codegen`; editing `ui/*` and reloading the webview shows stale UI.
- **No comments in code unless the *why* is non-obvious.**
- **Commit messages: no `Co-Authored-By: Claude` trailer.** Imperative subject; body explains *why*.
- **Do not widen the CSP.** `frame-src 'self' asset: http://asset.localhost` already permits everything this plan needs.
- **New Tauri commands return `Result<T, String>`.** This plan adds none.
- **Markdown and every file the tree can reach are untrusted input.** The xlsx parser is the new attack surface; caps are a requirement, not a hardening pass.
- **Branch:** `feat/pdf-and-sheet-preview` (already created; the spec is committed on it).

---

## File Structure

| File | Responsibility | Phase |
|---|---|---|
| `ui/filetype.js` (modify) | `viewKind` + the capability table + eight predicates. Pure, no DOM. | 1 |
| `ui/filetype.test.js` (modify) | Kind resolution, precedence, and every predicate row. | 1 |
| `ui/app.js` (modify) | Consume predicates at the fourteen gate sites; `renderPdf`; sheet class. | 1–3 |
| `ui/styles.css` (modify) | `.pdf-view` (iframe fills the pane), `.sheet-body` (table grid, theme-aware). | 2–3 |
| `src-tauri/src/xlsx.rs` (create) | `Cell`/`Sheet` types, Excel-serial date conversion, grid→HTML, caps, calamine loading. | 3 |
| `src-tauri/src/code.rs` (modify) | `escape_html` becomes `pub(crate)`. | 3 |
| `src-tauri/src/commands.rs` (modify) | `render_file` gains an xlsx branch before `is_binary`; `onExport` allowlist. | 3 |
| `src-tauri/src/lib.rs` (modify) | `mod xlsx;` | 3 |
| `src-tauri/Cargo.toml` (modify) | `calamine`; `rust_xlsxwriter` under `[dev-dependencies]`. | 3 |
| `CLAUDE.md`, `README.md` (modify) | Document the two new view types and the seam. `CHANGELOG.md` is deliberately untouched — see Task 10. | 3 |

---

# Phase 1 — The seam

## Task 1: `viewKind` and the capability table

**Files:**
- Modify: `ui/filetype.js`
- Test: `ui/filetype.test.js`

**Interfaces:**
- Consumes: nothing.
- Produces: `viewKind(path) -> "markdown" | "image" | "pdf" | "sheet" | "code"`; predicates `isEditable(kind)`, `hasSplitPreview(kind)`, `hasRawToggle(kind)`, `isAnnotatable(kind)`, `isRetainable(kind)`, `isExportable(kind)`, `rendersFromDisk(kind)`, `bustsCacheOnChange(kind)` — each takes a **kind string, not a path**, and returns `boolean`. Also `isPdfPath(path)`, `isSheetPath(path)`. `isImagePath` and `isMarkdownPath` keep their current signatures. `isCodeView` still exists after this task and is deleted in Task 2.

- [ ] **Step 1: Write the failing tests**

Append to `ui/filetype.test.js`, and add the new names to the import on line 3:

```js
import {
  isImagePath,
  isMarkdownPath,
  isCodeView,
  isPdfPath,
  isSheetPath,
  viewKind,
  isEditable,
  hasSplitPreview,
  hasRawToggle,
  isAnnotatable,
  isRetainable,
  isExportable,
  rendersFromDisk,
  bustsCacheOnChange,
} from "./filetype.js";

test("viewKind resolves each family, case-insensitively", () => {
  assert.equal(viewKind("notes.md"), "markdown");
  assert.equal(viewKind("A.MARKDOWN"), "markdown");
  assert.equal(viewKind("pic.PNG"), "image");
  assert.equal(viewKind("/docs/report.pdf"), "pdf");
  assert.equal(viewKind("C:\\books\\Manual.PDF"), "pdf");
  assert.equal(viewKind("budget.xlsx"), "sheet");
  assert.equal(viewKind("legacy.xls"), "sheet");
  assert.equal(viewKind("macro.xlsm"), "sheet");
  assert.equal(viewKind("binary.xlsb"), "sheet");
  assert.equal(viewKind("open.ods"), "sheet");
  assert.equal(viewKind("main.rs"), "code");
});

test("viewKind falls back to code for anything unrecognized", () => {
  for (const p of ["Makefile", "a.pngx", "notes.pdf.txt", "archive.tar.gz", ""]) {
    assert.equal(viewKind(p), "code", p);
  }
});

test("viewKind handles nullish input without throwing", () => {
  assert.equal(viewKind(null), "code");
  assert.equal(viewKind(undefined), "code");
});

test("isPdfPath and isSheetPath match only their own families", () => {
  assert.equal(isPdfPath("a.pdf"), true);
  assert.equal(isPdfPath("a.pdfx"), false);
  assert.equal(isPdfPath("notes.md"), false);
  assert.equal(isSheetPath("a.xlsx"), true);
  assert.equal(isSheetPath("a.xlsxx"), false);
  assert.equal(isSheetPath("a.pdf"), false);
});

test("capability table: exact rows for all five kinds", () => {
  const table = {
    //          edit  split  raw   annot retain export disk  bust
    markdown: [true, true, true, true, true, true, false, false],
    code: [true, false, false, false, true, false, false, false],
    image: [false, false, false, false, false, false, true, true],
    pdf: [false, false, false, false, false, false, true, true],
    sheet: [false, false, false, false, true, true, false, false],
  };
  const fns = [
    isEditable,
    hasSplitPreview,
    hasRawToggle,
    isAnnotatable,
    isRetainable,
    isExportable,
    rendersFromDisk,
    bustsCacheOnChange,
  ];
  for (const [kind, expected] of Object.entries(table)) {
    expected.forEach((want, i) => {
      assert.equal(fns[i](kind), want, `${fns[i].name}("${kind}")`);
    });
  }
});

test("predicates treat an unknown kind as code, not as a crash", () => {
  assert.equal(isEditable("nonsense"), true);
  assert.equal(isExportable("nonsense"), false);
  assert.equal(rendersFromDisk(undefined), false);
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `node --test ui/filetype.test.js`
Expected: FAIL — `SyntaxError: The requested module './filetype.js' does not provide an export named 'viewKind'`

- [ ] **Step 3: Implement**

Replace the body of `ui/filetype.js` below the existing `MARKDOWN_EXT` line with:

```js
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
```

Note the ordering inside `viewKind`: markdown is tested first so a hypothetical `.md` that also matched another family stays markdown, and `code` is last because it is the fallback, not a match.

- [ ] **Step 4: Run tests to verify they pass**

Run: `node --test ui/filetype.test.js`
Expected: PASS, including the four pre-existing `isImagePath`/`isMarkdownPath`/`isCodeView` tests, which must still pass unchanged.

- [ ] **Step 5: Commit**

```bash
git add ui/filetype.js ui/filetype.test.js
git commit -m "Add viewKind and capability predicates to filetype

The two existing predicates conflate four different questions across
fourteen call sites in app.js. Naming the questions separately is what
keeps a new view type from being a fourteen-site edit."
```

---

## Task 2: Migrate the gate sites and delete `isCodeView`

**Files:**
- Modify: `ui/app.js` (line numbers below are pre-edit; they shift as you go — match on the quoted code, not the number)
- Modify: `ui/filetype.js` (delete `isCodeView`)
- Test: `ui/filetype.test.js` (drop the `isCodeView` test)

**Interfaces:**
- Consumes: everything Task 1 produced.
- Produces: no new API. `app.js` no longer imports `isCodeView`; `imageVersions` is renamed `assetVersions` (a `Map<string, number>` of path → cache-bust counter), which Task 3 consumes.

**This task must not change behavior for markdown, image, or code tabs.** It *does* newly hide Edit and Raw for `.pdf`/`.xlsx` tabs, which previously offered both on files that could not honor them. That is the intended correction, not a regression.

- [ ] **Step 1: Update the import**

`ui/app.js:30` — replace:

```js
import { isImagePath, isMarkdownPath, isCodeView } from "./filetype.js";
```

with:

```js
import {
  isImagePath,
  isMarkdownPath,
  viewKind,
  isEditable,
  hasSplitPreview,
  hasRawToggle,
  isAnnotatable,
  isRetainable,
  bustsCacheOnChange,
} from "./filetype.js";
```

Import only what this task uses. `rendersFromDisk` and `isExportable` are added by Tasks 3 and 8 respectively, in the task that first consumes them — an unused import is exactly the kind of thing a reviewer should flag.

- [ ] **Step 2: Rename the cache-bust map**

Find the declaration of `imageVersions` and rename it to `assetVersions`, updating both its uses (site 286 and inside `renderImage`). It is a plain rename — no behavior change. Verify with:

```bash
grep -n "imageVersions" ui/app.js
```

Expected: no output.

- [ ] **Step 3: Migrate each gate site**

Apply these seventeen edits. Each is a substitution of the *question*, not just the predicate name.

| Site (pre-edit) | Before | After |
|---|---|---|
| 286 | `if (isImagePath(tab.path)) {` | `if (bustsCacheOnChange(viewKind(tab.path))) {` |
| 1379 | `&& !isImagePath(t.path)) {` | `&& isRetainable(viewKind(t.path))) {` |
| 1395 | `const inPlace = isCodeView(t.path);` | `const inPlace = !hasSplitPreview(viewKind(t.path));` |
| 1483 | `const image = isImagePath(t.path);` | `const kind = viewKind(t.path);` |
| 1484 | `const code = isCodeView(t.path);` | *(delete this line)* |
| 1485 | `editBtn.hidden = image;` | `editBtn.hidden = !isEditable(kind);` |
| 1486 | `if (!image) {` | `if (isEditable(kind)) {` |
| 1491 | `rawBtn.hidden = image \|\| code \|\| t.editing;` | `rawBtn.hidden = !hasRawToggle(kind) \|\| t.editing;` |
| 1492 | `if (!image && !code) {` | `if (hasRawToggle(kind)) {` |
| 1496 | `reviewBtn.hidden = image \|\| code \|\| t.editing \|\| t.raw;` | `reviewBtn.hidden = !isAnnotatable(kind) \|\| t.editing \|\| t.raw;` |
| 1596 | `if (!t \|\| isImagePath(t.path)) return;` | `if (!t \|\| !isEditable(viewKind(t.path))) return;` |
| 1618 | `const inPlace = isCodeView(t.path);` | `const inPlace = !hasSplitPreview(viewKind(t.path));` |
| 1662 | `if (isCodeView(t.path)) return; // …` | `if (!hasSplitPreview(viewKind(t.path))) return; // in-place editor has no live-preview pane` |
| 1744 | `if (!isCodeView(t.path)) await renderFromEditor(…)` | `if (hasSplitPreview(viewKind(t.path))) await renderFromEditor(…)` |
| 1804 | `if (t.editing && !isCodeView(t.path)) {` | `if (t.editing && hasSplitPreview(viewKind(t.path))) {` |
| 1843 | `if (isImagePath(t.path)) {` | `if (viewKind(t.path) === "image") {` |
| 2282 | `if (t.editing && !isCodeView(t.path)) {` | `if (t.editing && hasSplitPreview(viewKind(t.path))) {` |

Site 1843 becomes a `switch` in Task 3; a direct kind comparison here keeps this task behavior-preserving.

- [ ] **Step 4: Migrate `paintHtml`'s class selection (site ~1895)**

Replace:

```js
  const code = isCodeView(t.path);
  preview.classList.toggle("raw-body", raw && !code);
```

with:

```js
  const kind = viewKind(t.path);
  preview.classList.toggle("raw-body", raw && kind === "markdown");
```

and replace the `incoming.className` assignment:

```js
  incoming.className = code
    ? "code-body"
    : "markdown-body" + (raw ? " raw-body" : "");
```

with:

```js
  incoming.className =
    kind === "code"
      ? "code-body"
      : kind === "sheet"
        ? "sheet-body"
        : "markdown-body" + (raw ? " raw-body" : "");
```

`sheet-body` has no styling until Task 7 and nothing renders into it until then; wiring it here keeps the class decision in one place.

- [ ] **Step 5: Migrate `runEditAction` (sites 3453 and 3459)**

Replace the two guard blocks with one kind-aware guard:

```js
async function runEditAction(name) {
  const t = activeTab();
  const kind = t ? viewKind(t.path) : "code";
  const labels = { image: "images", pdf: "PDFs", sheet: "spreadsheets" };
  if (
    t &&
    !isEditable(kind) &&
    (name === "copy-source" || name === "toggle-raw" || name === "toggle-edit")
  ) {
    showTransientError(`Not available for ${labels[kind] || "this file type"}.`);
    return;
  }
  if (t && !hasRawToggle(kind) && name === "toggle-raw") {
    showTransientError("Raw view isn't available for this file type.");
    return;
  }
```

The first guard must stay first: for `image`/`pdf`/`sheet` both conditions match, and the more specific message should win.

- [ ] **Step 6: Delete `isCodeView`**

Remove the function and its doc comment from `ui/filetype.js`, and remove the `isCodeView` import and the `"isCodeView is true only for non-markdown, non-image"` test from `ui/filetype.test.js`.

- [ ] **Step 7: Verify no stragglers**

```bash
grep -rn "isCodeView" ui/ && echo "STRAGGLERS FOUND" || echo "clean"
```

Expected: `clean`

- [ ] **Step 8: Run the full gate**

```bash
node --test ui/*.test.js
cd src-tauri && cargo build && cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

Expected: all pass. `cargo build` is required — Tauri bundles `ui/` at compile time.

- [ ] **Step 9: Manual smoke test**

Run `cd src-tauri && cargo run -- ../README.md` and confirm, on the four kinds that already worked:

- A markdown tab: Edit, Raw, and Review buttons all present; Raw toggles; Review enters.
- A code tab (`src-tauri/src/lib.rs`): Edit present and edits **in place** (no split preview pane); Raw and Review absent.
- An image tab: Edit, Raw, Review all absent; the image renders.
- Switch away from a markdown tab and back: it repaints instantly (retained DOM still eligible).

Then confirm the intended correction: open a `.pdf` — Edit and Raw are now absent where they used to appear. It still shows "Can't preview this file type"; that is Task 3's job.

- [ ] **Step 10: Commit**

```bash
git add ui/app.js ui/filetype.js ui/filetype.test.js
git commit -m "Migrate render-dispatch gates onto capability predicates

Each of the fourteen sites now reads as the question it actually asks
rather than as a proxy for it. Edit and Raw are correctly hidden for
PDFs and spreadsheets, which could never honor either."
```

---

# Phase 2 — PDF preview

## Task 3: Render PDFs in the native viewer

**Files:**
- Modify: `ui/app.js` (`renderActive` dispatch; new `renderPdf`; `openFind` guard)
- Modify: `ui/styles.css`

**Interfaces:**
- Consumes: `viewKind`, `rendersFromDisk`, `bustsCacheOnChange`, `assetVersions` from Tasks 1–2.
- Produces: `renderPdf(tab, { scrollLock })` — sets `preview.className = "pdf-view"`, sets `liveRender = null`, returns `void`.

- [ ] **Step 1: Add the dispatch branch**

In `renderActive`, replace the image short-circuit added in Task 2:

```js
  if (viewKind(t.path) === "image") {
    renderImage(t, { scrollLock });
    return;
  }
```

with:

```js
  const kind = viewKind(t.path);
  if (rendersFromDisk(kind)) {
    if (kind === "image") renderImage(t, { scrollLock });
    else renderPdf(t);
    return;
  }
```

This must stay **above** the `render_file` invoke: the backend does `std::fs::read`, and a PDF would hit `code::is_binary` and come back as `unsupported_html()`.

- [ ] **Step 2: Implement `renderPdf`**

Add directly below `renderImage` so the two read side by side:

```js
/** Frame a PDF for the webview's own viewer. Unlike renderImage there is no
 *  same-file scroll preservation: the iframe owns its scroll, zoom, and page,
 *  and we cannot read them back across the boundary. */
function renderPdf(t) {
  previewEmpty.hidden = true;
  preview.hidden = false;
  if (findOpen()) closeFind();

  preview.className = "pdf-view";

  const v = assetVersions.get(t.path) || 0;
  const frame = document.createElement("iframe");
  frame.title = basename(t.path);
  frame.src = convertFileSrc(t.path) + (v ? `?v=${v}` : "");

  preview.replaceChildren(frame);
  previewScroll.scrollTop = 0;
  previewScroll.scrollLeft = 0;
  liveRender = null;
}
```

`liveRender = null` is what keeps PDF tabs out of the retained-DOM cache — required, because a stashed iframe would reload on reattach and lose the reader's page anyway.

- [ ] **Step 3: Guard find-in-page**

`openFind` currently opens for any tab. A PDF's text lives inside the iframe, which `collectFindSegments`' TreeWalker cannot enter, so find would open and truthfully report zero matches on a document full of text. Replace the first line of `openFind`:

```js
function openFind() {
  if (!activeTab()) return;
```

with:

```js
function openFind() {
  const t = activeTab();
  if (!t) return;
  if (viewKind(t.path) === "pdf") {
    showTransientError("Use the PDF viewer's own search for PDF files.");
    return;
  }
```

- [ ] **Step 4: Style the frame**

Append to `ui/styles.css`, near the existing `.image-view` rules:

```css
.pdf-view {
  display: flex;
  height: 100%;
  padding: 0;
}

.pdf-view iframe {
  flex: 1;
  width: 100%;
  height: 100%;
  border: 0;
}
```

`.pdf-view` must not inherit `.markdown-body`'s prose max-width, which is why `renderPdf` replaces `preview.className` outright rather than adding a class.

- [ ] **Step 5: Build and smoke test**

```bash
cd src-tauri && cargo build && cargo run -- ../README.md
```

Confirm, with any PDF in the tree (generate one via **File ▸ Export as PDF…** if you need one):

1. Clicking a `.pdf` renders the document in the webview's viewer, filling the pane.
2. Its own toolbar (page nav, zoom) works.
3. Edit, Raw, and Review buttons are absent.
4. ⌘F shows "Use the PDF viewer's own search for PDF files." instead of an empty find bar.
5. Switch to a markdown tab and back: the PDF reloads (expected — not retained) and the markdown tab still repaints instantly.
6. Overwrite the PDF on disk (re-export over it). The tab reloads with the new content rather than showing a cached copy.

Point 6 is the `assetVersions` cache-bust; if it shows stale content, verify `bustsCacheOnChange(viewKind(path))` is being consulted at the `file-changed` listener.

- [ ] **Step 6: Run the full gate**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- [ ] **Step 7: Commit**

```bash
git add ui/app.js ui/styles.css
git commit -m "Render PDF files in the webview's native viewer

Frontend-only: the short-circuit sits above the render_file IPC because
the backend reads to a string and would reject a PDF as binary. The tab
is deliberately not retained — an iframe reloads on reattach regardless."
```

---

## Task 4: Verify PDF framing on Windows

**Files:**
- Modify: `docs/superpowers/specs/2026-08-21-pdf-and-sheet-preview-design.md` (record the result)
- Modify: `ui/app.js` (only if the fallback is needed)

**Interfaces:**
- Consumes: `renderPdf` from Task 3.
- Produces: either a confirmation note, or a fallback panel in `renderPdf`.

This is the plan's one unverified platform assumption. macOS WKWebView frames `asset://` PDFs — proven by `ui/pdf-export.html`'s `exact-preview`. WebView2 ships Edge's PDF viewer and is expected to match, but that has not been observed in this app.

- [ ] **Step 1: Build and run on Windows**

On a Windows x86_64 machine (or the `windows-latest` CI runner via a scratch branch):

```bash
cd src-tauri
cargo build
cargo run -- ..\README.md
```

Open a `.pdf` from the tree.

- [ ] **Step 2: Record the outcome**

If it renders: add one line under the spec's "Cross-platform" heading — `Verified on Windows/WebView2 <date>.` — and skip Step 3.

If the frame is blank or blocked: implement the fallback in Step 3.

- [ ] **Step 3 (only if Step 2 failed): Fallback panel**

Replace the `preview.replaceChildren(frame)` line in `renderPdf` with a load-failure fallback:

```js
  frame.onerror = () => showPdfFallback(t);
  preview.replaceChildren(frame);
```

and add:

```js
/** WebView2 declined to frame the document: offer the OS handler instead.
 *  `pdf` is deliberately absent from UNSAFE_OPEN_EXTS — do not add it. */
function showPdfFallback(t) {
  const panel = document.createElement("div");
  panel.className = "image-error";
  const btn = document.createElement("button");
  btn.textContent = "Open in default application";
  btn.addEventListener("click", () => {
    invoke("open_path", { path: t.path }).catch((e) =>
      showTransientError(String(e)),
    );
  });
  panel.append("This PDF can't be previewed here. ", btn);
  preview.replaceChildren(panel);
}
```

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "Verify PDF framing on Windows/WebView2"
```

---

# Phase 3 — Excel preview

## Task 5: Pure spreadsheet types, date conversion, and HTML emission

**Files:**
- Create: `src-tauri/src/xlsx.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod xlsx;`)
- Modify: `src-tauri/src/code.rs` (`escape_html` → `pub(crate)`)

**Interfaces:**
- Consumes: `code::escape_html`.
- Produces:
  - `pub enum Cell { Empty, Text(String), Number(f64), Bool(bool), DateTime(f64), Error(String) }`
  - `pub struct Sheet { pub name: String, pub rows: Vec<Vec<Cell>> }`
  - `pub struct Caps { pub max_file_bytes: usize, pub max_rows_per_sheet: usize, pub max_total_cells: usize }` with `Default`
  - `pub fn is_spreadsheet_path(p: &Path) -> bool`
  - `pub fn excel_serial_to_iso(serial: f64) -> String`
  - `pub fn cell_to_string(c: &Cell) -> String`
  - `pub fn render_sheets(sheets: &[Sheet], caps: &Caps) -> String`

No `calamine` in this task — everything here is pure and tested against hand-built grids. Task 6 adds the parser behind this boundary, so a `calamine` API difference touches one function rather than the renderer.

- [ ] **Step 1: Make `escape_html` reusable**

`src-tauri/src/code.rs:14` — change `fn escape_html(` to `pub(crate) fn escape_html(`.

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/xlsx.rs` containing only the test module plus the type declarations it needs (implementations come in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognizes_spreadsheet_extensions_case_insensitively() {
        for p in ["a.xlsx", "a.XLSM", "a.xlsb", "a.xls", "a.ods"] {
            assert!(is_spreadsheet_path(Path::new(p)), "{p}");
        }
        for p in ["a.md", "a.pdf", "a.xlsxx", "a", "a.csv"] {
            assert!(!is_spreadsheet_path(Path::new(p)), "{p}");
        }
    }

    #[test]
    fn converts_excel_serials_to_iso_dates() {
        // Serial 1 is 1900-01-01; Excel's phantom 1900-02-29 sits at serial 60,
        // so serials at or below 59 need a one-day shift and later ones do not.
        assert_eq!(excel_serial_to_iso(1.0), "1900-01-01");
        assert_eq!(excel_serial_to_iso(59.0), "1900-02-28");
        assert_eq!(excel_serial_to_iso(61.0), "1900-03-01");
        assert_eq!(excel_serial_to_iso(25569.0), "1970-01-01");
        assert_eq!(excel_serial_to_iso(45000.0), "2023-03-15");
    }

    #[test]
    fn appends_a_time_component_only_when_the_serial_has_one() {
        assert_eq!(excel_serial_to_iso(45000.0), "2023-03-15");
        assert_eq!(excel_serial_to_iso(45000.5), "2023-03-15 12:00:00");
        assert_eq!(excel_serial_to_iso(45000.25), "2023-03-15 06:00:00");
    }

    #[test]
    fn formats_numbers_without_trailing_noise() {
        assert_eq!(cell_to_string(&Cell::Number(3.0)), "3");
        assert_eq!(cell_to_string(&Cell::Number(3.5)), "3.5");
        assert_eq!(cell_to_string(&Cell::Number(-0.25)), "-0.25");
        assert_eq!(cell_to_string(&Cell::Empty), "");
        assert_eq!(cell_to_string(&Cell::Bool(true)), "TRUE");
        assert_eq!(cell_to_string(&Cell::Text("hi".into())), "hi");
        assert_eq!(cell_to_string(&Cell::Error("#DIV/0!".into())), "#DIV/0!");
    }

    fn sheet(name: &str, rows: &[&[&str]]) -> Sheet {
        Sheet {
            name: name.to_string(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(|c| Cell::Text(c.to_string())).collect())
                .collect(),
        }
    }

    #[test]
    fn renders_first_row_as_a_header_and_the_rest_as_body() {
        let html = render_sheets(
            &[sheet("Sheet1", &[&["Item", "Qty"], &["Bolt", "120"]])],
            &Caps::default(),
        );
        assert!(html.contains("<h2>Sheet1</h2>"), "{html}");
        assert!(html.contains("<thead><tr><th>Item</th><th>Qty</th></tr></thead>"), "{html}");
        assert!(html.contains("<tbody><tr><td>Bolt</td><td>120</td></tr></tbody>"), "{html}");
    }

    #[test]
    fn renders_every_sheet_in_workbook_order() {
        let html = render_sheets(
            &[sheet("First", &[&["a"]]), sheet("Second", &[&["b"]])],
            &Caps::default(),
        );
        let first = html.find("First").unwrap();
        let second = html.find("Second").unwrap();
        assert!(first < second, "sheets out of order: {html}");
    }

    #[test]
    fn escapes_cell_text_and_sheet_names() {
        let html = render_sheets(
            &[sheet("<script>", &[&["<b>&</b>"]])],
            &Caps::default(),
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;"), "{html}");
        assert!(html.contains("&lt;b&gt;&amp;&lt;/b&gt;"), "{html}");
    }

    #[test]
    fn truncates_at_the_row_cap_and_says_so() {
        let rows: Vec<&[&str]> = (0..10).map(|_| &["x"][..]).collect();
        let caps = Caps { max_rows_per_sheet: 4, ..Caps::default() };
        let html = render_sheets(&[sheet("S", &rows)], &caps);
        assert_eq!(html.matches("<tr>").count(), 4, "{html}");
        assert!(html.contains("sheet-truncated"), "{html}");
    }

    #[test]
    fn stops_at_the_total_cell_cap_across_sheets() {
        let caps = Caps { max_total_cells: 3, ..Caps::default() };
        let html = render_sheets(
            &[
                sheet("A", &[&["1", "2"], &["3", "4"]]),
                sheet("B", &[&["5", "6"]]),
            ],
            &caps,
        );
        assert!(html.contains("sheet-truncated"), "{html}");
        assert!(!html.contains(">5<"), "budget exhausted, B should not render: {html}");
    }

    #[test]
    fn renders_an_empty_workbook_without_panicking() {
        let html = render_sheets(&[], &Caps::default());
        assert!(html.contains("sheet-empty"), "{html}");
    }

    #[test]
    fn pads_ragged_rows_to_the_widest_row() {
        let html = render_sheets(
            &[Sheet {
                name: "S".into(),
                rows: vec![
                    vec![Cell::Text("a".into())],
                    vec![Cell::Text("b".into()), Cell::Text("c".into())],
                ],
            }],
            &Caps::default(),
        );
        assert_eq!(html.matches("<th>").count(), 2, "header padded: {html}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd src-tauri && cargo test xlsx
```

Expected: FAIL to compile — `cannot find function is_spreadsheet_path in this scope` and similar.

- [ ] **Step 4: Implement**

Prepend to `src-tauri/src/xlsx.rs`, above the test module:

```rust
use crate::code::escape_html;
use std::path::Path;

const SHEET_EXTS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    DateTime(f64),
    Error(String),
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Debug, Clone)]
pub struct Caps {
    pub max_file_bytes: usize,
    pub max_rows_per_sheet: usize,
    pub max_total_cells: usize,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            max_file_bytes: 32 * 1024 * 1024,
            max_rows_per_sheet: 5_000,
            max_total_cells: 200_000,
        }
    }
}

pub fn is_spreadsheet_path(p: &Path) -> bool {
    match p.extension().and_then(|e| e.to_str()) {
        Some(ext) => SHEET_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// Excel day serial → ISO date (with a time component only when the serial
/// carries a fractional day). Excel's 1900 system contains a phantom
/// 1900-02-29 at serial 60, so serials at or below 59 sit one day ahead of the
/// real calendar and take a different epoch offset.
pub fn excel_serial_to_iso(serial: f64) -> String {
    let days = serial.trunc() as i64;
    let epoch_offset = if days <= 59 { 25_568 } else { 25_569 };
    let (y, m, d) = civil_from_days(days - epoch_offset);
    let date = format!("{y:04}-{m:02}-{d:02}");

    let frac = serial - serial.trunc();
    if frac <= f64::EPSILON {
        return date;
    }
    let total = (frac * 86_400.0).round() as i64;
    format!(
        "{date} {:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

/// Howard Hinnant's `civil_from_days`, verbatim: days since 1970-01-01 → (y, m, d).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn cell_to_string(c: &Cell) -> String {
    match c {
        Cell::Empty => String::new(),
        Cell::Text(s) => s.clone(),
        Cell::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Cell::Error(e) => e.clone(),
        Cell::DateTime(v) => excel_serial_to_iso(*v),
        // Rust's f64 Display already renders 3.0 as "3" and never uses
        // scientific notation, which is exactly the spreadsheet convention.
        Cell::Number(n) => format!("{n}"),
    }
}

fn truncation_notice(what: &str) -> String {
    format!("<p class=\"sheet-truncated\">{} truncated for preview.</p>", escape_html(what))
}

pub fn render_sheets(sheets: &[Sheet], caps: &Caps) -> String {
    if sheets.is_empty() {
        return "<p class=\"sheet-empty\">This workbook has no sheets.</p>".to_string();
    }
    let mut out = String::new();
    let mut budget = caps.max_total_cells;

    for sheet in sheets {
        out.push_str(&format!("<h2>{}</h2>", escape_html(&sheet.name)));
        if budget == 0 {
            out.push_str(&truncation_notice("Remaining sheets"));
            break;
        }
        if sheet.rows.is_empty() {
            out.push_str("<p class=\"sheet-empty\">Empty sheet.</p>");
            continue;
        }

        let width = sheet.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut truncated = sheet.rows.len() > caps.max_rows_per_sheet;
        let limit = sheet.rows.len().min(caps.max_rows_per_sheet);
        let mut tbody_open = false;

        out.push_str("<table>");
        for (i, row) in sheet.rows.iter().take(limit).enumerate() {
            if budget < width {
                truncated = true;
                break;
            }
            budget -= width;
            let tag = if i == 0 { "th" } else { "td" };
            if i == 0 {
                out.push_str("<thead>");
            } else if i == 1 {
                out.push_str("<tbody>");
                tbody_open = true;
            }
            out.push_str("<tr>");
            for col in 0..width {
                let text = row.get(col).map(cell_to_string).unwrap_or_default();
                out.push_str(&format!("<{tag}>{}</{tag}>", escape_html(&text)));
            }
            out.push_str("</tr>");
            if i == 0 {
                out.push_str("</thead>");
            }
        }
        if tbody_open {
            out.push_str("</tbody>");
        }
        out.push_str("</table>");
        if truncated {
            out.push_str(&truncation_notice("Sheet"));
        }
    }
    out
}
```

The `excel_serial_to_iso` arithmetic and all seven date assertions in Step 2 were verified against a reference implementation of the same algorithm while this plan was written — they agree exactly. If a test fails here, the transcription is wrong, not the expectations: do not "fix" the assertions, which encode Excel's documented behavior including the 1900 leap-year bug.

Add `mod xlsx;` to `src-tauri/src/lib.rs`, alphabetically after `mod watcher;`.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd src-tauri && cargo test xlsx
```

Expected: all 11 tests PASS.

- [ ] **Step 6: Run the full gate**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/xlsx.rs src-tauri/src/lib.rs src-tauri/src/code.rs
git commit -m "Add pure spreadsheet grid-to-HTML rendering

Types, Excel serial-date conversion, escaping, and cap enforcement, all
testable without a parser. Keeping calamine behind this boundary means a
version bump touches one mapping function, not the renderer."
```

---

## Task 6: Load workbooks with calamine

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/xlsx.rs`

**Interfaces:**
- Consumes: `Cell`, `Sheet`, `Caps`, `render_sheets` from Task 5.
- Produces: `pub fn render_workbook(bytes: &[u8], caps: &Caps) -> Result<String, String>`.

- [ ] **Step 1: Add the dependencies**

In `src-tauri/Cargo.toml`, under `[dependencies]` (after the `trash`/`interprocess` block):

```toml
# Spreadsheet preview: reads xlsx/xlsm/xlsb/xls/ods. Pure Rust, no native deps.
calamine = "0.36"
```

and bump the MSRV declaration at the top of the same file to match what this dependency tree actually requires:

```toml
rust-version = "1.88"
```

and add a new section before `[profile.release]`:

```toml
[dev-dependencies]
# Builds xlsx fixtures in-test so no binary blobs live in the repo.
rust_xlsxwriter = "0.98"
```

Then pin the lockfile:

```bash
cd src-tauri && cargo update -p mdviewer && cargo build
```

- [ ] **Step 2: Write the failing tests**

Add to `xlsx.rs`'s `mod tests`:

```rust
    fn fixture() -> Vec<u8> {
        use rust_xlsxwriter::Workbook;
        let mut wb = Workbook::new();
        let s1 = wb.add_worksheet();
        s1.set_name("Data").unwrap();
        s1.write_string(0, 0, "Item").unwrap();
        s1.write_string(0, 1, "Qty").unwrap();
        s1.write_string(1, 0, "Bolt").unwrap();
        s1.write_number(1, 1, 120.0).unwrap();
        let s2 = wb.add_worksheet();
        s2.set_name("Notes").unwrap();
        s2.write_string(0, 0, "<hello>").unwrap();
        wb.save_to_buffer().unwrap()
    }

    #[test]
    fn reads_every_sheet_of_a_real_workbook() {
        let html = render_workbook(&fixture(), &Caps::default()).unwrap();
        assert!(html.contains("<h2>Data</h2>"), "{html}");
        assert!(html.contains("<h2>Notes</h2>"), "{html}");
        assert!(html.contains("<th>Item</th>"), "{html}");
        assert!(html.contains("<td>Bolt</td>"), "{html}");
        assert!(html.contains("<td>120</td>"), "number formatting: {html}");
    }

    #[test]
    fn escapes_content_that_came_from_the_workbook() {
        let html = render_workbook(&fixture(), &Caps::default()).unwrap();
        assert!(!html.contains("<hello>"), "{html}");
        assert!(html.contains("&lt;hello&gt;"), "{html}");
    }

    #[test]
    fn rejects_a_workbook_over_the_byte_cap() {
        let caps = Caps { max_file_bytes: 16, ..Caps::default() };
        let err = render_workbook(&fixture(), &caps).unwrap_err();
        assert!(err.contains("too large"), "{err}");
    }

    #[test]
    fn reports_a_readable_error_for_a_corrupt_workbook() {
        let err = render_workbook(b"not a zip archive at all", &Caps::default()).unwrap_err();
        assert!(!err.is_empty());
        assert!(err.to_lowercase().contains("spreadsheet"), "{err}");
    }
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd src-tauri && cargo test xlsx
```

Expected: FAIL — `cannot find function render_workbook in this scope`.

- [ ] **Step 4: Implement**

Add to `xlsx.rs` above the test module:

```rust
use calamine::{Data, Reader};
use std::io::Cursor;

fn from_calamine(d: &Data) -> Cell {
    match d {
        Data::Empty => Cell::Empty,
        Data::String(s) => Cell::Text(s.clone()),
        Data::Float(f) => Cell::Number(*f),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Bool(b) => Cell::Bool(*b),
        Data::Error(e) => Cell::Error(format!("{e:?}")),
        Data::DateTime(dt) => Cell::DateTime(dt.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Text(s.clone()),
    }
}

pub fn render_workbook(bytes: &[u8], caps: &Caps) -> Result<String, String> {
    if bytes.len() > caps.max_file_bytes {
        return Err(format!(
            "spreadsheet is too large to preview ({} MB; limit {} MB)",
            bytes.len() / (1024 * 1024),
            caps.max_file_bytes / (1024 * 1024)
        ));
    }

    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("cannot read spreadsheet: {e}"))?;

    let names = wb.sheet_names().to_vec();
    let mut sheets = Vec::with_capacity(names.len());
    for name in names {
        let range = wb
            .worksheet_range(&name)
            .map_err(|e| format!("cannot read spreadsheet sheet '{name}': {e}"))?;
        sheets.push(Sheet {
            name,
            rows: range
                .rows()
                .take(caps.max_rows_per_sheet)
                .map(|r| r.iter().map(from_calamine).collect())
                .collect(),
        });
    }
    Ok(render_sheets(&sheets, caps))
}
```

The `.take(caps.max_rows_per_sheet)` here is a second line of defense: it bounds memory *while reading*, before `render_sheets` bounds the output. A workbook declaring a million rows must not be materialized in full just to be truncated afterwards.

The `from_calamine` match and the `render_workbook` body above were **compile-verified against calamine 0.36.1** while this plan was written — the exact code shown builds unchanged. If a later patch release moves a variant, `from_calamine` is the single point of contact; do not change `render_sheets`.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd src-tauri && cargo test xlsx
```

Expected: all 15 tests PASS.

- [ ] **Step 6: Run the full gate and commit**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/xlsx.rs
git commit -m "Read workbooks with calamine behind the pure renderer

Row cap is applied while reading, not only while rendering: a workbook
declaring a million rows must not be materialized in full to be
truncated afterwards."
```

---

## Task 7: Wire spreadsheets into `render_file` and style them

**Files:**
- Modify: `src-tauri/src/commands.rs` (`render_file`, ~line 173-185)
- Modify: `ui/styles.css`

**Interfaces:**
- Consumes: `xlsx::is_spreadsheet_path`, `xlsx::render_workbook`, `xlsx::Caps` from Tasks 5–6; the `sheet-body` class wired in Task 2 Step 4.
- Produces: `.xlsx` tabs that render as HTML tables.

- [ ] **Step 1: Write the failing test**

Add to `commands.rs`'s existing `mod tests`:

```rust
    #[test]
    fn render_file_renders_a_spreadsheet_as_a_table() {
        use rust_xlsxwriter::Workbook;
        let dir = std::env::temp_dir().join(format!("mdv-xlsx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("book.xlsx");

        let mut wb = Workbook::new();
        let s = wb.add_worksheet();
        s.set_name("Data").unwrap();
        s.write_string(0, 0, "Item").unwrap();
        s.write_string(1, 0, "Bolt").unwrap();
        std::fs::write(&f, wb.save_to_buffer().unwrap()).unwrap();

        let out = render_file(f.to_string_lossy().into_owned(), None, None, None).unwrap();
        let html = out.html.unwrap();
        assert!(html.contains("<h2>Data</h2>"), "{html}");
        assert!(html.contains("<th>Item</th>"), "{html}");
        assert!(!html.contains("Can't preview"), "{html}");

        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run to verify it fails**

```bash
cd src-tauri && cargo test render_file_renders_a_spreadsheet
```

Expected: FAIL — the assertion on `<h2>Data</h2>` fails because the file is currently reported as unsupported binary.

- [ ] **Step 3: Add the branch**

In `commands.rs::render_file`, change the `html` chain so the spreadsheet arm sits **before** `is_binary` (xlsx is a zip and would otherwise be rejected as binary):

```rust
    let html = if markdown::is_markdown_path(&p) {
        let contents = String::from_utf8_lossy(&bytes);
        if raw {
            markdown::render_plain(&contents)
        } else {
            markdown::render_markdown(&contents, theme)
        }
    } else if crate::xlsx::is_spreadsheet_path(&p) {
        crate::xlsx::render_workbook(&bytes, &crate::xlsx::Caps::default())?
    } else if crate::code::is_binary(&bytes) {
        crate::code::unsupported_html()
    } else {
        let contents = String::from_utf8_lossy(&bytes);
        crate::code::render_code(&contents, &p, theme)
    };
```

The `?` propagates as the command's `Err(String)`, which the frontend surfaces through the existing `showError` path — a corrupt workbook reports why rather than rendering a blank pane.

- [ ] **Step 4: Run to verify it passes**

```bash
cd src-tauri && cargo test render_file
```

Expected: PASS, including the two pre-existing `render_file_*` tests.

- [ ] **Step 5: Style the tables**

Append to `ui/styles.css`:

```css
.sheet-body {
  padding: 24px 32px;
  overflow-x: auto;
}

.sheet-body h2 {
  font-size: 1.1rem;
  margin: 24px 0 8px;
  color: var(--fg);
}

.sheet-body h2:first-child {
  margin-top: 0;
}

.sheet-body table {
  border-collapse: collapse;
  font-variant-numeric: tabular-nums;
  font-size: 0.9rem;
}

.sheet-body th,
.sheet-body td {
  border: 1px solid var(--border);
  padding: 4px 10px;
  text-align: left;
  white-space: nowrap;
}

.sheet-body th {
  background: var(--surface-2);
  font-weight: 600;
  position: sticky;
  top: 0;
}

.sheet-truncated,
.sheet-empty {
  color: var(--fg-muted);
  font-style: italic;
  margin: 8px 0 0;
}
```

Confirm the four CSS variables used here (`--fg`, `--border`, `--surface-2`, `--fg-muted`) exist in `styles.css`'s `:root` block; if any is named differently in this codebase, use the existing name — do not add new variables for this.

- [ ] **Step 6: Build and smoke test**

```bash
cd src-tauri && cargo build && cargo run -- ..
```

With any `.xlsx` in the tree, confirm:

1. It renders as one heading + table per sheet, in workbook order.
2. Numbers show as `120`, not `120.0`; dates as `2023-03-15`, not `45000`.
3. Edit, Raw, and Review buttons are absent.
4. ⌘F finds text inside the table and highlights it.
5. Dark mode: toggle the theme — borders and header background follow it.
6. Switch tabs away and back: it repaints instantly (sheets *are* retained).
7. Edit the file in Excel and save: the tab live-reloads.

- [ ] **Step 7: Run the full gate and commit**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git add src-tauri/src/commands.rs ui/styles.css
git commit -m "Render spreadsheets through render_file

The arm sits above the is_binary check because xlsx is a zip. Emitting
ordinary HTML into #preview is what earns find, print, the render cache
and retained DOM without any sheet-specific handling."
```

---

## Task 8: Allow export for spreadsheets

**Files:**
- Modify: `ui/app.js` (`onExport`, ~line 2235)

**Interfaces:**
- Consumes: `isExportable`, `viewKind` from Task 1.
- Produces: HTML and PDF export enabled for sheet tabs; still refused for image, pdf, and code tabs.

- [ ] **Step 1: Relax the allowlist**

Replace:

```js
  if (!isMarkdownPath(t.path)) {
    showTransientError("Export is only available for Markdown documents.");
    return;
  }
```

with:

```js
  if (!isExportable(viewKind(t.path))) {
    showTransientError("Export is only available for Markdown and spreadsheets.");
    return;
  }
```

- [ ] **Step 2: Verify the refusals still hold**

`isExportable` is false for `code`, `image`, and `pdf`, so this widens the allowlist by exactly one kind. Confirm by re-reading the capability table in `ui/filetype.js` — the Task 1 test already asserts every row, so no new unit test is needed here.

- [ ] **Step 3: Build and smoke test**

```bash
cd src-tauri && cargo build && cargo run -- ..
```

1. Open a `.xlsx`, then **File ▸ Export as HTML…**. The written file opens in a browser showing the tables. Because the exported document sets no `data-theme`, it must be **light** regardless of the app's current theme — check with dark mode on.
2. **File ▸ Export as PDF…** (macOS). Tables paginate, and the header row repeats on each page — that comes free from the existing `thead { display: table-header-group }` print rule, which is why the first row is a `<thead>`.
3. Open a `.pdf` tab and try both exports: refused with the message above.
4. Open a `.rs` tab and try: refused.

- [ ] **Step 4: Run the full gate and commit**

```bash
node --test ui/*.test.js
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git add ui/app.js
git commit -m "Allow HTML and PDF export for spreadsheet tabs

Sheets render as ordinary HTML in #preview, so both export paths already
work; the allowlist was the only thing refusing them."
```

---

## Task 9: Security review of the spreadsheet path

**Files:**
- Modify: whatever the review finds.

**Interfaces:**
- Consumes: the complete Phase 3 implementation.

The xlsx path parses untrusted, attacker-controllable zip archives in Rust — new territory for this codebase, and the reason the caps are in the design rather than deferred.

- [ ] **Step 1: Run the security reviewer**

Dispatch the `security-reviewer` agent against the diff for Tasks 5–8, asking it specifically to check:

- Cap enforcement cannot be bypassed: a zip bomb, a sheet declaring a huge used-range, or a workbook with thousands of sheets must not exhaust memory before a cap applies.
- Every string reaching the HTML output passes through `escape_html` — sheet names and cell text both, including the `Cell::Error` and `Cell::Text` arms.
- `render_workbook`'s error strings do not leak absolute filesystem paths beyond what `render_file` already reports.
- No panic path: `unwrap`/`expect`/slicing/arithmetic overflow on adversarial input. `render_file` is an IPC command, so a panic is a denial of service.

- [ ] **Step 2: Fix what it finds, re-run the gate**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "Address security review of the spreadsheet parser"
```

---

## Task 10: Documentation

**Files:**
- Modify: `CLAUDE.md`, `README.md` (see Step 3 on why `CHANGELOG.md` is not touched)

- [ ] **Step 1: `CLAUDE.md`**

Add `xlsx.rs` to the file-layout block (after `watcher.rs`):

```
    xlsx.rs       — calamine workbook → HTML tables; pure Cell/Sheet types,
                    Excel serial-date conversion, row/cell caps (unit-tested)
```

Add `viewKind` to the `ui/` block, replacing the `filetype.js` line:

```
  filetype.js     — viewKind(path) → markdown|image|pdf|sheet|code + the
                    capability table (isEditable, hasRawToggle, …); unit-tested
```

Add an architecture bullet after the **Image files** one:

```
- **PDF files**: frontend-only. `renderPdf` frames the file via
  `convertFileSrc` for the webview's native viewer, short-circuiting *before*
  the `render_file` IPC (the backend would reject a PDF as binary). Not
  retained in the DOM cache — an iframe reloads on reattach regardless. Find
  and export are refused; the viewer has its own search.
- **Spreadsheets**: `xlsx.rs` (calamine) renders xlsx/xlsm/xlsb/xls/ods to
  HTML tables in `render_file`, in an arm placed *above* the `is_binary`
  check because xlsx is a zip. Values and cached formula results only — no
  fonts, colors, or column widths. Because the output is ordinary HTML in
  `#preview`, find, both exports, print, the render cache and retained DOM
  all work with no sheet-specific handling. First row becomes `<thead>`, so
  the existing print rule repeats headers across PDF pages.
```

Add to **Things that took hours and shouldn't again**:

```
- **`viewKind`, not a pile of `isXPath` checks**: the gates in `app.js` ask
  four different questions (editable? split preview? raw toggle? retainable?),
  which `isImagePath`/`isCodeView` used to conflate. Adding a view type is a
  row in `CAPABILITIES` in `ui/filetype.js`. If you find yourself writing
  `|| isPdfPath(...)` at a call site, the answer belongs in the table instead.
- **The xlsx arm must precede `is_binary`** in `render_file`. xlsx is a zip;
  put it after and every workbook renders "Can't preview this file type".
```

- [ ] **Step 2: `README.md`**

Add to the Features list:

```
- **PDF and spreadsheet preview** — open `.pdf` files in the built-in viewer,
  and `.xlsx`/`.xls`/`.ods` workbooks as browsable data tables.
```

Note the limitations in Usage: PDFs use the viewer's own search rather than
⌘F, and spreadsheets show values, not formatting.

- [ ] **Step 3: `CHANGELOG.md`**

This repo has **no `[Unreleased]` section** — every heading is `## [X.Y.Z] - <date>`, and the release workflow extracts the one matching the tag. So do **not** add a heading here. Instead, append the bullets below to the plan's completion notes so whoever cuts the next release drops them into that version's `### Added` block:

```
- Preview PDF files in a built-in viewer.
- Preview Excel and OpenDocument spreadsheets as data tables, with export to
  HTML and PDF.
```

Leave `CHANGELOG.md` itself untouched in this task; adjust the commit in Step 4 accordingly.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md README.md
git commit -m "Document PDF and spreadsheet preview"
```

---

## Task 11: Full-branch verification

- [ ] **Step 1: Clean build and complete test run**

```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cd .. && node --test ui/*.test.js
```

All four must pass with no warnings.

- [ ] **Step 2: Bundle smoke test (macOS)**

```bash
./scripts/smoke-test.sh
```

- [ ] **Step 3: Regression sweep on the pre-existing view types**

The seam refactor touched every gate, so re-confirm the kinds that already worked:

| Tab type | Expect |
|---|---|
| `.md` | Edit (split preview), Raw toggles, Review enters and copies, export works, ⌘F works, retained on tab switch |
| `.rs` | Edit works **in place** (no preview pane), no Raw, no Review, no export |
| `.png` | Renders, no Edit/Raw/Review, live-reloads on overwrite |
| `.pdf` | Renders in the viewer, no Edit/Raw/Review/export, ⌘F redirects |
| `.xlsx` | Renders as tables, exports, ⌘F works, retained on tab switch |

- [ ] **Step 4: Finish the branch**

Use the `superpowers:finishing-a-development-branch` skill to decide how to integrate.

---

## Self-review notes

- **Spec coverage:** seam → Tasks 1–2; PDF → Tasks 3–4; Excel → Tasks 5–8; caps → Tasks 5–6; security review → Task 9; Windows parity → Task 4; export relaxation → Task 8; `escape_html` visibility → Task 5 Step 1; `sheet-body` class → Task 2 Step 4 + Task 7 Step 5. The spec's HTML section is explanatory only and intentionally has no task.
- **Verified while planning:** all seven `excel_serial_to_iso` assertions were run against a reference implementation of the same algorithm and agree exactly, so a failure there means a transcription error rather than a wrong expectation.
- **Verified while planning:** Task 6's `Data` match and `render_workbook` body compile unchanged against calamine 0.36.1, and `rust_xlsxwriter` 0.98.2 resolves alongside it. The `from_calamine` boundary remains the single point of contact if a future release moves a variant.
