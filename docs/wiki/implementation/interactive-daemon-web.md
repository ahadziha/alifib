---
kind: impl
status: stable
last-touched: 2026-06-10
code: [src/interactive/daemon.rs, src/interactive/protocol.rs, src/interactive/web.rs]
---

# interactive-daemon-web — the session over the wire

One session core, two non-terminal mouths. `daemon` speaks JSON-lines over
stdin/stdout for editors; `web` is a stateful adapter the browser-facing
backends call as a library. Both are thin transports over the *same* command
surface — `Session::apply` ([[interactive-session]]) — and `protocol` is the
shared vocabulary of requests and response payloads they trade in. None of these
modules touches the mathematics; they carry a [[rewriting|rewrite]] or
[[hole|hole]]-filling session across a process or language boundary. The
[[interactive-session|Session]] holds the live state and performs every command;
[[interactive-engine]] does the rewriting underneath it.

## What each module owns

| Module | Responsibility |
|---|---|
| `protocol.rs` | the wire vocabulary: `Request` (a `#[serde(tag="command")]` enum), the `Response` envelope, the `ResponseData` snapshot and all its sub-structs, plus the *builders* that turn the store or an engine into those structs |
| `daemon.rs` | `run_daemon` — the read-line / dispatch / write-line loop for `alifib serve`; loads a `Session` and forwards to `Session::apply` |
| `web.rs` | `WebRepl` — a handle wrapping a single `Option<Session>` that the three web backends (HTTP server, WASM shim, MCP server — [[web-backends]]) drive as a library; owns source loading, session setup, command dispatch, and string-diagram queries |

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
(`source_diagram`, `initial_diagram`, `target_diagram`) for backward compat.
`Cell` is wire-only — the typed `Command` language has no `cell` word. The
durable session form is the proof term — fetched with `Proof` (`proof_expr`) and
re-ingested by `Resume`.

`Session::apply` handles most of these and refuses the rest (*"Not a session
command"*): `Types`/`TypeInfo`/`Homology` are read-only queries served from the
store by each adapter, `Cell` needs an engine for its type context, and
`Help`/`Load`/`Shutdown` are front-end concerns — the daemon and web each peel
those off before `apply`. The MCP server's tools forward this same vocabulary
(its `command` tool excludes `start`/`resume`/`load`, exactly as web mode does);
the per-tool surface is documented in [[web-backends]].

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
| `types` / `type_detail` / `cell_detail` | `types` / `type` / `cell` (adapter-filled) | type summaries / full detail / cell detail |
| `auto` / `stored` | `auto`/`random` / `store` | the step-count summary / the appended `let` clause |
| `homology` | `homology` | `HomologyInfo` (groups + Euler characteristic; each group carries its `TorsionWitnessInfo`s — cycle + certifying preimage) |
| `fill` | during any fill | `FillInfo` — the hole being built (type, map, domain, source, dim) |
| `holes` / `constraints` | `holes` | `HoleInfo` per open hole / `ConstraintInfo` per conditional pending assignment |
| `zero_cell` | during a 0-cell fill | `ZeroCellInfo` — candidate 0-cells and the current pick |
| `source` | `done` / `store` / virtual `save` | the updated running source for the editor to write (a disk session's `save` writes the file itself and only reports `Saved to '…'`) |
| `module` | `show`/`status` when idle | the loaded module's path |

`DiagramInfo` (flat `label`, `dim`, `cell_count`, per-dimension `cells_by_dim`),
`RewriteInfo`/`FamilyMember` (one move: rule(s), the rule's `input`/`output`
boundaries, `match_positions`, a `[bracketed]` `match_display`), and the
type-detail sub-structs (`TypeDetailInfo`, `MapEntry` — whose `holes` field
pre-renders a map's open holes as `?name : in → out`) round it out. The builders
that fill these — `diagram_info`, `build_response`, `build_list_rules_response`,
`build_types_from_store`, `build_type_detail_from_store`, `build_cell_response`,
`build_homology_data`, and the `strdiag` family (`build_strdiag_response`,
`build_map_image_strdiag`, `step_output_strdiag_json`, `strdiag_to_json`) — are
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
            then apply (Start/Resume)                                    ← Session::apply
            └────────────────▶ Response::{Ok,Error} ──serde_json──▶ stdout line
```

1. `run_daemon` holds an `Option<Session>`; if pre-loaded from CLI args it emits
   an initial state response (only when `session_active`), then loops over stdin
   lines (blank lines skipped; a parse failure becomes one `Response::error` and
   the loop continues — a bad line never kills the session).
2. `dispatch` handles the front-end commands itself. `Load` and `Start`/`Resume`
   **(re)load the file from disk** with `Session::from_disk` — so a fresh `start`
   always sees the current source — then `apply` for the start/resume. `Load`
   leaves the session idle (so `holes`/`fill` work before any rewrite) and
   replies with the store's `types`. `Shutdown` exits. `query` serves
   `Types`/`TypeInfo` from the loaded store, `Cell` from `Session::engine`
   (a *free* session only — refused during a fill, and `Homology` is refused
   outright in daemon mode). **Everything else delegates to `Session::apply`**
   via `apply`, which first checks a session exists.
3. `emit` serialises and `writeln!`s one JSON line, then flushes.

## Data flow — a web session

`WebRepl { session: Option<Session> }` is a library object, not a server; the
HTTP/WASM/MCP crates own the transport and call its methods.

1. `WebRepl::new()` → `session: None`.
2. `load_source(text)` / `load_source_with_modules(text, modules, name)` does its
   **own** load — `InterpretedFile::load` over `Loader::with_virtual_files`
   (`<Name>.ali → text`), then wraps the store with `Session::from_loaded(…,
   LoadStrategy::Virtual(modules))` — rather than `Session::from_virtual`,
   precisely so a parse/interpret failure can return structured
   `diagnostics: [Diagnostic]` for the editor to highlight. (The daemon assumes a
   clean load; the CLI reported errors up front.) The success reply carries the
   frontend's `types` summary JSON.
3. `start_session` / `resume_session` build a `Request::Start`/`Resume` and call
   `Session::apply` — these are also the MCP `start_session`/`resume_session`
   tools' entry points.
4. `parse_command(line)` runs the **shared** `command::parse(line, Frontend::Web)`
   and classifies the result: UI-flow commands (`start`, `resume`, `fill`, `done`,
   `stop`, `clear`, `holes`, `backward`) return `{"status":"action", …}` for the
   frontend to drive; `help` and every plain command return
   `{"status":"request","request":…}`; an unknown command returns
   `{"status":"error", …}` — identical wording to the CLI
   (`web_parse_command_shares_the_cli_parser`, tests/web_fill.rs).
5. `run_command(json)` parses a `Request` and dispatches: `help` is answered with
   no session; `Types`/`TypeInfo`/`Homology` are served from the store (the web
   **does** answer `homology`, unlike the daemon) and `Cell` from
   `Session::active_engine` (so it also works inside a fill's rewrite, unlike the
   daemon's `engine`); `Start`/`Resume`/`Load`/`Shutdown` are refused ("command
   not supported in web mode") — birth and death go through
   `start_session`/`resume_session`/`stop_session`/`reset`; everything else is
   forwarded to `Session::apply`. When the request has a `RenderKind`, the reply
   bundles the `richtext`-rendered view in a sibling `"rendered"` field so the
   browser styles it identically to the CLI
   (`cli_and_web_responses_are_identical`, tests/cli_render.rs); message-only
   replies and `cell` come without it.
6. String-diagram queries are web-only side channels returning
   `analysis::strdiag::StrDiag` JSON for the canvas, with no daemon counterpart:
   `get_strdiag` and `get_map_image_strdiag` need only the loaded store;
   `get_session_strdiag`, `get_target_strdiag`, `get_rewrite_preview_strdiag`
   read the active engine via `Session::active_engine`; `get_proof_strdiag` and
   `set_proof_view` (the incremental proof cache toggle) take
   `active_engine_mut`. `get_types` re-serves the load-time type summary for the
   frontend accordion.

## The `thin` annotation — greying out coherence cells

A type built for rewriting is mostly *scaffolding*: identities, unitors,
associators, the naturality squares that let a node slide past a wire. Drawn at
full strength these swamp the handful of cells that are the actual rewrite
rules. The **`thin` annotation** is the author's way of saying "these cells are
coherence, not content," so the [[string-diagram]] canvas can mute them. It is
read **only** here, in `web.rs`, and consumed **only** by the renderer — the
matching/pushout/step machinery ([[core-matching]], [[interactive-engine]])
never looks at it. So thinness carries **no semantic force**: it does not mark a
cell invertible, does not change what rewrites or how, and is *not* the
categorical notion of a thin/degenerate cell. It is display metadata, nothing
more — the most load-bearing fact about it.

The annotation is an ordinary **index** — the `index Name = [ … ]` string-list
templating construct ([[language-parser]]) — that happens to be named exactly
`thin`. That name is a magic string `web.rs` privileges; the lexer and parser
give it no special status, and nothing checks that the listed cells *are*
coherence. In `examples/TRS/Aux.ali` a `Wire` ends with `index thin = [ Idem ]`
(its idempotent $\mathsf{Idem} : \mathrm{id}\,\mathrm{id} \to \mathrm{id}$); a
node lists its unit coherences, `index thin = [ DomId, CodId ]`; the larger
algebras in `examples/TRS.ali` list their unitors, associators, and `*_Nat`
naturality cells.

The pipeline is three steps in `web.rs` plus one in the frontend:

1. **Seed — `compute_thin_tags`.** For one type complex, read
   `Complex::find_index("thin")` and resolve each listed name to a
   [[core-complex|`Tag`]]: a generator via `find_generator`, or a let-bound
   [[diagram]] via `find_diagram` then its `top_label`. So `thin` may name
   either a raw generator or a `let`-bound cell.
2. **Propagate — `propagate_thin_through_maps`.** Thinness flows along
   [[partial-map|attach-maps]]. For every map in the complex, resolve its domain
   complex; for each domain generator already known thin, take its image under
   the map (`image`); if that image is a single cell, mark the cell's
   `top_label` thin. This is why `Unit`'s attached `Aux.Wire` drags the wire's
   `Idem` onto `Unit` without `Unit` re-declaring it.
3. **Accumulate and emit — `type_summaries_json`.** The builder walks every
   `(module, type)` pair in `GlobalStore::normalize` order, carrying a running
   `known_thin: HashSet<Tag>`. Per type it seeds with `compute_thin_tags`,
   extends `known_thin`, then propagates against that running set — so an
   attached type's thin cells reach a host only once they have themselves been
   visited and seeded (propagation reads the set as it stands, in iteration
   order). Each type's JSON carries a `thin_tags` array: its own seeds plus the
   freshly propagated images, each a **bare integer** — the global id, via
   `protocol::tag_to_json` (local/hole tags serialise to `null`). The same
   payload gives every generator a `tag` and its `face_tags`: the
   $\partial^-$/$\partial^+$ top-dimension boundary labels, via `face_tags_json`.
   `type_summaries_json` is served by `WebRepl::get_types` and bundled into the
   load reply, so the frontend has it the moment a file loads.
4. **Render — frontend ([[web-frontend]], `web/frontend/src/app.js`).** On load,
   `app.js` fills `thinTags` from every type's `thin_tags` and `tagFaces` from
   the generators' `face_tags`, then `recomputeFullyThin` derives a second set:
   a tag is **fully thin** iff it is thin *and* every face in `tagFaces` is thin
   too — coherence all the way down. The canvas reads both. A thin **wire**
   (`drawWire`) is stroked in the muted `thin` colour instead of `wire`; a thin
   **node** collapses from a full node disc (`NODE_R`) to a wire-sized dot
   (`WIRE_R`), filled with the `thin` colour when fully thin and the `wire`
   colour otherwise; thin **labels** are muted to match. The accordion adds a
   per-cell **thin-toggle** that edits `thinTags` live and re-renders — so the
   `index thin` list is only the *initial* thin set; a reader may promote or
   demote any cell in the GUI, and that override is session-only (never written
   back to source).

## Non-obvious invariants and gotchas

- **`Session::apply` is the single shared command surface.** Both the daemon and
  `WebRepl::run_command` funnel non-lifecycle commands through it, so the two
  transports cannot drift in *which* command does *what*; see
  [[interactive-session]]. The fill workflow behaves like a session over the
  wire too — `web_fill_one_dim_hole`, `web_premature_done_keeps_session`,
  `web_zero_cell_fill_behaves_like_a_session` (tests/web_fill.rs).
- **The daemon (re)loads from disk on `start`/`resume`/`load`.** Each builds a
  fresh `Session::from_disk`, so an editor that edited the file on disk gets the
  new source without a separate reload command.
- **`Cell` is never store-served.** It needs an engine for its type context: the
  daemon takes `Session::engine` (free sessions only), the web
  `Session::active_engine` (free *or* fill) — so `cell` inside a fill works on
  the web but is refused by the daemon.
- **The daemon refuses `Homology`; the web serves it.** `build_homology_data`
  needs only the store, but the daemon's `query` returns an explicit
  "homology not supported in daemon mode" while `WebRepl` answers from the
  loaded store.
- **`save` is asymmetric by `LoadStrategy`.** A `Disk` session (CLI, daemon)
  writes the running source to the path itself; a `Virtual` session (web) cannot
  write, so `ResponseData.source` hands the text to the editor. `done` and
  `store` return `source` on both.
- **`ResponseData` is one struct for every command.** Optional fields are elided
  rather than modelled as separate response types; don't expect a command's extra
  payload (`fill`, `holes`, `stored`, `homology`, …) unless that command set it.
- **`Diagnostic` is the web layer's reason to differ.** It lives in
  `language::error`; the web load path turns every parse/interpret failure into
  structured `Diagnostic`s the editor renders inline, which is why `WebRepl`
  bypasses `Session::from_virtual` and loads itself.
- **The web holds one `Option<Session>`, not a state enum.** `None` is
  idle/unloaded, `Some` with an inactive session is loaded
  (`web_show_when_idle_reports_module`), `Some` with an active engine/fill is
  running. `stop_session` ends the session but keeps the store; `reset` drops to
  `None`. `load_source_with_modules` sets `session = None` *before* allocating
  the new store, because in WASM peak linear-memory pages are never returned —
  the two stores must not coexist.
- **The `thin` index is display-only and a magic string.** `compute_thin_tags`
  reads an index named literally `thin`, propagates it along attach-maps
  (`propagate_thin_through_maps`), and emits each type's `thin_tags` purely so
  the canvas can grey coherence cells. The rewriting engine never reads it; the
  grammar gives the name no status. See *The `thin` annotation* above.

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
