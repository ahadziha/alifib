---
kind: impl
status: stable
last-touched: 2026-06-01
code: [web/shared/src/lib.rs, web/server/src/lib.rs, web/wasm/src/lib.rs, web/mcp/src/lib.rs]
---

# web-backends — deployment wrappers around `WebRepl`

> Three transports, one kernel. `web/server` (HTTP), `web/wasm` (browser), and
> `web/mcp` (LLM agents) each own a wire format and a process model, but all
> three hold a single `alifib::interactive::web::WebRepl` and forward to its
> methods verbatim. `web/shared` is the one thing they genuinely share: a
> runtime scanner for the `.ali` examples directory. None of these crates
> touches the mathematics — they re-export a [[rewriting]] session, unchanged,
> across a network/language/agent boundary.

This page documents the **deployment-wrapper crates** in the `web/` workspace.
The in-library layer they all wrap — `WebRepl`, the protocol, the daemon — is
documented in [[interactive-daemon-web]]; do not look here for command
semantics, only for transport and packaging.

## What each crate owns

| Crate | Kind | Role |
|---|---|---|
| `web/shared` (`alifib-web-shared`) | lib | `ExampleSet` — recursively scans an on-disk `.ali` examples tree, names each file by its `.ali`-stripped POSIX-relative path, and serves it as text or as an `index.json` map. Path-traversal-safe (`read_path`). Depends on `serde_json` *only* — not on `alifib`. |
| `web/server` (`alifib-web-server`) | lib | `run_web_server` — a hand-rolled localhost HTTP/1.1 server (raw `TcpListener`, no framework) over one long-lived `WebRepl`, plus a `build.rs` that bundles the browser frontend into the binary. |
| `web/wasm` (`alifib-wasm`) | cdylib | `WasmRepl` — a `#[wasm_bindgen]` struct that is a thin pass-through over `WebRepl`, exposing the same surface to in-browser JavaScript with no server. |
| `web/mcp` (`alifib-web-mcp`) | lib | `run_mcp_server` / `serve` — a Model Context Protocol server (newline-delimited JSON-RPC 2.0 over stdio) mapping a fixed tool set 1:1 onto `WebRepl` methods. |

`web/{shared,server,mcp}` are root-workspace members; `cli` wires them as
`alifib web` and `alifib mcp` (`cli::run_web_cmd` / `run_mcp_cmd`, examples dir
defaulting to `./examples`). `web/wasm` is **deliberately not a workspace
member** — its `Cargo.toml` carries its own empty `[workspace]` table so
`cargo build/test` at the root ignores it; it is built out-of-band with
`wasm-pack build --target web web/wasm`.

## `web/shared` — the examples directory

`ExampleSet::new(dir)` wraps a root path; everything else re-scans the tree on
demand, so the running server picks up filesystem edits without a restart
(`recursive_scan_with_subdirs`). An entry's `name` is its relative path minus
`.ali` (`Theory`, `TRS/Aux`); the subdirectory prefix disambiguates same-stem
files (`duplicate_stems_in_different_dirs_allowed`). Every path segment must
match the language identifier rule `[A-Za-z_][A-Za-z0-9_]*` — anything else is
*skipped with a warning*, never an error, since it could not be `include`d
anyway (`invalid_segments_skipped_not_errored`). A missing root yields an empty
index, not a failure (`missing_root_is_empty_not_error`). `read_path` validates
each segment and canonicalises-in-root, rejecting `..` / absolute escapes
(`read_path_rejects_traversal`).

This crate carries **no `.ali` content compiled in** and no dependency on
`alifib`: it is a directory model, consumed by the server (HTTP example routes)
and the MCP server (auto-seeding, `list_examples`). WASM has no `ExampleSet` —
the browser supplies modules over HTTP instead.

## `web/server` — localhost HTTP kernel

`run_web_server(bind_addr, examples)` binds a `TcpListener`, constructs **one**
`WebRepl`, and serves connections sequentially. `read_request` parses just the
request line and `Content-Length` (no keep-alive — every response sets
`Connection: close`); `handle_connection` dispatches on `(method, path)`:

- **Assets** — `GET /`, `/app.js`, `/style.css` are served from compiled-in
  strings. `index_html()` rewrites the frontend's dev script tag to inject
  `window.ALIFIB_CONFIG = { backend: 'http', apiBase: '' }`, telling the same
  frontend to talk to this server rather than to an in-page WASM module.
- **`/api/*`** — each route deserialises a typed body struct (`LoadSourceBody`,
  `StartSessionBody`, …) and forwards to the matching `WebRepl` method, writing
  the returned JSON envelope back verbatim. `StartSessionBody` carries the same
  `serde(alias)` back-compat field names as the protocol's `Start`.
- **`/examples/index.json`** and `GET /examples/<rel>.ali` — backed by
  `ExampleSet::index_json` / `read_path`; a rejected path returns a bare 404
  rather than leaking the reason.

### `build.rs` bundles the **frontend**, not the stdlib

This is the easily-misread part. `web/server/build.rs` runs `npm ci`/`install`
+ `npm run build` in `web/frontend/` to produce `dist/app.js`, which `lib.rs`
pulls in with `include_str!("../../frontend/dist/app.js")` (alongside
`index.html` and `style.css`). So the binary self-contains the **browser GUI**
([[web-frontend]]).
It does **not** bake in any `.ali` example or standard-library module — those
are read at runtime from the on-disk `ExampleSet`. When Node.js is missing or
the build fails, `build.rs` emits a cargo warning and writes a stub `app.js` so
the crate still compiles (this is what lets `cargo test --workspace` pass on a
toolchain without Node). `find_npm` also falls back to the newest
`~/.nvm/versions/node` install since cargo's spawned shell lacks nvm's PATH.

The `bundled_modules.rs` test name is about *user-supplied modules*, not the
build script: `include_user_supplied_module_resolves` checks that a `Theory.ali`
passed in the `modules` map lets user source `include Theory` resolve through
`WebRepl::load_source_with_modules` — the browser's stand-in for the CLI's
filesystem module search.

## `web/wasm` — browser bindings

`WasmRepl { inner: WebRepl }` is a `#[wasm_bindgen]` newtype whose every method
delegates straight to `inner`: `load_source` (parsing an optional
`modules_json` string into the modules map), `start_session`, `resume_session`,
`run_command`, the `get_*_strdiag` family, `set_proof_view`, `reset`,
`stop_session`. There is no transport and no server — JS calls these methods
directly on the in-page module. The crate is `crate-type = ["cdylib"]`; the
WASM memory-discipline reasons it exists (peak linear-memory pages are never
returned, so the old store must be dropped before the new one is allocated)
live in `WebRepl::load_source_with_modules` and are covered in
[[interactive-daemon-web]].

## `web/mcp` — Model Context Protocol server

`run_mcp_server(examples)` locks stdin/stdout and calls `serve(reader, writer,
examples)`; `serve` is generic over `BufRead`/`Write` so `handshake.rs` can
drive it with in-memory `Cursor` pipes. The loop reads newline-delimited
JSON-RPC 2.0, logs to **stderr** (stdout is protocol-only), and dispatches by
`method`:

- `initialize` → `protocolVersion: "2024-11-05"`, `serverInfo.name:
  "alifib-mcp"`, `capabilities.tools: {}` (pinned by
  `initialize_returns_protocol_metadata`).
- `tools/list` → the eight `tool_descriptors`: `load_source`, `start_session`,
  `run_command`, `get_types`, `get_strdiag`, `get_session_strdiag`,
  `get_rewrite_preview_strdiag`, `list_examples` (pinned by
  `tools_list_advertises_expected_surface`).
- `tools/call` → `dispatch` routes to the matching `WebRepl` method and wraps
  the returned JSON envelope in a single `text` content block.
- `ping` → `{}`; unknown method → JSON-RPC `-32601`; missing method → `-32600`.

Two MCP-specific behaviours beyond the HTTP server:

- **`load_source` auto-seeds the examples dir.** `dispatch` scans `examples`
  and pushes every entry into the modules map as `<name>.ali → content` *before*
  applying any caller-supplied `modules` overrides, so an agent's `include
  <Name>` resolves without shipping content. Scan failure here is non-fatal —
  surfaced via `list_examples` instead (`examples_dir_auto_seeded_for_include`,
  `list_examples_sees_seeded_dir`).
- **`run_command` is sugar over the wire command.** If `command_json` is given
  it is forwarded raw; otherwise the whole `arguments` object (minus
  `command_json`) *is* the command body, re-serialised — so
  `{command:'step', choice:0}` becomes the daemon `Request`.

An error envelope (`"status":"error"`) is forwarded with `isError: true` so MCP
clients branch without re-parsing; an unknown tool likewise yields `isError`
(`unknown_tool_returns_iserror`). Notifications (no `id`) get no response — the
loop `continue`s, which is why the `tools/list` test sees two responses for
three messages.

## Non-obvious invariants and gotchas

- **`WebRepl` is the single shared spine.** Server, WASM, and MCP each `use
  alifib::interactive::web::WebRepl` and add only framing. Command semantics
  cannot drift between them, because none of them re-implement a command — see
  [[interactive-daemon-web]] for the `State` machine and the
  `Start`/`Resume`/`Shutdown`-refused contract they all inherit.
- **`build.rs` packages JS, not math.** The bundled artifact is the frontend
  GUI; standard-library / example `.ali` text is never compiled in. Do not
  conflate `bundled_modules.rs` (runtime `include` resolution) with the build
  script.
- **WASM is off the workspace.** Its empty `[workspace]` table keeps it out of
  root `cargo build/test`; it ships via `wasm-pack`. The other three are
  ordinary members and are exercised by `cargo test --workspace`.
- **`ExampleSet` is alifib-free.** `web/shared` depends on `serde_json` only,
  so the examples model can be reused (e.g. by a static deployment mirror)
  without pulling in the interpreter.
- **The HTTP server is single-threaded and localhost-only by intent** — a local
  notebook kernel, not a public service; it accepts connections one at a time
  and closes each.

## Mathematics

These crates carry **no mathematics**. They are packaging and transport: each
re-exports a live [[rewriting]] session — the matching, pushouts, and step
construction of [[core-matching]], sequenced by [[interactive-engine]] — to a
different kind of client. What crosses each boundary is the protocol's
description of that session: the current and target [[diagram|diagrams]],
candidate rewrite moves, and the [[string-diagram]] render data the
`get_*_strdiag` methods return for the frontend to draw. The realisation of
all of it lives one layer down, behind `WebRepl`; see [[interactive-daemon-web]]
for that bridge, and [[core-diagram]] for what a diagram is.
