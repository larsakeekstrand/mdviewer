#!/usr/bin/env bash
# Pre-release launch smoke test: build the bundle, then launch it and round-trip
# get_viewer_state over the MCP socket. Set MDVIEWER_SMOKE_APP to a prebuilt
# .app to skip the (slow) bundle build.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${MDVIEWER_SMOKE_APP:-}" ]]; then
  echo "Building the app bundle..."
  # Only the .app is needed. A full `cargo tauri build` also signs the updater
  # artifact, which requires the release-only TAURI_SIGNING_PRIVATE_KEY and
  # fails on a dev machine; --bundles app + createUpdaterArtifacts:false skips
  # the dmg and the updater signing.
  ( cd src-tauri && cargo tauri build --bundles app \
      --config '{"bundle":{"createUpdaterArtifacts":false}}' )
fi

echo "Running the launch smoke test..."
( cd src-tauri && cargo test --test launch_smoke -- --ignored --nocapture )
