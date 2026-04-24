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
    cargo run -- web {{ARGS}}

# Prepare a static WASM deployment under web/frontend/:
#   - wasm-pack build into web/frontend/pkg/
#   - mirror examples/ into web/frontend/examples/
#   - write examples/index.json for the frontend's dropdown
# The resulting directory can be served as-is (GitHub Pages, `python3 -m
# http.server`, anything static).
web-wasm:
    wasm-pack build --target web web/wasm --out-dir ../frontend/pkg
    rm -rf web/frontend/examples
    mkdir -p web/frontend/examples
    cp examples/*.ali web/frontend/examples/
    python3 -c "import json, os, sys; names = sorted(os.path.splitext(f)[0] for f in os.listdir('web/frontend/examples') if f.endswith('.ali')); open('web/frontend/examples/index.json','w').write(json.dumps(names))"

# Serve web/frontend/ as static files on port 8000 so you can preview the
# WASM-backed build end-to-end.  Run `just web-wasm` first.
web-static port="8000":
    cd web/frontend && python3 -m http.server {{port}}
