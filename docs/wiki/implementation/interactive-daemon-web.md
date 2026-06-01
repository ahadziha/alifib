---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/interactive/daemon.rs, src/interactive/protocol.rs, src/interactive/web.rs]
---

# interactive-daemon-web — the rewrite engine over the wire

> One engine, two mouths. `daemon` speaks JSON-lines over stdin/stdout for
> editors; `web` is a stateful adapter the browser frontends call as a library.
> Both are thin transport skins over the *same* command surface,
> `RewriteEngine::handle`; `protocol` is the shared vocabulary of requests and
> response payloads they trade in. None of these modules touches the
> mathematics — they only carry a [[rewriting]] session across a boundary.

The [[interactive-engine]] holds the live session and does the actual stepping.
This page documents how a session is *reached from outside the process*: the
wire format (`protocol`), the line-oriented subprocess server (`daemon`), and
the in-library browser adapter (`web`).

## What each module owns

| Module | Responsibility |
|---|---|
| `protocol.rs` | the wire vocabulary: `Request` (a `#[serde(tag="command")]` enum), the `Response` envelope, the `ResponseData` snapshot and all its sub-structs, plus the *builders* that turn an engine into those structs |
| `daemon.rs` | `run_daemon` — the read-line / dispatch / write-line loop for `alifib serve`; owns only the session-lifecycle commands |
| `web.rs` | `WebRepl` — a `State` machine (`Empty`→`Loaded`→`Active`) the HTTP server, WASM shim, and MCP server drive as a library; owns source loading, session setup, and string-diagram queries |

## Key public types

- `Request` (`protocol.rs`) — externally-tagged on `command` (snake_case). Spans
  the whole interactive vocabulary: session lifecycle (`Start`, `Resume`,
  `Shutdown`), stepping (`Step`, `StepMulti`, `Auto`, `Random`), navigation
  (`Undo`, `UndoTo`, `Redo`, `RedoTo`), inspection (`Show`, `Proof`, `History`,
  `ListRules`, `Types`, `TypeInfo` — wire name `type` — `Cell`), mutation
  (`Store`, `Parallel`, `SetTarget`), and `Homology`. `Start` carries
  `#[serde(alias = "source_diagram", alias = "initial_diagram")]` on `initial`
  and `#[serde(alias = "target_diagram")]` on `target` for backward compat. There
  is no `save`/`load`: the durable session form is the proof term, fetched with
  `Proof` (`engine.proof_expr`) and re-ingested by `Resume`.
- `Response` (`protocol.rs`) — tagged on `status`: `Ok { data }` or
  `Error { message }`. `Response::error` is the one-line constructor.
- `ResponseData` (`protocol.rs`) — the snapshot every successful command returns.
  A fat struct where most fields are `skip_serializing_if`-elided so each command
  populates only what it touches (`rules` for `list_rules`, `types` for `types`,
  `type_detail` for `type`, `cell_detail` for `cell`, `auto` for `auto`/`random`,
  `stored` for `store`). The always-present core is `current`/`initial`
  (`DiagramInfo`), `step_count`, `can_redo`, `rewrites`, `target_reached`,
  `parallel`, `backward`. `target` (`DiagramInfo`) and `proof` (`ProofInfo` —
  the running proof's `dim`, `step_count`, input/output labels) are also set by
  `build_response`, but both are `skip_serializing_if`-elided: `target` when no
  goal was given, `proof` when no step has been taken.
- `DiagramInfo` / `DimSlice` (`protocol.rs`) — a rendered diagram: flat `label`,
  `dim`, top-cell `cell_count`, and a per-dimension `cells_by_dim` breakdown of
  resolved generator names. Built by `diagram_info`.
- `RewriteInfo` / `FamilyMember` (`protocol.rs`) — one available move: the
  rule(s), the resulting `input`/`output` boundaries, `match_positions`, and a
  `match_display` (current diagram with the matched cells bracketed). A parallel
  family fills `family`; a singleton leaves it empty. Built by
  `build_rewrite_info_from_family` (*internal*).
- `WebRepl` (`web.rs`) — the browser-facing handle, wrapping a private
  `State` enum (`web::State`, *internal*).
- `Diagnostic` (`src/language/error.rs`) — *not defined here* but the web layer's
  reason for existing as a distinct skin: on a parse/interpret failure
  `load_source` emits structured `Diagnostic`s (`kind`, `message`, `start`/`end`
  `Position`, pre-rendered `snippet` with caret underline) so an editor can
  highlight the offending span. The daemon has no analogue — it only ever started
  from an already-loaded engine.

## Data flow — a daemon session

```
alifib serve ──cli::run_serve_cmd──▶ run_daemon(initial)
                                          │
  stdin line ──serde_json::from_str──▶ Request
                                          │
                                      dispatch(&mut engine, req)
                                          │
            ┌─────────────────────────────┴──────────────────────────────┐
   Start/Resume/Shutdown                                  everything else
   (daemon's own layer)                          with_engine ─▶ engine.handle(&req)
   build (start) or                                       │  Some(Ok|Err)
   decompose (resume) via install                         ▼
            └──────────────▶ Response::{Ok,Error} ──serde_json──▶ stdout line
```

1. `run_daemon` optionally emits an initial `Show`-shaped response if pre-loaded
   from CLI args, then loops over stdin lines (blank lines skipped).
2. Each line is parsed to a `Request`; a parse failure becomes a single
   `Response::error("invalid request: …")` and the loop continues — a bad line
   never kills the session.
3. `dispatch` handles `Start`/`Resume`/`Shutdown` itself — `Start`/`Resume` build
   or decompose the `RewriteEngine` through the shared `install` helper (construct,
   then swap into the active slot), `Shutdown` exits; `Homology` is explicitly
   refused; **everything else delegates to `engine.handle`** via `with_engine`,
   which first checks a session exists.
4. `emit` serialises and `writeln!`s one JSON line, then flushes. A serialisation
   error falls back to a hand-written error line.

## Data flow — a web session

`WebRepl` is a library object, not a server; the HTTP/WASM/MCP crates own the
transport and call methods on it.

1. `WebRepl::new()` → `State::Empty`.
2. `load_source(text)` (or `load_source_with_modules`) parses and interprets
   `.ali` source into a `GlobalStore`, moving to `State::Loaded`. On failure it
   returns a JSON error carrying `diagnostics: [Diagnostic]`.
3. `start_session(type, initial, target?, backward)` (or `resume_session(type,
   proof, target?, backward)` for a proof diagram) builds a `RewriteEngine`
   through the shared `open_session` helper and moves to `State::Active`. A
   *failed* setup collapses back to `Loaded`, not `Empty`, so the caller keeps the
   store it loaded.
4. `run_command(json)` parses a `Request` and dispatches:
   - `Start`/`Resume`/`Shutdown` are refused ("command not supported in web
     mode") — the engine's birth and death are driven by
     `start_session`/`resume_session`/`reset`, not the wire.
   - `Homology` is served from the `GlobalStore` directly, with no session.
   - `Types`/`TypeInfo` work even in `Loaded` (no engine) via the
     `*_from_store` builders.
   - everything else requires `State::Active` and is forwarded to
     `engine.handle` — the *same* call the daemon makes.
5. String-diagram queries (`get_strdiag`, `get_session_strdiag`,
   `get_proof_strdiag`, …) are web-only side channels returning
   `analysis::strdiag::StrDiag` JSON for rendering; they have no daemon
   counterpart.

## Non-obvious invariants and gotchas

- **`engine.handle` is the single shared command surface.** Both
  `daemon::dispatch` and `WebRepl::run_command` funnel non-lifecycle commands
  through it, so the two transports can never drift in *which* commands do
  *what*. `handle` returns `None` exactly for the session-transition variants plus
  `Homology` (`Request::{Start,Resume,Shutdown,Homology}`); both callers
  match those beforehand, so the `None` arm is unreachable and both treat it as
  an internal error. Keep that set in sync if a command moves layers.
- **The daemon refuses `Homology`; the web layer serves it.** Homology needs only
  a `GlobalStore`, not a live engine, and the daemon was never wired to query the
  store outside a session — so `daemon::dispatch` returns an explicit
  "not supported in daemon mode" while `WebRepl` answers it from `state.store()`.
- **`Diagnostic` is the web layer's whole reason to differ.** It lives in
  `language::error`, not here; the daemon assumes an engine that already loaded
  cleanly (CLI handled errors up front), whereas the web `load_source` path turns
  every `LoadFileError::Parse` / `InterpError` into structured `Diagnostic`s
  (`error.rs::to_diagnostic`) the editor renders inline.
- **`ResponseData` is one struct for every command.** Optional fields are
  `skip_serializing_if`-elided rather than modelled as separate response types;
  a command builder calls `build_response(engine, …)` then sets its one field
  (`build_list_rules_response`, `build_types_response`, etc.). Don't expect a
  command's extra payload to appear unless that builder ran.
- **`include_history` gates the `history` field.** Only `Request::History` calls
  `build_response(self, true)`; every other path passes `false`, so `history` is
  empty (and elided) elsewhere — saving the per-response cost of walking the
  step history.
- **`State` forbids `store=None, engine=Some`.** The enum makes the illegal
  "engine without a store" shape unrepresentable, replacing the old
  two-`Option` convention. `stop_session` demotes `Active`→`Loaded` preserving
  the store; `reset` wipes to `Empty`.
- **WASM memory discipline.** `load_source_with_modules` sets `State::Empty`
  *before* allocating the new store, because in WASM peak linear-memory pages are
  never returned; the two stores must not coexist.
- **`Store` must re-sync the adapter's store handle.** After a successful
  `Request::Store`, `WebRepl` does `*store = engine.store_arc()` — `handle`
  mutates the engine's store via `Arc::make_mut`, so the cached handle would
  otherwise miss the new let-binding on subsequent `types`/`type` queries.
- **Three consumers, not two.** The module docs (`web.rs` doc-comment) say
  "both web backends", but `WebRepl` is used by `web/server`, `web/wasm`, *and*
  `web/mcp` (each `use alifib::interactive::web::WebRepl`).
  `web/server/tests/bundled_modules.rs` exercises the load-then-query path as
  behavioural evidence. The three crates themselves are documented in
  [[web-backends]].

## Mathematics

These three modules carry no mathematics of their own — they are transport and
serialisation. Their bridge to [[rewriting]] is a **support relationship**, not a
realisation: the actual matching, pushout, and step construction live in
[[core-matching]], and the session that sequences steps lives in
[[interactive-engine]] (`RewriteEngine`). `protocol`'s `ResponseData` merely
*describes* the state of a rewriting session — `current`/`target`
[[diagram|diagrams]], available `RewriteInfo` moves (each a candidate
[[rewriting|rewrite]] with its matched cell positions and resulting
[[boundary|boundaries]]), the running proof — for a client to render. `daemon`
and `web` only move those descriptions across a process or language boundary. See
[[interactive-repl]] for the in-process terminal front-end built on the same
engine, [[web-backends]] for the `web/server`, `web/wasm`, and `web/mcp` crates
that drive `WebRepl`, and [[output]] for the name-keyed render tree the labels
come from.
