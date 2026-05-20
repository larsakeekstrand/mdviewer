# Mermaid diagram rendering — design

**Date:** 2026-05-20
**Status:** Approved, ready for planning

## Problem

` ```mermaid ` fenced code blocks currently flow through comrak + syntect like
any other code block. Since syntect doesn't know the `mermaid` language, they
render as a plain, poorly-highlighted block of source text — not a diagram. We
want them rendered as actual diagrams (flowcharts, sequence diagrams, etc.).

## Goal & scope

- Render ` ```mermaid ` fences as live SVG diagrams in the rendered preview.
- Static, inline diagrams: scaled to the preview width, following the app's
  light/dark theme. **No** zoom/pan (possible future follow-up).
- Keep everything offline (the app is a local file viewer).
- Don't regress existing code-fence syntax highlighting, live reload, scroll
  anchoring, or the raw/rendered toggle.

Out of scope: zoom/pan/interactivity, server-side SVG pre-rendering, exporting
diagrams, configurable mermaid themes beyond light/dark.

## Approach (chosen)

Backend adapter override + vendored `mermaid.min.js` rendered on the frontend.

Rejected alternatives:
- **Pure frontend, no Rust change** — detection/source-extraction would depend
  on how syntect marks up an *unknown* language, which can silently break across
  comrak/syntect versions.
- **Server-side pre-render to SVG** — mermaid is JS-only; would require an
  embedded JS engine or headless browser, defeating the lean pure-Rust backend.

## Data flow

No change to the IPC shape. `render_file` still returns an HTML string; only the
markup for mermaid fences changes.

```
file → render_file → comrak (mermaid fences → <pre class="mermaid">) → HTML
     → morphdom into #preview → renderMermaid() → SVG
```

## Backend — `src-tauri/src/markdown.rs`

Add a thin `SyntaxHighlighterAdapter` wrapper around the existing
`SyntectAdapter`:

- When `lang == Some("mermaid")`: emit `<pre class="mermaid"><code>{escaped
  source}</code></pre>` and skip syntect. The `<code>` wrapper keeps the HTML
  valid (comrak appends `</code></pre>` itself); the `<pre>`'s `textContent` is
  the exact diagram source mermaid needs. Source is HTML-escaped on output
  (`&`, `<`, `>`); the browser decodes it back for `textContent`.
- For any other language: delegate every trait method to the inner
  `SyntectAdapter`, so Rust/JS/etc. highlight exactly as today.

The wrapper is constructed per-render in `render_markdown`, referencing the
existing static `LIGHT_ADAPTER` / `DARK_ADAPTER` (chosen by theme), and passed
as `plugins.render.codefence_syntax_highlighter`.

`opts.render.r#unsafe` stays `false` and `opts.render.sourcepos` stays `true`.
Consequence: `pre.mermaid` carries no `data-sourcepos`, so the scroll-anchor
logic skips diagrams. Acceptable — surrounding headings/paragraphs still anchor.

comrak 0.52 `SyntaxHighlighterAdapter` trait methods to implement/delegate:
`write_highlighted`, `write_pre_tag`, `write_code_tag`. Exact signatures to be
confirmed against the pinned crate during implementation.

## Frontend

### `ui/index.html`
Vendor `ui/mermaid.min.js` (UMD build that sets `window.mermaid`), loaded as a
classic `<script>` **before** the `app.js` module — same pattern as
`morphdom-umd.min.js`. Module scripts are deferred, so `window.mermaid` exists
by the time `app.js` runs.

Pin a specific mermaid 11.x release. The exact version and source URL are
recorded here during implementation and in a one-line comment near the script
tag:

- mermaid version: `<pinned during implementation>`
- source: `<URL pinned during implementation>`

### `ui/app.js`
- **Init once** (in `init()`):
  `mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme })`
  where `theme` maps `light → "default"`, `dark → "dark"`. `strict` because
  markdown may be untrusted (disables click-bound JS, sanitizes labels).
- **`renderMermaid({ force })`**, called from `renderActive` after morphdom +
  `annotateLinks`, only when `!result.raw`:
  - For each `pre.mermaid` in `#preview`: read source from the block, and if it
    is already rendered with unchanged source and `!force`, skip it. Otherwise
    `await mermaid.render(uniqueId, src)` and set the node's `innerHTML` to the
    returned SVG. Mark the node rendered and stash its source (e.g.
    `data-mv-state="ok"` + `data-mermaid-src`).
  - On render error: set the node to an inline error fallback (short message +
    original source as a code block), mark `data-mv-state="err"`. mermaid's
    default error "bomb" is suppressed.
- **Live reload:** extend morphdom's `onBeforeElUpdated` so a `.mermaid` node
  that is already rendered and whose incoming source matches the current source
  is preserved (return `false`) — no flicker when editing nearby prose. When the
  source differs (or `force`), let it update so `renderMermaid` re-renders it.
- **Theme switch:** the existing `prefers-color-scheme` `change` listener
  re-inits the mermaid theme and calls `renderActive` with `force` so all
  diagrams re-render in the new theme.
- Thread a `forceMermaid` flag through `renderActive` into both the morphdom
  hook and `renderMermaid`.

### `ui/styles.css`
- `pre.mermaid`: remove the gray code-block background/border/padding, center
  contents. Scope with enough specificity to beat `github-markdown.css`'s `pre`
  rules (e.g. `.markdown-body pre.mermaid`).
- `pre.mermaid svg`: `max-width: 100%; height: auto;`.
- Hide source until rendered/errored to avoid a flash of raw mermaid text:
  `pre.mermaid:not([data-mv-state]) { visibility: hidden; }`.
- Error-fallback styling for the inline error message + source.

## Error handling

A diagram with a syntax error must not break the page. The offending block shows
a small inline error message plus its original source as a code block so the user
can see and fix it. All other blocks render normally.

## Testing

- **Rust unit tests** (`markdown.rs`, run in CI):
  - A `mermaid` fence emits `<pre class="mermaid">` containing the escaped
    source and **no** syntect markup / `language-` class.
  - A normal ` ```rust ` fence is still syntect-highlighted.
- **Manual** (frontend is vanilla JS, no test harness): `cargo run` against a
  sample `.md` containing a couple of diagram types, one deliberately broken
  (error fallback), and a normal code block (still highlighted). Verify:
  - live reload: editing a diagram's source updates it; editing nearby prose
    does **not** flicker the diagram, and scroll position holds.
  - light/dark: toggling OS appearance re-themes diagrams.

Reminder: frontend changes require `cargo build` (Tauri embeds `frontendDist` at
compile time).

## Notes / risks

- The embedded frontend bundle grows by mermaid's size (~2–3 MB). Acceptable for
  a desktop app; keeps everything offline.
- New vendored dependency to update occasionally.
- `"csp": null` in `tauri.conf.json`, so mermaid's injected `<style>`/SVG are not
  blocked by any Content-Security-Policy.
```
