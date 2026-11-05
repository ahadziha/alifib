run *ARGS:
    cargo run -- {{ARGS}}

test:
    cargo test --workspace

release:
    cargo run --release

# ── Web preview ────────────────────────────────────────────────────────────
# Run the localhost HTTP server + GUI, rescanning the directory on each request.
# alifib-cli's build script bundles the frontend JS before compiling.
# Pass an alternate directory and optionally a bind address:
#
#     just web some/other/dir
#     just web examples 127.0.0.1:8080
web dir='examples' bind='127.0.0.1:8000':
    cargo run -p alifib-cli -- web '{{dir}}' --bind '{{bind}}'

# Bundle the frontend JS (CodeMirror + app) with esbuild, running the named
# package.json script ("build" by default, "watch" to rebuild on change).
# Prefers Bun if present, otherwise npm — both drive the same esbuild scripts.
# Installs from the lockfile as cli/build.rs does: frozen, and `npm ci` only
# when the installed tree is missing or older than package-lock.json.
web-js script="build":
    #!/usr/bin/env bash
    set -euo pipefail
    cd web/frontend
    if command -v bun >/dev/null 2>&1; then
        if [[ -f bun.lock ]]; then bun install --silent --frozen-lockfile; else bun install --silent; fi
        bun run {{script}}
    elif command -v npm >/dev/null 2>&1; then
        if [[ ! node_modules/.package-lock.json -nt package-lock.json ]]; then npm ci --silent; fi
        npm run {{script}}
    else
        echo "web-js: need bun or npm on PATH" >&2; exit 1
    fi

# Pure browser synchronization helpers (Node's built-in test runner).
web-test:
    node --test web/frontend/test/*.test.js

# Watch frontend JS for changes and rebuild automatically.
web-js-watch: (web-js "watch")

# Prepare a static WASM deployment under web/frontend/:
#   - wasm-pack build into web/frontend/pkg/
#   - compile alifib-cli, whose build script bundles the frontend JS
#   - recursively mirror examples/ into web/frontend/examples/, preserving
#     directory tree and writing the server's index.json via static_examples
#     (also used by the GitHub Pages workflow)
# The resulting directory can be served as-is (GitHub Pages, `python3 -m
# http.server`, anything static).
web-wasm:
    wasm-pack build --target web web/wasm --out-dir ../frontend/pkg
    cargo run -p alifib-cli --example static_examples -- examples web/frontend/examples

# Serve web/frontend/ as static files on port 8000 so you can preview the
# WASM-backed build end-to-end.  Run `just web-wasm` first.
web-static port="8000":
    cd web/frontend && python3 -m http.server {{port}}

# Package the static WASM build into a self-contained zip for drag-and-drop
# deployment (Cloudflare Pages, Netlify, any static host).  Builds via
# `web-wasm`, then stages only the five runtime pieces — index.html, style.css,
# dist/app.js, the wasm pkg/, and the examples/ tree — with the same script as
# the Pages workflow, leaving node_modules, frontend src, lockfiles, and
# wasm-pack's type stubs out of the archive.
web-zip out="alifib-web.zip": web-wasm
    #!/usr/bin/env bash
    set -euo pipefail
    stage=$(mktemp -d)
    trap 'rm -rf "$stage"' EXIT
    bash scripts/stage_web.sh "$stage"
    out=$(cd "$(dirname "{{out}}")" && pwd)/$(basename "{{out}}")
    rm -f "$out"
    (cd "$stage" && zip -qr "$out" .)
    echo "wrote $out ($(du -h "$out" | cut -f1)) — drag it into Cloudflare Pages"
