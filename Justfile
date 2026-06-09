run *ARGS:
    cargo run -- {{ARGS}}

test:
    cargo test

release:
    cargo run --release

# ── Web preview ────────────────────────────────────────────────────────────
# Run the localhost HTTP server + GUI.  The server scans `examples/` for
# `.ali` files at startup (rescanned on each request), so edits show up
# without restarting.  Pass an alternate directory as a positional arg:
#
#     just web some/other/dir
#     just web --bind 127.0.0.1:8080
web *ARGS:
    just web-js
    cargo run -- web {{ARGS}}

# Bundle the frontend JS (CodeMirror + app) with esbuild.
web-js:
    cd web/frontend && npm install --silent && npm run build

# Watch frontend JS for changes and rebuild automatically.
web-js-watch:
    cd web/frontend && npm install --silent && npm run watch

# Prepare a static WASM deployment under web/frontend/:
#   - bundle frontend JS with esbuild
#   - wasm-pack build into web/frontend/pkg/
#   - recursively mirror examples/ into web/frontend/examples/, preserving
#     the directory tree and generating an index.json manifest via the
#     shared script (same logic the GitHub Pages workflow uses)
# The resulting directory can be served as-is (GitHub Pages, `python3 -m
# http.server`, anything static).  Duplicate stems fail the build.
web-wasm:
    just web-js
    wasm-pack build --target web web/wasm --out-dir ../frontend/pkg
    python3 scripts/build_examples_manifest.py examples web/frontend/examples

# Serve web/frontend/ as static files on port 8000 so you can preview the
# WASM-backed build end-to-end.  Run `just web-wasm` first.
web-static port="8000":
    cd web/frontend && python3 -m http.server {{port}}

# ── Wiki (Quartz) ───────────────────────────────────────────────────────────
# Build the wiki to docs/quartz/public/.  Run once to produce static HTML.
wiki:
    cd docs/quartz && bun install --frozen-lockfile && bun run quartz build -d ../wiki

# Serve the wiki locally with hot-reload on http://localhost:8080.
# Frees ports 8080/3001 first, so a server orphaned by a previous run (closed
# terminal, hard kill) never blocks the new one — the usual "page won't load".
wiki-dev:
    #!/usr/bin/env bash
    set -euo pipefail
    for port in 8080 3001; do
        pids=$(lsof -ti tcp:$port 2>/dev/null || true)
        [ -n "$pids" ] && kill -9 $pids 2>/dev/null || true
    done
    cd docs/quartz
    bun install --frozen-lockfile
    exec bun run quartz build -d ../wiki --serve
