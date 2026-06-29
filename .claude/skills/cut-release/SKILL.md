---
name: cut-release
description: Cut a new MDViewer release end-to-end — update README + CHANGELOG, bump the version in both manifests, refresh Cargo.lock, commit, tag, push, and publish the draft Release. Use when the user wants to "cut a release", "ship vX.Y.Z", "release a beta/rc", or "publish the draft".
disable-model-invocation: true
---

# Cut an MDViewer release

This skill performs the ritualized release process documented in `CLAUDE.md`
("Cutting a release"). It has real side effects — it pushes a tag and publishes
a GitHub Release — so it is **user-invoked only**.

Work through the checklist in order. Create one todo per numbered step and do
not skip the verification step at the end.

## Before you start

Confirm the target version with the user (e.g. `1.23.0`, or a beta like
`1.23.0-rc.1`). Note whether it is a **beta/prerelease** (contains a `-`) — the
workflow branches on that and the steps differ slightly (see "Beta" below).

Capture the current version for the changelog diff:

```sh
grep '^version' src-tauri/Cargo.toml          # current X.Y.Z
git describe --tags --abbrev=0                # last published tag
git log --oneline "$(git describe --tags --abbrev=0)..HEAD"   # commits since
```

## Checklist

1. **Update `README.md`** to cover any user-facing features added since the last
   release (Features list, Usage, Menus) and fix any now-stale claims. The README
   is the user-facing source of truth and drifts silently — every release must
   leave it accurate. Review the commits-since-last-tag list to find what changed.

2. **Add a `CHANGELOG.md` section** at the top, immediately under the intro
   paragraph, in the exact existing format:
   ```
   ## [X.Y.Z] - YYYY-MM-DD

   <one-line summary of the release theme>

   ### Added / ### Changed / ### Fixed
   - **Bold lead-in.** User-facing bullet, no commit hashes, no internal/
     test/bump commits.
   ```
   Use today's date. This section is what the GitHub release page and the in-app
   "What's new" modal show; if it's missing the workflow falls back to the raw
   commit log.

3. **Bump the version in BOTH manifests** — they must match:
   - `src-tauri/Cargo.toml` → `version = "X.Y.Z"`
   - `src-tauri/tauri.conf.json` → `"version": "X.Y.Z"`

4. **Refresh the lockfile**: `cd src-tauri && cargo update -p mdviewer`.

5. **Sanity-check before committing** (see Verification below).

6. **Commit**: `Bump to X.Y.Z` (or a subject describing the headline user-facing
   change). No `Co-Authored-By` trailer — the user sets that globally.

7. **Tag and push**:
   ```sh
   git tag vX.Y.Z
   git push
   git push origin vX.Y.Z
   ```

8. **Watch the release build**, then publish:
   ```sh
   gh run watch <run-id>          # release.yml builds aarch64 + windows
   gh release view vX.Y.Z --json assets   # confirm .dmg/.app.tar.gz/.exe + latest.json
   gh release edit vX.Y.Z --draft=false    # GO-LIVE: this is what reaches existing installs
   ```
   Publishing the draft is the auto-update trigger. Until then nothing ships.

## Beta / prerelease (version contains `-`)

- The release workflow detects the `-` and publishes to the rolling **`beta`**
  release (prerelease, **non-draft**) instead of a draft — so there is no
  `--draft=false` step; beta-opted installs pick it up automatically once the
  run finishes.
- A prerelease gets curated notes only if `CHANGELOG.md` has a matching
  `## [X.Y.Z-rc.N]` section; otherwise it falls back to the commit log (fine for
  testers).
- Smoke-test the manifest: `gh release view beta --json assets`.
- The MSI target rejects non-numeric pre-release identifiers, so the Windows job
  builds NSIS-only for prereleases (handled in `release.yml` — don't "fix" the
  version to be numeric, that desyncs the per-platform `latest.json`).

## Verification (do not skip — step 5 and after step 8)

Before committing, confirm the version is consistent and the build is sound:

```sh
grep -n '"version"\|^version' src-tauri/tauri.conf.json src-tauri/Cargo.toml   # both X.Y.Z
grep -n "^## \[X.Y.Z\]" CHANGELOG.md                                            # section exists
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && for f in ui/*.test.js; do node --test "$f"; done                       # JS helper tests
```

If anything fails, stop and fix it before tagging — a bad tag is painful to
unwind. After publishing, re-run `gh release view vX.Y.Z --json assets` and
confirm `latest.json` is present (it's what the in-app updater reads).

Before publishing the release (after step 8's build, against the artifact you
are about to ship), run the launch smoke test — it boots the bundled app and
confirms the frontend responds:

```sh
./scripts/smoke-test.sh   # builds MDViewer.app, launches it, round-trips get_viewer_state
```

If it times out or fails, do not publish — the bundle does not boot.

## Don't

- Don't change the bundle identifier `com.mdviewer.app` (orphans all persisted
  state).
- Don't strip a `-rc.N` suffix to a numeric version to appease MSI.
- Don't include internal/test/bump commits in the CHANGELOG bullets.
