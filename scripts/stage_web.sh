#!/usr/bin/env bash
# Stage the static WASM site's runtime pieces into <dest>, which must be
# missing or empty: index.html, style.css, dist/app.js, the wasm pkg/ without
# its packaging files, and the mirrored examples/. Used by `just web-zip` and
# the Pages workflow, after the bundle, wasm-pack, and static_examples ran.
set -euo pipefail

dest="${1:?usage: stage_web.sh <dest>}"
frontend="$(cd "$(dirname "$0")/../web/frontend" && pwd)"

# cli/build.rs leaves this stub when it cannot bundle; never publish it.
if grep -q 'alifib frontend bundle missing' "$frontend/dist/app.js"; then
  echo "stage_web: dist/app.js is the build script's stub; install bun or Node.js and rebuild" >&2
  exit 1
fi
if [[ -e "$dest" && -n "$(ls -A "$dest")" ]]; then
  echo "stage_web: $dest is not empty" >&2
  exit 1
fi

mkdir -p "$dest/dist"
cp "$frontend/index.html" "$frontend/style.css" "$dest"/
cp "$frontend/dist/app.js" "$dest/dist"/
cp -r "$frontend/pkg" "$dest/pkg"
rm -f "$dest"/pkg/package.json "$dest"/pkg/.gitignore "$dest"/pkg/*.d.ts
cp -r "$frontend/examples" "$dest/examples"
