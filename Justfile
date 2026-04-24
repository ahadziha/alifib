run *ARGS:
    cargo run -- {{ARGS}}

test:
    cargo test

release:
    cargo run --release

# ── Web preview ────────────────────────────────────────────────────────────
# Run the localhost HTTP server + GUI.  All frontend assets (index.html,
# app.js, style.css) and every bundled `.ali` example are embedded into the
# binary at compile time, so `just web` is enough — no separate build step.
# Pass extra args, e.g. `just web --bind 127.0.0.1:8080`.
web *ARGS:
    cargo run -- web {{ARGS}}

# Build the WebAssembly bundle for static deployment.  Output lands in
# web/frontend/pkg/, which is what app.js imports when ALIFIB_CONFIG.backend
# is 'wasm' (the default when the file is opened without a backing server).
# Requires `wasm-pack` in PATH (cargo install wasm-pack).
web-wasm:
    wasm-pack build --target web web/wasm --out-dir ../frontend/pkg

# Serve web/frontend/ as static files on port 8000 so you can preview the
# WASM-backed build end-to-end.  Run `just web-wasm` first.
web-static port="8000":
    cd web/frontend && python3 -m http.server {{port}}
