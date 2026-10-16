# 3D Model Viewer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Open `.stl` / plain `.3mf` / `.obj` files in an MDViewer tab as a rotatable, live-reloading 3D view with a bounding-box dimensions readout.

**Architecture:** A new `model` view kind in `ui/filetype.js` short-circuits `renderActive` (like image/PDF) into `renderModel`, which fetches bytes through one new size- and extension-capped Rust command (`read_model`, returning a raw `ArrayBuffer`) and hands them to a lazily-imported, per-window shared three.js viewer. Pure logic lives in `src-tauri/src/model.rs` and `ui/model.js`, both unit-tested; WebGL/DOM wiring stays thin.

**Tech Stack:** Rust (Tauri 2.11, `zip` 8 already a direct dep), vanilla ES modules, vendored three.js (OrbitControls, STLLoader, 3MFLoader, OBJLoader, fflate).

**Spec:** `docs/superpowers/specs/2026-10-16-3d-model-viewer-design.md`

## Global Constraints

- Formats: `stl`, `3mf`, `obj`, case-insensitive. Nothing else.
- `MAX_MODEL_BYTES = 200 * 1024 * 1024`; `MAX_3MF_DECLARED_BYTES = 512 * 1024 * 1024`.
- Bambu refusal copy, verbatim: `This looks like a Bambu Studio project file; only plain 3MF models are supported`.
- No CSP change. No `'unsafe-eval'`, no inline scripts, no import maps.
- three.js is loaded only via dynamic `import()` on first model open — never from `index.html`.
- One `WebGLRenderer` per window, reused across all model tabs.
- All formats rotated Z-up → Y-up (−90° about X). Dimensions labeled `mm`.
- `modelCamera` per-tab state is ephemeral — never serialized in session restore.
- Theme toggle on a model tab: no `read_model` call, no camera change.
- No `bundle.fileAssociations` change; Finder `RunEvent::Opened` stays markdown-only.
- Text from model files reaches the DOM only via `textContent`.
- Gate per task: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`, and `cd ui && node --test`. Frontend changes need `cargo build` before they show in the app.
- Commit messages: imperative subject, no `Co-Authored-By: Claude` trailer.

## Review Focus

1. **Half-written file during a Claude rewrite** — a `file-changed` that reads a truncated STL shows the inline error, and the *next* `file-changed` repaints the model with the camera still kept. (Task 6 manual step 6.)
2. **Fast tab switching while a model loads** — a load that resolves after the active tab/path changed must not paint, must not touch the new tab's DOM. (Task 6: `loadSeq` guard; `keepCamera` tests in Task 4.)
3. **Binary STL whose 80-byte header begins with `solid`** (SolidWorks and some trimesh outputs) must parse as binary, not ASCII. (Task 6 fixture `solid-header-binary.stl`.)
4. **Degenerate bounds** — a single-triangle, flat (zero Z), or zero-size model must frame without NaN camera and format dimensions as `0.0`. (Task 4 tests.)
5. **Pane resize** (window resize, sidebar splitter drag) — canvas resizes without stretching and redraws once. (Task 6: `ResizeObserver`; manual step 5.)

---

### Task 1: `model.rs` + `read_model` command

**Files:**
- Create: `src-tauri/src/model.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod model;`, register `commands::read_model` in `generate_handler!`)
- Modify: `src-tauri/src/commands.rs` (thin `read_model` wrapper)

**Interfaces:**
- Produces (Rust, `model.rs`):
  - `pub const MAX_MODEL_BYTES: u64`, `pub const MAX_3MF_DECLARED_BYTES: u64`
  - `pub fn is_model_path(p: &Path) -> bool`
  - `pub fn check_size(len: u64, max: u64) -> Result<(), String>`
  - `pub fn is_bambu_project<'a>(names: impl IntoIterator<Item = &'a str>) -> bool`
  - `pub fn check_3mf(bytes: &[u8], max_declared: u64) -> Result<(), String>` — opens a `zip::ZipArchive` over a `Cursor`, collects `by_index_raw(i)` name + `size()`; errors on unreadable zip (`"not a valid 3MF archive: {e}"`), on Bambu project (verbatim copy), on declared sum > max (`"3MF declares too much uncompressed data ({n} MB; limit {m} MB)"`).
  - `pub fn read_model_bytes(p: &Path) -> Result<Vec<u8>, String>` — extension check (`"not a 3D model file: {path}"`), metadata length vs `MAX_MODEL_BYTES` before reading, read, `check_3mf` when extension is `3mf`.
- Produces (IPC): `read_model(path: String) -> Result<tauri::ipc::Response, String>` — JS receives an `ArrayBuffer`.

- [ ] **Step 1: Write failing tests in `model.rs` `#[cfg(test)]`**

  - `is_model_path_accepts_print_formats`: `a.stl`, `B.STL`, `c.3mf`, `d.3MF`, `e.obj` → true; `a.step`, `a.scad`, `a.stl.exe`, `a`, `a.stlx`, `a.md` → false.
  - `check_size_rejects_above_cap`: `check_size(10, 10)` Ok; `check_size(11, 10)` Err containing `"too large"`.
  - `is_bambu_project_detects_objects_dir_and_project_settings`: `["3D/3dmodel.model","[Content_Types].xml"]` → false; adding `"3D/Objects/object_1.model"` → true; `["Metadata/project_settings.config"]` → true.
  - `check_3mf_accepts_plain_archive`: in-memory zip (`zip::ZipWriter`, `CompressionMethod::Stored`) with `[Content_Types].xml` + `3D/3dmodel.model` → Ok.
  - `check_3mf_refuses_bambu_project`: same plus `3D/Objects/object_1.model` → Err equal to the verbatim Bambu copy.
  - `check_3mf_refuses_declared_bytes_over_cap`: plain archive, `max_declared` = 1 → Err containing `"too much uncompressed data"`.
  - `check_3mf_refuses_non_zip`: `b"solid x"` → Err starting `"not a valid 3MF archive"`.
  - `read_model_bytes_refuses_non_model_extension`: temp `x.md` → Err starting `"not a 3D model file"`.
  - `read_model_bytes_reads_stl`: temp `x.stl` with `b"solid t\nendsolid t\n"` → Ok equal to those bytes.

- [ ] **Step 2: Run `cd src-tauri && cargo test model::` — expect compile failure / FAIL.**

- [ ] **Step 3: Implement `model.rs` per the Interfaces block; add `mod model;` to `lib.rs`.**

- [ ] **Step 4: Add the command** in `commands.rs`:
  `#[tauri::command] pub fn read_model(path: String) -> Result<tauri::ipc::Response, String>` = `model::read_model_bytes(Path::new(&path)).map(tauri::ipc::Response::new)`; register it in `lib.rs`'s `generate_handler!` after `render_file`.

- [ ] **Step 5: Run the full gate** (fmt, clippy `-D warnings`, `cargo test`) — expect PASS.

- [ ] **Step 6: Commit** — `Add read_model command for 3D model bytes`.

### Task 2: MCP allowlist + Open File filter

**Files:**
- Modify: `src-tauri/src/mcp.rs` (~L190–215 ext lists + `viewable_path`; ~L102 tool description; tests ~L757)
- Modify: `src-tauri/src/mcp_server.rs:452` (error message)
- Modify: `src-tauri/src/menu.rs:303` (dialog filter)

**Interfaces:** Consumes nothing from Task 1 (separate ext list in `mcp.rs`, mirroring `MODEL_EXT` in `ui/filetype.js`, same as existing lists).

- [ ] **Step 1: Extend the test** `viewable_path_allowlists_markdown_images_pdf_and_sheets` → rename to `viewable_path_allowlists_markdown_images_pdf_sheets_and_models`; add accepted `m.stl`, `m.STL`, `m.3mf`, `m.obj`; add rejected `m.step`, `m.scad`, `m.stlx`. Update its mirror comment to include `MODEL_EXT`.
- [ ] **Step 2: `cargo test mcp::` — expect FAIL on `m.stl`.**
- [ ] **Step 3: Add `const MODEL_EXTS: [&str; 3] = ["stl", "3mf", "obj"];` and include it in `viewable_path`.** Update the `open_document` `path` description to `"Path to a markdown, image, PDF, spreadsheet (.xlsx/.xlsm/.xlsb/.xls/.ods), or 3D model (.stl/.3mf/.obj) file. …"` (keep the rest), and the `validate` error to `"'{p}' is not a markdown, image, PDF, spreadsheet, or 3D model file"`.
- [ ] **Step 4: In `menu.rs` add `.add_filter("3D models", &["stl", "3mf", "obj"])` between the Markdown and All files filters.**
- [ ] **Step 5: Full gate — PASS.**
- [ ] **Step 6: Commit** — `Let open_document and Open File accept 3D models`.

### Task 3: `model` view kind

**Files:**
- Modify: `ui/filetype.js`, `ui/filetype.test.js`

**Interfaces:**
- Produces: `export const MODEL_EXT = /\.(stl|3mf|obj)$/i;` and `viewKind(path) === "model"`. Capability row: `editable:false, splitPreview:false, rawToggle:false, annotatable:false, retainable:false, exportable:false, ownRenderPath:true, cacheBust:false`.

- [ ] **Step 1: Tests:** `viewKind("a.stl")`, `("A.STL")`, `("b.3mf")`, `("c.obj")` → `"model"`; `viewKind("p.scad")` → `"code"`; for kind `"model"`: `rendersFromDisk` true, and `isEditable`, `hasRawToggle`, `isAnnotatable`, `isRetainable`, `isExportable`, `bustsCacheOnChange`, `hasSplitPreview` all false.
- [ ] **Step 2: `cd ui && node --test filetype.test.js` — FAIL.**
- [ ] **Step 3: Implement** (check `MODEL_EXT` after `SHEET_EXT`, before the `code` fallback).
- [ ] **Step 4: `node --test` — PASS.** Note: until Task 6, `renderActive`'s `rendersFromDisk` branch would send a model to `renderPdf`; Task 6 replaces that branch. Do not ship Task 3 alone.
- [ ] **Step 5: Commit** — `Add model view kind for STL, 3MF and OBJ`.

### Task 4: `ui/model.js` pure helpers

**Files:**
- Create: `ui/model.js`, `ui/model.test.js`

**Interfaces:**
- Produces:
  - `modelFormat(path: string) => "stl" | "3mf" | "obj" | null`
  - `formatDimensions(min: {x,y,z}, max: {x,y,z}, unit = "mm") => string` — one decimal each, `" × "` separator (U+00D7), e.g. `"120.0 × 45.5 × 30.0 mm"`; negative-zero renders `0.0`.
  - `frameCamera(center: {x,y,z}, radius: number, fovDeg: number, aspect: number) => { position: {x,y,z}, target: {x,y,z}, near: number, far: number }` — radius floored to `1e-3`; distance fits the sphere in the narrower of vertical/horizontal FOV with 1.25× margin; direction is front-right-above, normalized `(1, 0.8, 1)` in Y-up world; `near = distance / 100`, `far = distance * 100`.
  - `keepCamera(prevPath: string|null, nextPath: string, reason: "reload"|"switch"|"open") => boolean` — true only when `reason === "reload" && prevPath === nextPath`.

- [ ] **Step 1: Tests** (`node:test` + `assert/strict`):
  - `modelFormat`: `a.STL`→`"stl"`, `b.3mf`→`"3mf"`, `c.obj`→`"obj"`, `d.md`→`null`, `""`→`null`.
  - `formatDimensions({x:0,y:0,z:0},{x:120,y:45.54,z:30})` → `"120.0 × 45.5 × 30.0 mm"`; `({x:-10,y:-5,z:0},{x:10,y:5,z:0})` → `"20.0 × 10.0 × 0.0 mm"`; `({x:1,y:1,z:1},{x:1,y:1,z:1})` → `"0.0 × 0.0 × 0.0 mm"`.
  - `frameCamera` with `radius: 0` → all numbers finite, `near > 0`, `far > near`; with `radius: 50, fovDeg: 45, aspect: 1` → distance from target ≥ `50 / Math.sin(22.5°)`; with `aspect: 0.5` → distance larger than with `aspect: 1`; `target` equals `center`.
  - `keepCamera("/a.stl","/a.stl","reload")` true; `("/a.stl","/b.stl","reload")` false; `("/a.stl","/a.stl","switch")` false; `(null,"/a.stl","open")` false.
- [ ] **Step 2: `node --test model.test.js` — FAIL.**
- [ ] **Step 3: Implement `ui/model.js`** (no DOM, no three.js import).
- [ ] **Step 4: `node --test` — PASS.**
- [ ] **Step 5: Commit** — `Add pure helpers for the 3D model viewer`.

### Task 5: Vendor three.js

**Files:**
- Create: `ui/three/three.module.min.js`, `ui/three/three.core.min.js` (if the chosen release splits core out — vendor whatever `three.module.min.js` imports), `ui/three/OrbitControls.js`, `ui/three/STLLoader.js`, `ui/three/3MFLoader.js`, `ui/three/OBJLoader.js`, `ui/three/fflate.module.js`, `ui/three/README.md`

- [ ] **Step 1: Download** a pinned current stable three.js release from `https://cdn.jsdelivr.net/npm/three@<ver>/` (`build/` and `examples/jsm/{controls,loaders,libs}/`) into a scratch dir, then copy the listed files into `ui/three/`.
- [ ] **Step 2: Rewrite import specifiers** in the addons: `from 'three'` → `from './three.module.min.js'`; `'../libs/fflate.module.js'` → `'./fflate.module.js'`. Verify: `grep -nE "from ['\"](three|\.\./)" ui/three/*.js` prints nothing.
- [ ] **Step 3: Write `ui/three/README.md`**: version, source URLs, the exact rewrite rules from Step 2, and "re-vendoring must repeat the rewrite; import maps are blocked by CSP `script-src 'self'`".
- [ ] **Step 4: Smoke the module graph** under node: `node -e "import('./ui/three/STLLoader.js').then(m=>console.log(typeof m.STLLoader))"` → `function` (repeat for `3MFLoader.js` → `ThreeMFLoader`, `OBJLoader.js`, `OrbitControls.js`; OrbitControls may need `globalThis.window` stubbing — a resolution error is the only failure that matters here).
- [ ] **Step 5: Commit** — `Vendor three.js <ver> for the 3D model viewer`.

### Task 6: Viewer + `app.js` wiring

**Files:**
- Create: `ui/model-viewer.js`
- Modify: `ui/app.js` (`renderActive` ~L1895, `setActiveTab` ~L1417, `applyTheme` ~L1848, tab construction where `scrollTop` is initialized, session serialization guard)
- Modify: `ui/styles.css` (`.model-view`, overlay, reset button, `.model-error` reusing `.image-error` styling)
- Create: `src-tauri/tests/fixtures/models/{cube-ascii.stl, cube-binary.stl, solid-header-binary.stl, two-parts.3mf, cube.obj, bambu-project.3mf}` + `src-tauri/tests/fixtures/models/make_fixtures.py` (stdlib only: `struct`, `zipfile`) that regenerates the binary ones

**Interfaces:**
- Consumes: `read_model` (Task 1), `viewKind === "model"` (Task 3), `formatDimensions`, `frameCamera`, `keepCamera` (Task 4), `ui/three/*` (Task 5).
- Produces, `ui/model-viewer.js` (default-exported factory, one instance per window):
  - `createModelViewer({ getTheme: () => "light"|"dark" }) => viewer`
  - `viewer.show(container: HTMLElement, bytes: ArrayBuffer, format: "stl"|"3mf"|"obj", { camera: {position,target}|null }) => void` — parses, disposes the previous object's geometries/materials, computes the dimensions box **before** rotating (so X/Y/Z are the file's — the slicer's — axes, not three's Y-up world), applies the −90° X rotation, adds to scene, sets dimensions overlay via `formatDimensions`, frames via `frameCamera` when `camera` is null, appends the canvas+overlay wrapper into `container`. Throws `Error` with a readable message on parse failure or empty geometry (`"the file contains no geometry"`).
  - `viewer.camera() => { position, target }` — current camera for stashing.
  - `viewer.applyTheme() => void` — recolor background/material/grid, redraw.
  - `viewer.resetView() => void`
- Produces, `app.js`: `async function renderModel(t, { reason })` with `reason ∈ "reload"|"switch"|"open"`; module-level `let modelViewer = null; let modelLoadSeq = 0; let modelShownPath = null;`; tab field `modelCamera: null`.

- [ ] **Step 1: Generate fixtures.** `make_fixtures.py` writes: binary 12-triangle cube (20 mm); a binary cube whose 80-byte header starts with `b"solid "`; `two-parts.3mf` (plain: `[Content_Types].xml`, `_rels/.rels`, `3D/3dmodel.model` with two `<object>`s and two `<item>`s, `unit="millimeter"`); `bambu-project.3mf` (adds `3D/Objects/object_1.model`). Hand-write `cube-ascii.stl` and `cube.obj` (20 mm cube). Run it; commit outputs alongside the script.
- [ ] **Step 2: Implement `ui/model-viewer.js`** per Interfaces: `WebGLRenderer({ antialias: true })`, `setPixelRatio(devicePixelRatio)`; `PerspectiveCamera(45)`; `OrbitControls` with `enableDamping: false` and a `change` listener that calls `render()`; `HemisphereLight` + a `DirectionalLight` parented to the camera; `MeshStandardMaterial` (flat grey, `side: DoubleSide`) applied to every mesh in the loaded object (OBJ/3MF loaders produce their own materials — replace them); `GridHelper` sized to the footprint, placed at the model's min Y; `ResizeObserver` on `container` → `setSize` + camera aspect + `render()`; double-click on the canvas → `resetView()`; Reset view button in the overlay. Render on demand only — no `setAnimationLoop`. Loaders: `new STLLoader().parse(bytes)` → `Mesh`; `new ThreeMFLoader().parse(bytes)` → `Group`; `new OBJLoader().parse(new TextDecoder().decode(bytes))` → `Group`. 3MF unit: check whether the loader applies `<model unit>`; if not, scale to mm from that attribute (`micron` 0.001, `millimeter` 1, `centimeter` 10, `inch` 25.4, `foot` 304.8, `meter` 1000).
- [ ] **Step 3: Wire `app.js`:**
  - `renderActive`: `if (kind === "model") { await renderModel(t, { reason: modelShownPath === t.path ? "reload" : "open" }); return; }` before the image/pdf line. (A tab switch reaches `renderActive` too; `setActiveTab` passes the reason — see next bullet — so thread an optional `modelReason` through `renderActive`'s options and prefer it when set.)
  - `setActiveTab`: when `outgoing` is a model tab and `modelViewer`, set `outgoing.modelCamera = modelViewer.camera()`; when rendering an incoming model tab, `modelReason: "switch"`.
  - `renderModel`: `const seq = ++modelLoadSeq;` close find; lazy `modelViewer ??= (await import("./model-viewer.js")).createModelViewer({ getTheme: () => currentTheme })`; `const bytes = await invoke("read_model", { path: t.path })`; after every `await`, bail if `seq !== modelLoadSeq || activeTab() !== t || t.path !== path-captured-at-start`. Camera: `keepCamera(modelShownPath, t.path, reason) ? modelViewer.camera() : (reason === "switch" ? t.modelCamera : null)`. `preview.className = "model-view"`; `preview.replaceChildren()`; `modelViewer.show(preview, bytes, modelFormat(t.path), { camera })`; `modelShownPath = t.path`; `liveRender = null`. On error: `modelShownPath = null` is **not** set (so the next reload still keeps the camera), render `<div class="model-error">` with `textContent = "Can't display this model: " + message`.
  - `applyTheme`: if the active tab's kind is `"model"` and `modelViewer`, call `modelViewer.applyTheme()` and **skip** `renderActive`.
  - Any non-model paint (`paintHtml`, `renderImage`, `renderPdf`, `showEmptyState`, `showError`) sets `modelShownPath = null`.
  - New tabs get `modelCamera: null`; confirm `persistSession` serializes only whitelisted fields (if it serializes the tab object wholesale, exclude `modelCamera` as Review fields are excluded).
- [ ] **Step 4: Styles**: `.model-view` fills the preview pane (no `markdown-body` max-width), `position: relative`; overlay bottom-left using existing CSS variables for text/background; reset button top-right; dark-theme variants via `:root[data-theme="dark"]` (no `prefers-color-scheme`).
- [ ] **Step 5: `cd src-tauri && cargo build`, then `cargo run -- tests/fixtures/models`** and verify manually: each fixture opens; Bambu fixture shows the verbatim refusal; `solid-header-binary.stl` renders a cube; dimensions read `20.0 × 20.0 × 20.0 mm`; drag rotates, scroll zooms, right-drag pans, Reset/double-click re-frames; resize window and drag the sidebar splitter (canvas follows, no stretch); toggle theme (recolors, camera unchanged); switch to a markdown tab and back (camera restored per tab); find (⌘F) is refused; Edit/Raw/Review/Export hidden.
- [ ] **Step 6: Live-reload checks**: rotate, then `python3 make_fixtures.py` (rewrites binary cube) → model repaints, camera unchanged. Then `head -c 200 cube-binary.stl > /tmp/x && cp /tmp/x cube-binary.stl` → inline error; rerun the script → model returns with camera kept. Rapidly click between two model tabs and a markdown tab — never shows the wrong file.
- [ ] **Step 7: Full gate** (`cargo fmt --check`, clippy, `cargo test`, `node --test`) — PASS.
- [ ] **Step 8: Commit** — `Render STL, 3MF and OBJ files in a rotatable 3D view`.

### Task 7: Docs + security review

**Files:**
- Modify: `CLAUDE.md` (file layout: `model.rs`, `ui/model.js`, `ui/model-viewer.js`, `ui/three/`; capability-table list in "View dispatch"; new "3D models" architecture bullet covering `read_model` + why not `fetch` (CSP), Bambu refusal, shared renderer, `modelCamera` ephemeral, theme path; MCP bullet's allowlist sentence; "Things that took hours" entry for the three.js import rewrite)
- Modify: `README.md` (Features list: 3D model viewing; Usage: controls)

- [ ] **Step 1: Update docs as listed.** Do not touch `CHANGELOG.md` (release-time step).
- [ ] **Step 2: Dispatch the `security-reviewer` agent** on the branch diff (`git diff main...HEAD`), focusing on `read_model`, the 3MF precheck, and DOM insertion of model-derived text. Fix any concrete finding, rerun the gate.
- [ ] **Step 3: Windows note**: add to the PR description that WebGL in WebView2 needs one manual smoke (open each fixture, rotate, live-reload) before release.
- [ ] **Step 4: Commit** — `Document the 3D model viewer`.
