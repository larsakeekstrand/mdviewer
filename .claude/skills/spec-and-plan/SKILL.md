---
name: spec-and-plan
description: Start a new MDViewer feature the way this repo always does it — a design spec then an implementation plan, in docs/superpowers/, driven by the superpowers brainstorming + writing-plans flow with mdviewer's conventions and gotchas injected. Use when the user wants to spec, design, or plan a new feature before writing code.
---

# Spec-and-plan an MDViewer feature

This repo has a consistent front-half-of-every-feature ritual: a **design spec**
followed by an **implementation plan**, both committed under `docs/superpowers/`
before any code is written (28+ feature pairs in the history). This skill drives
that flow — it does **not** reinvent ideation or planning; it routes through the
superpowers skills and layers on this project's file conventions and hard-won
constraints.

## The flow

1. **Brainstorm the design** — invoke `superpowers:brainstorming` to explore
   intent and requirements with the user. Don't skip to a plan; the spec is
   where the design decisions and trade-offs get pinned down.

2. **Write the design spec** to:
   ```
   docs/superpowers/specs/<YYYY-MM-DD>-<slug>-design.md
   ```
   Use today's date and a short kebab-case `<slug>` naming the feature (match the
   style of existing files, e.g. `2026-06-09-review-mode-design.md`).

3. **Write the implementation plan** — invoke `superpowers:writing-plans` — to:
   ```
   docs/superpowers/plans/<YYYY-MM-DD>-<slug>.md
   ```
   (same date + slug, no `-design`). Decompose into TDD-ordered tasks; the
   history labels these like "implementation plan: X (N tasks, TDD)".

4. **Commit** the spec (and later the plan) on a feature branch — don't author
   them on `main`. Subjects mirror the repo: `Add design spec for <feature>` /
   `Add implementation plan for <feature>`.

## mdviewer constraints the spec and plan MUST account for

Inject these into the design so the plan doesn't rediscover them mid-implementation
(all are from CLAUDE.md):

- **Pure helpers are unit-tested.** Factor logic into pure functions —
  `ui/*.js` helper modules with a sibling `*.test.js` (run via `node --test`),
  and Rust modules with `#[cfg(test)]`. DOM/IPC wiring stays thin. Almost every
  existing module follows this; a plan that puts logic in untestable wiring is
  off-pattern.
- **The render seam is `postRender()`** — new post-render frontend behavior
  (annotations, link/image handling, math/mermaid-like passes) hooks there, in
  the documented order; height-changing passes run before `restoreAnchor`.
- **Tab model is the state spine** — per-tab fields on `tabs[]` + `activeIdx`;
  ephemeral state (like Review Mode) is excluded from session restore. Say which
  bucket new state lives in.
- **Untrusted content** — markdown/files/repos are untrusted. New file/path/URL
  handling routes through `fs_ops::within_root`; new "open" targets respect
  `UNSAFE_OPEN_EXTS`; don't widen the CSP; render via escaping / DOMParser, not
  raw HTML strings. (A spec touching this area should plan a `security-reviewer`
  pass.)
- **Cross-platform parity** — macOS (.dmg, Apple-Event file opens, native PDF/
  CLI-install) vs Windows (NSIS, argv opens). macOS-only code is `cfg`-gated and
  the menu item hidden on Windows; the plan should name the Windows behavior.
- **New IPC commands** return `Result<T, String>` and are attack surface — list
  each one the feature adds.
- **Frontend edits need `cargo build`** (Tauri bundles `frontendDist` at compile
  time) — note this in any plan that touches `ui/*`.
- **Lint/test gate** — `cargo fmt --check`, `cargo clippy --all-targets -D
  warnings`, `cargo test`, and `node --test` for JS helpers must all pass; tasks
  should be checkable against these.
- **"Things that took hours"** — skim that CLAUDE.md section for the feature's
  area (comrak quirks, sourcepos anchoring, atomic write-via-rename, mermaid
  config, the Edit-submenu macOS trap, `[hidden]` vs flex) and pre-empt the
  relevant landmines in the spec.

## Output

End the spec phase by confirming the file path written and a one-paragraph
summary of the chosen design; end the plan phase with the task count and the
verification commands each task will be checked against. Hand back to the user
before implementing — execution is a separate step (`superpowers:executing-plans`
or `subagent-driven-development`).
