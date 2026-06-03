---
kind: impl
status: stable
last-touched: 2026-06-03
code: [src/interactive/daemon.rs, src/interactive/protocol.rs, src/interactive/web.rs]
---

# interactive-daemon-web — the session over the wire

> One session core, two non-terminal mouths. `daemon` speaks JSON-lines over
> stdin/stdout for editors; `web` is a stateful adapter the browser frontends
> call as a library. Both are thin transports over the *same* command surface —
> [`Session::apply`][[interactive-session]] — and `protocol` is the shared
> vocabulary of requests and response payloads they trade in. None of these
> modules touches the mathematics; they carry a [[rewriting|rewrite]] or
> [[hole|hole]]-filling session across a process or language boundary.

The [[interactive-session|`Session`]] holds the live state and performs every
command; the [[interactive-engine]] does the rewriting underneath it. This page
documents how a session is *reached from outside the terminal*: the wire format
(`protocol`), the line-oriented subprocess server (`daemon`), and the in-library
browser adapter (`web`).

## What each module owns

| Module | Responsibility |
|---|---|
| `protocol.rs` | the wire vocabulary: `Request` (a `#[serde(tag="command")]` enum), the `Response` envelope, the `ResponseData` snapshot and all its sub-structs, plus the *builders* that turn the store or an engine into those structs |
| `daemon.rs` | `run_daemon` — the read-line / dispatch / write-line loop for `alifib serve`; loads a `Session` and forwards to `Session::apply` |
| `web.rs` | `WebRepl` — a handle wrapping a single `Option<Session>` that the HTTP server, WASM shim, and MCP server drive as a library; owns source loading, session setup, command dispatch, and string-diagram queries |

## `Request` — the wire vocabulary

`Request` (`protocol.rs`) is externally-tagged on `command` (snake_case) and
spans the whole interactive vocabulary, mirroring the shared `Command` language
([[interactive-session]]): session lifecycle (`Start`, `Resume`, `Stop`,
`Backward`, `Load`, `Shutdown`), stepping (`Step`, `StepMulti`, `Auto`, `Random`),
navigation (`Undo`, `UndoTo`, `Redo`, `RedoTo`), inspection (`Show`, `Proof`,
`History`, `ListRules`, `Types`, `TypeInfo` — wire name `type` — `Cell`,
`Homology`), mutation (`Store`, `Parallel`, `SetTarget`, `Save`), and the
**hole-filling trio `Holes` / `Fill { index, backward }` / `Done`**, plus
`Help { web }`. `Start` carries `#[serde(alias)]`es on `initial`/`target`
(`source_diagram`, `initial_diagram`, `target_diagram`) for backward compat. The
durable session form is still the proof term — fetched with `Proof`
(`proof_expr`) and re-ingested by `Resume`; `Done` and `Save` additionally return
the edited `source` for the editor to persist.

`Session::apply` handles most of these; `Types`/`TypeInfo`/`Cell`/`Homology` are
read-only queries served from the store, and `Help`/`Load`/`Shutdown` are
front-end concerns — the daemon and web each peel those off before `apply`.

## `Response` and `ResponseData`

`Response` is tagged on `status`: `Ok { data }` or `Error { message }`
(`Response::error` is the one-liner). `ResponseData` is the single snapshot every
successful command returns — a fat struct whose optional fields are
`skip_serializing_if`-elided, so each command populates only what it touches. The
always-present scalars are `step_count`, `can_redo`, `target_reached`,
`parallel`, `backward`; the rewrite core is `current`/`initial`/`target`/
`rewrites`/`proof` (all elided when absent — e.g. `current` is absent for `holes`,
an idle session, or a 0-cell fill). The per-command extras:

| Field | Set by | Carries |
|---|---|---|
| `message` | almost every command | the canonical one-liner (`Applied …`, `Filled ?x with …`, `Stored …`) |
| `proof_expr` | `proof` | the running proof as a re-parseable expression |
| `history` / `rules` | `history` / `list_rules` | the move list / the dimension's rules |
| `types` / `type_detail` / `cell_detail` | `types` / `type` / `cell` | type summaries / full detail / cell detail |
| `auto` / `stored` | `auto`/`random` / `store` | the step-count summary / the appended `let` clause |
| `homology` | `homology` | `HomologyInfo` (groups + Euler characteristic) |
| `fill` | during any fill | `FillInfo` — the hole being built (type, map, domain, source, dim) |
| `holes` / `constraints` | `holes` | `HoleInfo` per open hole / `ConstraintInfo` per conditional pending assignment |
| `zero_cell` | during a 0-cell fill | `ZeroCellInfo` — candidate 0-cells and the current pick |
| `source` | `done` / `save` | the updated running source for the editor to write |
| `module` | `show`/`status` when idle | the loaded module's path |

`DiagramInfo` (flat `label`, `dim`, `cell_count`, per-dimension `cells_by_dim`),
`RewriteInfo`/`FamilyMember` (one move: rule(s), resulting `input`/`output`
boundaries, `match_positions`, a `[bracketed]` `match_display`), and the type-detail
sub-structs (`TypeDetailInfo`, `MapEntry` — whose `holes` field pre-renders a
map's open holes as `?name : in → out`) round it out. The builders that fill these
— `build_response`, `build_list_rules_response`, `build_types_from_store`,
`build_type_detail_from_store`, `build_cell_response`, `build_homology_data` — are
shared by all three front-ends.

## Data flow — a daemon session

```
alifib serve ──cli::run_serve_cmd──▶ run_daemon(initial: Option<Session>)
                                          │  (emit initial state if pre-loaded & active)
  stdin line ──serde_json::from_str──▶ Request
                                          │
                                      dispatch(&mut Option<Session>, req)
                                          │
   ┌──────────────────────────────────────┴───────────────────────────────────┐
 Shutdown   Load/Start/Resume          Types/TypeInfo/Cell/Homology     everything else
 exit       Session::from_disk(file)   query(session, req)              apply(session, req)
            then apply (Start/Resume)   ← served from the store          ← Session::apply
            └────────────────▶ Response::{Ok,Error} ──serde_json──▶ stdout line
```

1. `run_daemon` holds an `Option<Session>`; if pre-loaded from CLI args it emits
   an initial state response, then loops over stdin lines (blank lines skipped, a
   parse failure becomes one `Response::error` and the loop continues — a bad
   line never kills the session).
2. `dispatch` handles the front-end commands itself. `Load` and `Start`/`Resume`
   **(re)load the file from disk** with `Session::from_disk` — so a fresh `start`
   always sees the current source — then `apply` for the start/resume. `Load`
   leaves the session idle (so `holes`/`fill` work before any rewrite).
   `Shutdown` exits. The read-only queries are served from the store by `query`
   (homology is *refused* in daemon mode — it was never wired to query outside a
   session there). **Everything else delegates to `Session::apply`** via `apply`,
   which first checks a session exists.
3. `emit` serialises and `writeln!`s one JSON line, then flushes.

## Data flow — a web session

`WebRepl { session: Option<Session> }` is a library object, not a server; the
HTTP/WASM/MCP crates own the transport and call its methods.

1. `WebRepl::new()` → `session: None`.
2. `load_source(text)` / `load_source_with_modules(text, modules, name)`
   constructs a `Session::from_virtual` over an in-memory `<Name>.ali → text`
   module map, returning a JSON envelope; on a parse/interpret failure it returns
   structured `diagnostics: [Diagnostic]` instead, so an editor can highlight the
   offending span. (The web does its own load to surface those diagnostics; the
   daemon assumes a clean load.)
3. `start_session` / `resume_session` build a `Request::Start`/`Resume` and call
   `Session::apply`.
4. `parse_command(line)` runs the **shared** `command::parse(line, Frontend::Web)`
   and classifies the result: UI-flow commands (`start`, `resume`, `fill`, `done`,
   `stop`, `clear`, `holes`, `backward`) return `{"status":"action", …}` for the
   frontend to drive; `help` and every plain command return
   `{"status":"request","request":…}`; an unknown command returns
   `{"status":"error", …}` — identical wording to the CLI.
5. `run_command(json)` parses a `Request` and dispatches: `help` is answered with
   no session; `Types`/`TypeInfo`/`Cell`/`Homology` are served from the store
   (the web **does** answer `homology`, unlike the daemon); `Start`/`Resume`/
   `Load`/`Shutdown` are refused ("not supported in web mode") — birth and death
   go through `start_session`/`resume_session`/`stop_session`/`reset`; everything
   else is forwarded to `Session::apply`. Successful replies bundle the
   `richtext`-rendered view in a `"rendered"` field so the browser styles it
   identically to the CLI.
6. String-diagram queries (`get_strdiag`, `get_map_image_strdiag`,
   `get_session_strdiag`, `get_target_strdiag`, `get_rewrite_preview_strdiag`,
   `get_proof_strdiag`) are web-only side channels returning
   `analysis::strdiag::StrDiag` JSON for the canvas; they have no daemon
   counterpart, and read the active engine through `Session::active_engine`.

## Non-obvious invariants and gotchas

- **`Session::apply` is the single shared command surface.** Both the daemon and
  `WebRepl::run_command` funnel non-lifecycle commands through it, so the two
  transports can never drift in *which* command does *what*. The old per-engine
  `handle` was retired with this unification; see [[interactive-session]].
- **The daemon (re)loads from disk on `start`/`resume`/`load`.** Each builds a
  fresh `Session::from_disk`, so an editor that edited the file on disk gets the
  new source without a separate reload command.
- **The daemon refuses `Homology`; the web serves it.** Homology needs only a
  `GlobalStore`; the daemon was never wired to query the store outside a session,
  so it returns an explicit refusal while `WebRepl` answers from the loaded store.
- **`ResponseData` is one struct for every command.** Optional fields are elided
  rather than modelled as separate response types; don't expect a command's extra
  payload (`fill`, `holes`, `stored`, `homology`, …) unless that command set it.
- **`Diagnostic` is the web layer's reason to differ.** It lives in
  `language::error`; the daemon assumes a clean load (CLI handled errors up
  front), whereas the web `load_source` path turns every parse/interpret failure
  into structured `Diagnostic`s the editor renders inline.
- **The web holds one `Option<Session>`, not a state enum.** The old
  `State` (`Empty`/`Loaded`/`Active`) is gone: `None` is idle/unloaded, `Some`
  with an inactive session is loaded, `Some` with an active engine/fill is
  running. `stop_session` ends the session but keeps the store; `reset` drops to
  `None`. `load_source_with_modules` sets `session = None` *before* allocating the
  new store, because in WASM peak linear-memory pages are never returned — the two
  stores must not coexist.

## Mathematics

These three modules carry no mathematics of their own — they are transport and
serialisation. Their bridge to [[rewriting]] is a **support relationship**: the
matching, pushout, and step construction live in [[core-matching]], the session
that sequences them in [[interactive-engine]], and the command semantics in
[[interactive-session]]. `protocol`'s `ResponseData` merely *describes* the state
of a session — `current`/`target` [[diagram|diagrams]], candidate `RewriteInfo`
moves, the running proof, a fill's open [[hole|holes]] — for a client to render;
`daemon` and `web` only move those descriptions across a boundary. See
[[interactive-repl]] for the terminal front-end and the shared `richtext`
renderer, [[web-backends]] for the `web/server`, `web/wasm`, and `web/mcp` crates
that drive `WebRepl`, and [[output]] for the name-keyed render tree the labels
come from.
