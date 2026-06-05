---
kind: impl
status: stable
last-touched: 2026-06-05
code: [web/frontend/src/app.js, web/frontend/src/ali-lang.js, web/frontend/index.html]
---

# web-frontend — the browser GUI

> A single-page, bundle-only browser app. A CodeMirror editor on the left, an
> interactive [[rewriting]] REPL in the middle, and a canvas that draws the
> current diagram as a [[string-diagram]] on the right. It owns no mathematics:
> it ships `.ali` source to a backend, receives JSON snapshots and string-diagram
> render trees, and paints them.

The frontend is a vanilla-JS ES module (`src/app.js`, ~3200 lines) bundled by
**esbuild** into `dist/app.js`; `index.html` loads only that one bundle plus
`style.css`. The sole runtime dependency family is **CodeMirror 6** (editor,
language, search, autocomplete, commands) plus `@lezer/highlight` for the alifib
syntax mode. There is no framework and no virtual DOM — DOM is built by hand and
the diagram is drawn to a `<canvas>`.

## What it owns

The browser-side of an alifib session: editing source, driving a rewrite session
through the wire protocol, and rendering diagrams to a canvas. It is one of three
consumers of the same backend command surface (the others — daemon and terminal
REPL — are documented in [[interactive-daemon-web]] / [[interactive-repl]]).

## Key components

| Area (`app.js` section) | Responsibility |
|---|---|
| **Backend abstraction** (`WasmBackend`, `HttpBackend`, `createBackend`, `backendConfig`) | one async method surface (`load_source`, `parse_command`, `run_command`, `start_session`, `get_*_strdiag`, …) over two transports; selected at boot |
| **Editor & tabs** (`makeEditorState`, `createTab`, `switchTab`, `renderTabBar`) | CodeMirror state per tab, dirty tracking, untitled-naming, the `+` tab |
| **Evaluate** (`evaluateSource`) | reset → collect `include` modules → `load_source` → build the type accordion, populate the Type selector, cache thin/face tags |
| **Session lifecycle** (`startSession`, `startSessionFromRepl`, `startResumeFromRepl`, `enterSession`, `resetSession`) | drive `Empty→Loaded→Active` from the setup form or a REPL `start`/`resume` action |
| **REPL** (`handleCommand`, `runAction`, `renderResult`) | classify a typed line via the backend's shared `parse_command`, then either drive a UI *action* or forward a ready *request* to `run_command`; history with `↑`/`↓` |
| **Transcript rendering** (`renderSegments`, `ROLE_CLASS`, `formatError`, `renderDiagnostic`, `appendReplEntry`) | style the backend's shared **RichText** (`rendered` field) and structured diagnostics into REPL HTML — no command-specific renderers live here |
| **String-diagram layout** (`layoutStrDiag`, `longestPathDistances`, `separateOverlaps`, `toScreen`/`fromScreen`) | position vertices from the render tree's three edge DAGs |
| **String-diagram rendering** (`renderStrDiag`, `resizeAndRender`, `entryPoint`, `topoSort`) | draw wires (quadratic Béziers) and nodes to the canvas |
| **Diagram interaction** (`mousedown`/drag handler, pan, zoom) | drag a vertex (height-graph BFS influence falloff), pan, zoom |
| **Session diagram & rewrites** (`showSessionDiagram`, `buildRewriteList`, `showRewritePreview`, `applyRewrite`, `applyBunch`, `performUndo`/`performRedo`, `toggleProofView`) | the Analysis pane while a session is live |
| **Item inspection** (`selectItem`, `refreshInfobox`, boundary/sign controls) | click a generator/diagram in the accordion to view it or a chosen [[boundary]] |
| **Pane layout** (`distributeSizes`, the `sync*Layout`/`*Drag` family) | three resizable horizontal panes; vertical splitters inside Analysis |
| **Examples & I/O** (`populateExamples`, `fetchExampleByKey`, `collectIncludeModules`, `resolveIncludeKey`) | the Examples dropdown and `include <Name>` resolution (see `web/EXAMPLES.md`) |
| **Syntax** (`src/ali-lang.js`) | the CodeMirror StreamLanguage mode + dark/light highlight styles for `.ali` |

## Two backends, one surface

`createBackend` returns either a `WasmBackend` or an `HttpBackend`; both expose
the identical async method set and **every method returns a JSON *string*** that
the caller parses with `parseReplResponse`. The mode comes from
`globalThis.ALIFIB_CONFIG.backend` or a `?backend=` query param, defaulting to
`wasm`.

- **WASM** — dynamically imports `../pkg/alifib_wasm.js` (the `web/wasm` crate
  built by wasm-pack, kept `--external` from the esbuild bundle), constructs a
  `WasmRepl`, and calls its exported methods in-process. No server.
- **HTTP** — `fetch`-`POST`s JSON to `/api/<method>` on the localhost server
  (`web/server`, the `alifib-web-server` crate). Request bodies are snake_case
  (`type_name`, `boundary_dim`, `command_json`) matching the server's routes.

Both ultimately drive the *same* `WebRepl` adapter inside the `alifib` library —
the `WasmRepl` wraps it directly, the server wraps it behind HTTP. The frontend
no longer builds `Request` JSON itself: `handleCommand` sends the raw typed line
to the backend's **shared parser** (`parse_command`, the same Rust parser the CLI
uses), which returns one of three classifications — `error`, `action`, or
`request`. `action`s (`start`/`resume`/`fill`/`done`/`stop`/`clear`/`holes`/
`backward`) are UI flows the frontend drives via `runAction`; a `request` carries
a ready `Request` that `handleCommand` re-serialises straight into `run_command`,
which the backend forwards to `Session::apply` ([[interactive-session]]). Keeping
parsing — and its error wording — in one place is why the web and CLI REPLs cannot
drift. Session birth and death are *not* wire commands here: they go through the
dedicated `start_session`/`resume_session`/`stop_session`/`reset` methods on the
`WebRepl`'s single `Session`. See [[web-backends]] for the three backend crates and
[[interactive-daemon-web]] for the protocol they share.

## String diagrams: render tree → canvas

The frontend never computes a diagram's geometry as mathematics — the backend
hands it a **render tree** (`StrDiag` JSON from `analysis::strdiag`, serialised by
`protocol::strdiag_to_json`) and the frontend only *positions and paints* it.

The render tree is `{ num_wires, num_nodes, vertices[], height{edges}, width{edges},
depth{edges} }`: a flat vertex list (each `{index, kind: wire|node, label, tag}`)
and three directed acyclic edge sets over those indices.

1. **`layoutStrDiag`** builds adjacency/predecessor tables for the height and
   width DAGs and runs **`longestPathDistances`** (Kahn topo-sort + longest-path
   in both directions) on each. A vertex's abstract coordinate is its centred
   band fraction `(bw+1)/(bw+fw+2)` in width and height. `separateOverlaps`
   nudges near-coincident vertices onto a small circle. The depth DAG is carried
   through for draw-order.
2. **`toScreen`/`fromScreen`** map abstract `(w,h)` to normalised screen `(x,y)`
   per the chosen orientation (`bt`/`tb`/`lr`/`rl`).
3. **`renderStrDiag`** scales to canvas pixels (devicePixelRatio-aware via
   `resizeAndRender`) and strokes each wire as a pair of quadratic Béziers
   through its predecessors/successors (`entryPoint` supplies a boundary stub when
   a wire has none), then fills nodes. Thinness/face tags drive colour.

The canvas is interactive: clicking near a vertex starts a drag whose
displacement propagates along the height graph with BFS distance-decay (`DECAY =
0.5`); the surrounding wheel/space handlers pan and zoom.

## Editor: the alifib CodeMirror mode

`ali-lang.js` defines a `StreamLanguage` token-by-token scanner for `.ali`, not a
full grammar. It recognises nested `(* … *)` comments (depth counter),
`@`-decorations (`@Type` vs other `@…`), `<Name>` interpolations, the
`Name <<= …` type-head form (lookahead for `<<=`), `#N` pastes, the arrow family
(`<<=`/`->`/`=>`/`::`/`#`/`=`), `?` holes, and the two keyword sets
(`KEYWORDS_CONTROL`, `KEYWORDS_OTHER`). `aliExtensions(dark)` bundles the language
with a dark or light `HighlightStyle`; the editor swaps it through a CodeMirror
`Compartment` on theme toggle.

## Panel layout

`index.html` is a three-pane horizontal `workspace`:

- **File** — examples dropdown, open/save, **Evaluate**, a tab bar, and the
  CodeMirror editor.
- **REPL** — the session-setup form (Type / Initial / Target / Backward), a
  stop button, scrollable output history, and the command input.
- **Analysis** — the type/generator accordion (after Evaluate) and, during a
  session, an infobox + the `<canvas>` diagram + the rewrite list, with an
  Appearance menu (orientation, node/wire labels, zoom).

All three panes and the inner Analysis sections are resizable; the splitter logic
(`distributeSizes`, the `sync*Layout` / `start*Drag` / `update*Drag` family)
keeps proportions across window resizes and persists nothing.

## Non-obvious invariants and gotchas

- **Every backend method returns a JSON string, not an object.** Callers must go
  through `parseReplResponse` (`JSON.parse`). The `HttpBackend` even synthesises
  `{"status":"error",…}` strings for network/`fetch` failures so the frontend has
  a uniform `{status, …}` shape regardless of transport.
- **The bundle externalises the WASM glue.** `esbuild … --external:../pkg/alifib_wasm.js`
  — the wasm-pack output is fetched at runtime by the dynamic `import`, not
  inlined. A missing `pkg/` directory only fails when the WASM backend is chosen.
- **`start`/`resume`/`stop` are method calls, never `run_command`.** The wire
  protocol *refuses* lifecycle commands in web mode (`WebRepl`), so the shared
  `parse_command` classifies those REPL words as `action`s; `runAction` routes
  them to the dedicated `start_session`/`resume_session`/`stop_session` methods
  rather than to `run_command`.
- **Command parsing and the transcript layout both live in Rust.** The frontend
  holds no per-command renderers — `renderResult` styles the backend's shared
  RichText (the `rendered` field, produced by `render_response`) through
  `renderSegments`/`ROLE_CLASS`, the web half of the one renderer the CLI also
  uses. After a state-changing request (`STATE_REQS`: step/undo/show/store/…)
  it refreshes the diagram pane via `updateVisInfo`/`showSessionDiagram`.
- **Evaluate fully resets first.** `evaluateSource` calls `repl.reset()` and
  `resetSession()` before loading, then rebuilds the accordion and Type selector
  from scratch — there is no incremental reload. Include modules are gathered
  client-side by `collectIncludeModules` and passed alongside the source.
- **Proof view fetches a second render tree.** `toggleProofView` flips
  `set_proof_view` on the backend and `showSessionDiagram` then pulls
  `get_proof_strdiag` (carrying an `output_boundary_map`) instead of the per-step
  diagram; `currentLayout` switches between the step and proof layouts.
- **Layout is recomputed, never cached across diagrams.** `currentLayout` is
  replaced on each `showSessionDiagram`/`selectItem`; dragging mutates only the
  abstract positions of the live layout.
- **The examples manifest is built at deploy time, not by the frontend.** The
  dropdown is populated from `GET examples/index.json`. Under `alifib web` that
  file comes from `ExampleSet::index_json` ([[web-backends]]); for the static
  WASM site it is generated by `scripts/build_examples_manifest.py`, which the
  GitHub Pages workflow (`.github/workflows/deploy.yml`) runs to mirror
  `examples/` into `dist/examples/` with a sorted `{display_name: relpath}`
  manifest. Recursive example names are the relative path minus `.ali` (e.g.
  `TRS/Aux`), and every path segment must be a valid identifier
  (`[A-Za-z_][A-Za-z0-9_]*`) — the script enforces exactly the rules
  `web/shared`'s `ExampleSet` does, so local preview and deploy agree.
- **Client-side `include` resolution mirrors the interpreter's precedence.**
  `collectIncludeModules` chases `include <Name>` transitively and
  `resolveIncludeKey` picks the example key by the same own-dir / same-named-subdir
  / fallback order the loader uses ([[aux]]): for a parent `Dir/Foo`, an
  `include Aux` is tried as `Dir/Aux`, then `Dir/Foo/Aux`, then bare `Aux`. The
  gathered `<Name>.ali → contents` map is handed to `load_source` so the backend's
  virtual loader resolves the includes without any filesystem access; open editor
  tabs override fetched examples for the same name.

## Mathematics

This page realises no mathematics; it is a renderer and a client. Its bridge is
what it *displays*:

- It draws the current diagram as a [[string-diagram]] — the
  `analysis::strdiag` render tree (wires, nodes, and the height/width/depth
  partial orders) computed backend-side and merely laid out and painted here.
  The thing being drawn is a [[diagram]] in a [[regular-directed-complex]].
- It drives [[rewriting]] interactively: the rewrite list is the set of available
  moves (`RewriteInfo`) for the current diagram, and applying one steps the
  session. Each move concerns a [[boundary]] (input/output, $\partial^-/\partial^+$)
  the infobox lets you inspect.
- The session it talks to lives in [[interactive-engine]]; the wire vocabulary it
  speaks is in [[interactive-daemon-web]]; the servers it connects to are in
  [[web-backends]].
