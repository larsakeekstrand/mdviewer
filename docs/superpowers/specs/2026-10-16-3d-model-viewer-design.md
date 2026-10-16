# 3D model viewer (STL / 3MF / OBJ) — design

## Intent

The user works in repos that mix embedded software with 3D-printable parts
(printed on a Bambu Lab P1S). Claude generates the part files — via OpenSCAD,
CadQuery/build123d, or Python (trimesh) — and the loop is *Claude changes the
model → the user looks → the user gives feedback*. Today "looking" means a trip
to Bambu Studio. The goal is to click a model in MDViewer's tree and see its
shape immediately, rotate it, and have it refresh in place when Claude rewrites
the file.

The question the viewer answers is **"does this look right?"** — not "will this
print?".

## Goals

- Open `.stl` (binary and ASCII), `.3mf`, and `.obj` files in a tab, like any
  other file type, from the tree, Open File, session restore, and the MCP
  `open_document` tool.
- Interactive camera, OrbitControls defaults: left-drag rotates (orbit);
  scroll wheel / trackpad two-finger scroll / pinch zooms; right-drag
  (two-finger *click*-drag on a trackpad) or Shift/Ctrl/⌘ + left-drag pans.
  A **Reset view** button (and double-click on empty background) re-frames
  the model. (Our call: keep the defaults rather than remap two-finger scroll
  to pan.)
- **Live reload**: when the active model file changes on disk, the geometry is
  replaced in place and the user's current camera is kept.
- **Dimensions readout** (nice-to-have, but cheap and in scope): a small overlay
  showing the bounding-box size `X × Y × Z mm`.
- Works identically on macOS and Windows. Fully offline.

## Non-goals

- Slicing, printability analysis (overhangs, bed adhesion, manifold checks),
  measuring tools, cross-sections, editing.
- Colors, materials, textures, painted/multi-material data, print settings, or
  plates from 3MF. **Geometry only.**
- **Bambu Studio project files.** Decided by the user: only the plain 3MFs
  that Claude's pipelines write (trimesh, CadQuery/build123d — all geometry in
  `3D/3dmodel.model`) must render. Bambu Studio projects use the 3MF
  production extension (objects in separate `3D/Objects/*.model` parts, laid
  out across plates), which three's `3MFLoader` doesn't reliably follow. They
  get a clear refusal message instead of an empty view (see `read_model`).
- STEP/STP (needs an OpenCascade-class CAD kernel, ~10 MB WASM — too heavy for
  "does this look right").
- Rendering `.scad` sources (needs OpenSCAD). `.scad` keeps opening as text in
  the existing `code` view.
- Registering MDViewer as a Finder/Explorer handler for model files
  (`bundle.fileAssociations`) — it must not compete with Bambu Studio as the
  default app. Finder `RunEvent::Opened` stays markdown-only.
- Exporting a model view to HTML/PDF.

## Anti-goals (would make this a failure even if it "works")

- Widening the CSP (no `connect-src asset:`, no `'unsafe-eval'`, no inline
  scripts / import maps).
- Slowing app startup for people who never open a model: the 3D library must be
  loaded lazily, on first model open.
- Leaking WebGL contexts as the user flips between model tabs (WebKit and
  Chromium cap live contexts; exceeding it kills the oldest silently).
- Resetting the camera on every live reload — that defeats the iteration loop.

## How it fits the existing architecture

### File-type dispatch (`ui/filetype.js`)

Add `MODEL_EXT = /\.(stl|3mf|obj)$/i` and a `model` kind in `viewKind`, plus a
capability row:

| capability    | value | why |
|---------------|-------|-----|
| editable      | false | ASCII STL / OBJ are text, but editing vertex lists by hand isn't a use case (our call) |
| splitPreview  | false | |
| rawToggle     | false | |
| annotatable   | false | Review Mode is block-based markdown |
| retainable    | false | the canvas is driven by a shared renderer, not DOM we can stash (see below) |
| exportable    | false | |
| ownRenderPath | true  | `renderActive` must short-circuit **before** `render_file`, which would reject binary STL/3MF |
| cacheBust     | false | we don't load via asset URL, so there's nothing to cache-bust |

`renderActive`'s `rendersFromDisk` branch grows a third case: `renderModel(t)`.
Find (⌘F) is refused for model tabs (`closeFind()`, like images/PDF). Edit, Raw,
Review, Copy Source and Export controls are hidden/guarded through the
capability table — no new `isModelPath` checks at call sites (see the
"`viewKind`, not a pile of `isXPath` checks" lesson in CLAUDE.md).

### Getting the bytes into the webview — new IPC command

The CSP has no `connect-src`, so it falls back to `default-src 'self'` and a
`fetch(convertFileSrc(path))` of the model is **blocked**. Rather than widen the
CSP, add one Rust command:

- **`read_model(path: String) -> Result<tauri::ipc::Response, String>`** in
  `commands.rs` (thin wrapper) over a new pure-ish module
  `src-tauri/src/model.rs`. Returns the raw file bytes as an `ArrayBuffer`
  (`tauri::ipc::Response` avoids JSON-encoding a byte array). It:
  1. refuses any path whose extension isn't `stl`/`3mf`/`obj`
     (`model::is_model_path`, case-insensitive), so it can't be used as a
     general binary-read primitive;
  2. refuses files larger than `MAX_MODEL_BYTES` (proposed **200 MB**) by
     checking metadata before reading;
  3. for `.3mf` (a zip), sums the declared uncompressed sizes from the zip
     central directory **without inflating** (same `by_index_raw` technique as
     `xlsx.rs`'s `max_declared_bytes`) and refuses above
     `MAX_3MF_DECLARED_BYTES` (proposed **512 MB**) — a zip-bomb guard, since
     the JS loader inflates everything in memory. Like the xlsx precheck this
     is defense-in-depth, not a complete bound.
  4. for `.3mf`, from the same central-directory listing, detects a Bambu
     Studio / production-extension project — any entry under `3D/Objects/`,
     or `Metadata/project_settings.config` — and refuses with
     `"This looks like a Bambu Studio project file; only plain 3MF models are
     supported"`. Pure helper `model::is_bambu_project(entry_names)`,
     unit-tested.
  5. returns a readable error string on refusal or I/O failure
     (`format!("…: {e}")` convention).

  Path containment: `render_file` (the existing read path for every tab) does
  not confine reads to `current_root` — tabs can show files opened from
  anywhere — and `read_model` mirrors that. The extension allowlist is what
  keeps it narrow. This is the only new IPC command.

Pure helpers in `model.rs`, unit-tested under `#[cfg(test)]`:
`is_model_path`, `check_size`, `declared_zip_bytes`, `is_bambu_project` (tested with an in-memory
zip fixture built by the `zip` crate, as `xlsx.rs` does).

### Rendering — vendored three.js, lazily imported

- Vendor a pinned three.js release (builder picks a current stable `r1xx` and
  records the version in the vendor directory, as with mermaid) under
  `ui/three/`: `three.module.min.js` plus the addons `OrbitControls.js`,
  `STLLoader.js`, `3MFLoader.js`, `OBJLoader.js`, and `fflate.module.js` (the
  3MF loader's unzip dependency).
- The addons import from the bare specifier `'three'` and a relative
  `../libs/fflate.module.js`. Import maps are inline `<script>`s and are blocked
  by `script-src 'self'`, so **rewrite those import specifiers to relative
  paths** in the vendored copies (one-time, documented in a short
  `ui/three/README` noting the version and the rewrite so a re-vendor repeats
  it).
- `app.js` loads it with a dynamic `import("./model-viewer.js")` on the first
  model open, so nothing 3D is parsed at startup. Dynamic import of a
  same-origin module is allowed by `script-src 'self'`.

### `ui/model-viewer.js` (DOM/WebGL wiring) + `ui/model.js` (pure)

- **One shared viewer instance** per window (scene, camera, `WebGLRenderer`,
  `OrbitControls`), created lazily, reused across all model tabs — the same
  pattern as the single shared CodeMirror instance. Switching to a model tab
  attaches the viewer's canvas into `#preview` (class `model-view`, not
  `markdown-body`); switching away detaches it and swaps out the mesh, but the
  renderer and its single WebGL context live on. This is why `retainable` is
  false and why context leaks can't happen.
- **Render on demand**: draw only on controls `change`, resize
  (`ResizeObserver` on the preview pane), theme change, and geometry load — no
  continuous `requestAnimationFrame` loop (battery).
- **Scene**: neutral matte material (`MeshStandardMaterial`, light grey; slightly
  lighter in dark theme), a hemisphere light plus a directional light attached
  to the camera so the visible side is always lit, a subtle grid "build plate"
  under the model's footprint, and background matching the app theme
  (`data-theme`), updated on theme toggle.
- **Orientation**: printing formats are **Z-up**; three.js is Y-up. Rotate the
  loaded object −90° about X so the model sits on the plate as it would in
  Bambu Studio. This applies to **all three formats including OBJ** (our call:
  the files come from Claude's Z-up print pipelines; a Y-up OBJ from a
  general 3D tool will lie on its side, which is acceptable). Default camera: an isometric-ish three-quarter view from the
  front-right, framed to the bounding sphere.
- **Multi-object files** (3MF with several build items, OBJ with groups): show
  all of them together in one group, framed together.
- **Units**: STL and OBJ are unitless; label them as mm (3D-printing
  convention). 3MF declares a `unit` on `<model>`; the builder should confirm
  whether three's `3MFLoader` applies it, and if not, scale to mm so the
  readout is right.
- **Dimensions overlay**: bottom-left, small, `120.0 × 45.5 × 30.0 mm` from the
  world-space bounding box after the Z-up rotation (so X/Y/Z match the slicer's
  axes). Plus a **Reset view** button top-right of the canvas.
- **Errors**: parse failure, empty geometry, or a `read_model` refusal render an
  inline message in `#preview` ("Can't display this model: <reason>"), styled
  like the existing `.image-error`. Never a blank canvas.

Pure helpers in `ui/model.js` with `ui/model.test.js` (`node --test`):
- `modelFormat(path)` → `"stl" | "3mf" | "obj" | null`
- `formatDimensions(min, max, unit)` → `"120.0 × 45.5 × 30.0 mm"` (rounding
  rules, tiny/huge values, negative coordinates)
- `frameCamera(boundingSphere, fovDeg, aspect)` → camera distance/position and
  near/far planes for a given bounds (zero-size and single-point bounds don't
  produce NaN)
- `keepCamera(prevPath, nextPath, reason)` → whether a load preserves the
  current camera (same path + live reload → keep; tab switch → restore that
  tab's saved camera or frame fresh; new file → frame fresh)

`filetype.test.js` gains cases for the new kind and its capability row.

### Theme toggle

`applyTheme` clears the DOM cache and calls `renderActive`. For a model tab
that must **not** re-read the file or move the camera: the viewer subscribes to
theme changes itself (or `renderModel` receives a "theme-only" reason) and just
recolors background/material/grid and redraws. The plan picks the mechanism;
the requirement is no `read_model` call and no camera change on theme toggle.

### Open File dialog

Add a "3D models" filter (`stl`, `3mf`, `obj`) next to the existing "Markdown"
filter in `menu.rs`'s Open File dialog (our call).

### Tab state

One new **ephemeral** per-tab field: `modelCamera` (`{ position, target }` or
null) — saved when leaving a model tab, restored when returning, so each model
tab keeps its own view. Like `scrollTop` and the Review Mode fields it is never
serialized in session restore; restored model tabs open framed fresh.

### Live reload

The watcher already watches the active file's parent dir and emits
`file-changed`. For a model tab the existing `renderActive` path calls
`renderModel` → `read_model` again; `keepCamera` says keep, so only the mesh is
swapped. Debounce is the watcher's existing 200 ms, which also absorbs
OpenSCAD/trimesh writing a file in several chunks; if a read lands on a
half-written file and parsing fails, show the error — the next `file-changed`
repaints it. An in-flight load must be ignored if the active tab/path changed
before it resolved (a sequence number, like `paintSeq`).

Background model tabs need nothing special: they aren't retained, so returning
to one always re-reads the file.

### MCP

`mcp::viewable_path` gains a `MODEL_EXTS` list (`stl`, `3mf`, `obj`), mirroring
`MODEL_EXT` in `filetype.js`; update the `open_document` tool's `path`
description and the `validate` error message to mention 3D models, plus the
mirror tests in `mcp.rs`. `request_review` and `generate_pdf` stay markdown-only.
`get_viewer_state` needs no change. The `line` argument is meaningless for
models and is ignored.

## Cross-platform

Nothing is `cfg`-gated. WebGL is available in WKWebView (macOS) and WebView2
(Windows). Windows should get one manual smoke check (open each format, rotate,
live-reload) before release since nobody has observed WebGL in our WebView2
build yet.

## Security notes (plan a `security-reviewer` pass)

- Model files are untrusted input. Parsing happens in JS loaders on bytes; no
  HTML is generated from file contents. Any text from the file that reaches the
  UI (error messages, 3MF metadata) goes through `textContent`, never
  `innerHTML`.
- `read_model` is the new attack surface: extension allowlist, size cap, 3MF
  declared-size cap.
- No CSP change. No new open targets (`UNSAFE_OPEN_EXTS` untouched).

## Docs / housekeeping

- CLAUDE.md: file-layout lines for `model.rs`, `ui/model.js`,
  `ui/model-viewer.js`, `ui/three/`; an architecture bullet "3D models"; note
  the import-specifier rewrite and the CSP reason for `read_model` under
  "Things that took hours" if they bite.
- README feature list + CHANGELOG entry at release time.
- Frontend edits need `cargo build` to show up (Tauri bundles `ui/` at compile
  time).
- Test fixtures under `src-tauri/tests/fixtures/models/`: a small binary STL, an
  ASCII STL, a two-object plain 3MF, an OBJ, and a minimal Bambu-style 3MF
  (with a `3D/Objects/` entry) for the refusal path.

## Left to the builder

- Exact three.js version, light intensities, colors, grid size/spacing.
- Whether double-click-to-reset is worth keeping if it conflicts with
  OrbitControls.
- Exact cap values (200 MB / 512 MB are proposals).
- Overlay styling, within the existing CSS variable palette.
