# View standalone image files

**Date:** 2026-05-25
**Status:** Approved (design)

## Problem

MDViewer renders markdown and shows other files as escaped plain text. Opening
an actual image file (PNG, JPG, …) doesn't work at all: the backend
`render_file` does `std::fs::read_to_string`, which fails on binary data with a
UTF-8 error, so the user gets an error pane instead of the picture. The user
wants to view image files directly in the app.

## Key facts (from exploration)

- `render_file` (`commands.rs`) reads the file as a UTF-8 string and renders
  markdown or `render_plain` — unusable for binary images.
- The frontend already has the asset-protocol plumbing for images embedded in
  markdown: `convertFileSrc(path)` (from `window.__TAURI__.core`) and
  `localImageUrl`/`resolveImages`. `asset:` is already in the CSP `img-src` and
  the asset-protocol scope is `["**"]`, so any local image path can be loaded.
- `renderActive` is the single render entry point; the `file-changed` listener
  calls it for live reload; `renderTabBar` updates the Raw button face.
- `MD_EXT` is an inline regex in `app.js`. Pure helpers (e.g. `export.js`)
  live in small modules unit-tested with `node --test`.
- `runEditAction` dispatches `copy` / `copy-source` / `toggle-raw` / `find`;
  `actionCopySource` reads the file as text (would fail for images).
- `onExport`/`exportDocument` serialize `#preview` (text-oriented).
- `showTransientError` shows a self-clearing banner without wiping the preview;
  `showError` replaces the preview (used for hard failures).

## Decisions (from brainstorming)

1. **Frontend-only.** No Rust changes; the image path never calls `render_file`.
2. **Formats:** `png, jpg, jpeg, gif, webp, avif, bmp, ico, svg` (no HEIC —
   WKWebView won't reliably render it in `<img>`).
3. **Sizing: actual size.** Natural pixel dimensions; the existing scroll
   container provides horizontal + vertical scrollbars when the image exceeds
   the pane. Not constrained by the prose-width rules.
4. **Live reload** preserved via a per-path cache-bust token (`?v=N`).
5. **Raw button hidden** for image tabs; **Copy Source** and **Export** guarded
   (text-only) with a transient notice; **broken/unsupported** images show an
   inline message, not a wiped pane.
6. **Out of scope:** Finder file-association for images, a dedicated tree icon.

## Design

### File-type helper (`ui/filetype.js` + test)

New tiny pure module so the detection is unit-tested (mirrors `export.js`):

```js
export const IMAGE_EXT = /\.(png|jpe?g|gif|webp|avif|bmp|ico|svg)$/i;
export function isImagePath(path) {
  return IMAGE_EXT.test(path || "");
}
```

Unit tests cover positive extensions (case-insensitive), negatives (`.md`,
`.txt`, no extension, a path containing `png` but not as the extension), and
empty/nullish input.

### Render path (`ui/app.js`)

- Import `isImagePath` from `./filetype.js`.
- `renderActive`: after the `activeTab()` guard, if `isImagePath(t.path)`, call
  a new `renderImage(t, { scrollLock })` and return — before the `render_file`
  invoke.
- `renderImage(t)`:
  - `previewEmpty.hidden = true; preview.hidden = false;`
  - Set `preview.className = "image-view"` (drop `markdown-body`/`raw-body`).
  - Build an `<img>`: `src = convertFileSrc(t.path)` plus `?v=${version}` when
    `version > 0` (see live reload). Set `alt` to the basename.
  - `img.onerror`: replace the image with an inline
    `<div class="image-error">Can't display this image.</div>`.
  - `preview.replaceChildren(img)`.
  - Preserve scroll only when `scrollLock` (live reload) — capture/restore
    `previewScroll.scrollTop`/`scrollLeft` around the swap; otherwise reset to
    top-left. (No sourcepos anchoring; images have none.)
  - Does **not** run the markdown `postRender` pipeline.

### Live reload (`ui/app.js`)

- Module-level `const imageVersions = new Map();` (path → int).
- In the `file-changed` listener, when the changed path is the active image
  tab, `imageVersions.set(path, (imageVersions.get(path) || 0) + 1)` before
  calling `renderActive`. Theme toggles / unrelated re-renders don't bump it,
  so they reuse the cached image (no flicker).

### Toolbar + guards (`ui/app.js`)

- `renderTabBar`: when the active tab is an image, `themeBtn` stays; set
  `rawBtn.hidden = true`. For non-image tabs, `rawBtn.hidden = false` and the
  existing label/`aria-pressed` logic runs.
- `runEditAction`: for an active image tab, `copy-source` and `toggle-raw`
  short-circuit with `showTransientError("Not available for images.")`.
- `onExport`: if the active tab is an image, `showTransientError("Export is
  only available for text documents.")` and return.

### CSS (`ui/styles.css`)

```css
.image-view {
  padding: 20px;
}
.image-view img {
  display: block;
}
.image-view .image-error {
  color: var(--sidebar-muted);
  padding: 20px;
}
```

The image renders at intrinsic size; `.preview-scroll` (`overflow: auto`)
supplies both scrollbars. `#preview.image-view` is excluded from the
`.markdown-body` rules because it no longer carries that class.

## Trade-offs / risks

- **Cache-bust query on an `asset:` URL:** Tauri's asset protocol should ignore
  `?v=N` and serve the file; verified live. If a query ever breaks loading,
  fall back to recreating the `<img>` element.
- **Actual size** means very large images require scrolling — explicitly chosen.
- **SVG** is treated as an image (rendered via `<img>`), not as markup; that's
  the intended "view the file" behavior and keeps it sandboxed (an `<img>`
  can't run script).

## Testing

- `node --test ui/filetype.test.js` (and the full `ui/*.test.js` suite) pass.
- `cargo build` clean (no Rust changes, but the frontend bundles at build time).
- Manual: click a PNG/JPG/GIF/SVG in the tree → renders at actual size with
  scrollbars when large; Raw button hidden; theme toggle doesn't refetch;
  overwrite the image on disk → it live-reloads; Copy Source / Export on an
  image show the transient notice; a deleted/garbage file shows the inline
  error; markdown and plain-text files still render as before.

## Out of scope

- Finder default-app association for images.
- Tree icon/affordance for image files.
- Zoom / pan / fit-to-window controls (actual size only).
