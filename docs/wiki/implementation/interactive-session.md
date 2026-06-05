---
kind: impl
status: stable
last-touched: 2026-06-05
code: [src/interactive/session.rs, src/interactive/command.rs, src/interactive/fill.rs]
---

# interactive-session — the shared command core

> One state machine, one command language, one set of canonical messages. Every
> front-end — the terminal REPL, the stdio daemon, the browser — parses a typed
> line into a [`Command`], turns it into a [`Request`], and hands it to
> `Session::apply`. *What a command means* lives here and nowhere else, so a new
> command lands on all three front-ends at once and they cannot drift.

This page documents the unification introduced when the CLI, web, and daemon
REPLs were merged onto a single core (`865021a`…`d5e85fb`). The mathematics is
still in [[interactive-engine]] (`RewriteEngine`); this layer is the session
*orchestration* that wraps it, plus the interactive [[hole|hole]]-filling that
reuses the same engine.

## What each module owns

| Module | Responsibility |
|---|---|
| `session.rs` | `Session` — the live session: the loaded store, the running source, an active rewrite **or** fill, the `backward` flag. Its `apply(Request)` performs **all** command semantics and canonical messages. |
| `command.rs` | `Command` — the parsed REPL command language; `parse(line, Frontend)` and `Command::to_request`. The single source of *what is a valid command, with what arguments, and what the error reads*. |
| `fill.rs` | interactive [[hole|hole]]-filling: list a module's open holes, start a fill (a rewrite or a 0-cell choice), and splice the finished proof back into the map's source definition. |

## `Session` — the state machine

`Session` (`session.rs`) holds the store (`Arc<GlobalStore>`), the canonical
`root_path`, the running `source` text, and exactly one of two mutually exclusive
sessions: `engine: Option<RewriteEngine>` (a free `start`/`resume` rewrite) or
`fill: Option<(FillContext, FillSession)>` (a hole-fill). A `backward` flag is the
idle pre-session direction; once a session starts the direction is fixed and
`session_backward` reads it back off the active engine.

Three constructors capture the only genuinely per-front-end concern — *how source
is loaded and re-evaluated* — as a `LoadStrategy`:

- `from_disk(source_file)` — `LoadStrategy::Disk`; the CLI and daemon.
- `from_virtual(source, modules, name)` — `LoadStrategy::Virtual(map)`; the web,
  whose `include`s resolve against an in-memory `<Name>.ali → text` map.
- `from_loaded(store, root_path, source, loader)` — wrap an already-loaded store
  (the web loads first to surface structured diagnostics, then constructs around
  the result).

### `apply` — the one dispatch

`apply(&mut self, req: Request) -> Result<ResponseData, String>` is the whole
command surface. `Ok` carries a `ResponseData` snapshot of the resulting state
(with a canonical one-line `message` like `Applied idem` or `Filled ?x with …`);
`Err` carries the user-facing error. It handles session lifecycle
(`Start`/`Resume`/`Stop`/`Backward`), filling (`Holes`/`Fill`/`Done`),
persistence (`Save`/`Store`), and every engine command (`Step`/`StepMulti`/
`Auto`/`Random`/`Undo`/`Redo`/`Parallel`/`SetTarget`/`Show`/`Proof`/`History`/
`ListRules`). It explicitly *refuses* the read-only queries (`Types`/`TypeInfo`/
`Cell`/`Homology`) and the front-end-only commands (`Help`/`Load`/`Shutdown`) —
those are served by the adapters from the store, not by the session, because they
need no session state.

The engine commands route through `engine_ref`/`engine_mut`, which return the
active engine whether it is a free rewrite *or* a fill's rewrite — so the same
`apply`, `undo`, `proof`, `history` drive both. A 0-cell fill has no engine, so
those helpers special-case `zero_cell_mut` (a choice is a one-step session: `step`
picks, `undo` reopens the candidates, `redo` re-picks).

### Snapshots

`snapshot` builds the `ResponseData` for the current state — the active engine's
state, a 0-cell fill's synthetic state, or an empty response when idle — and
`show`/`status` returns the loaded module's path when there is no session. A
zero-step proof is still a proof: `proof_response`'s `stored_expr` renders the
initial diagram when no rewrite has been applied, never `None` for an engine
session.

## `Command` — the REPL language

`command.rs` parses a line into a `Command` (or returns the finished error
string), then `Command::to_request(backward)` maps it to the backend `Request`.
`parse(line, fe: Frontend)` gates the medium-specific commands: `print`/`save`/
`quit` are `Frontend::Cli` only, `clear` is `Frontend::Web` only, and each
front-end treats the others' as unknown — `errors_are_identical_strings` and
`medium_specific_commands_are_gated` pin that both front-ends parse the shared
commands, aliases, and errors identically.

The command set (the source of truth for [[interactive-repl]] and the web):

**Always available** — `types`, `type <name>`, `homology <name>`, `holes`,
`start <t> <s> [<g>]`, `resume <t> <p> [<g>]`, `fill <n>`, `backward [on|off]`,
`status`/`show`, `stop`, `help`/`?`; plus CLI-only `print`, `save <path>`,
`quit`/`exit`/`q` and web-only `clear`.

**Session commands** — `apply <n> [<n2>…]` (`a`), `auto <n>`, `random <n>`,
`parallel [on|off]`, `undo [<n>]` (`u`), `undo all`/`restart`, `redo [<n>]`,
`rules` (`r`), `history` (`h`), `proof` (`p`), `store <name>`, and `done`
(finalise a fill).

`split_quoted_args` honours `'`/`"` so a composite diagram expression with spaces
stays one argument (`start Idem 'id id id' id`). `start`/`resume`/`help`/`print`/
`quit`/`clear` have **no** `Request` (`to_request` returns `None`): the adapter
builds `Start`/`Resume` itself because they also need the source path, and the
rest are handled locally.

## `fill.rs` — interactive hole-filling

A hole on an $m$-cell `x` of a map `F : D → T` is a request to build `F(x)`: an
$m$-diagram in `T` from `F(x.in)` to `F(x.out)`. For $m \ge 1$ that is a
[[rewriting|rewrite]], driven by the ordinary `RewriteEngine`; for a $0$-cell it is
the choice of one of `T`'s $0$-cells. A `FillSession` is therefore either
`Rewrite(RewriteEngine)` or `ZeroCell(ZeroCellFill)`, and `FillSession::filler`
yields the finished proof — the assembled rewrite proof, or the chosen $0$-cell.

- `list_open_holes(store, root_module)` enumerates the *actual* holes
  (`image: None`) of every map of every type, in a deterministic `(type, map,
  dim, source-name)` order — the numbering `fill <n>` uses. `list_constraints`
  lists the equations a *conditional* pending assignment imposes (`F(x.side) =
  a.side`). Both walk `visit_maps`, the single traversal that resolves each map's
  domain/target once.
- `start_fill(store, root, source_file, index, backward)` opens the fill for hole
  `index`, first checking via `blocking_holes` that the hole's dependency holes
  are filled (else *"Must fill holes … first"*). A $0$-cell becomes a
  `ZeroCellFill` over the type's $0$-cells; an $m$-cell realises the hole's
  (now hole-free) boundary trees to concrete diagrams and opens a `RewriteEngine`
  with them as initial/target (swapped under `backward`).
- `edit_for_fill` / `finalize` splice the proof back into `F`'s definition. An
  explicit `source_name => ?` clause is **replaced in place**
  (`pins_a_dotted_explicit_hole_in_place`); an implicit hole, with no `?` of its
  own, is **appended** as a new clause (`appends_when_no_matching_explicit_hole`),
  committing the cell by the idempotence of `[x => ?, x => a]`. Re-evaluating the
  edited source is what actually fills the hole — the file is the durable record.

`Session::begin_fill`/`finalize_fill` wire these into `apply`; `filled_report`
produces the shared `Filled ?x with … : in → out` message, the boundary read off
the *filler* so it is correct even for a degenerate one.

## Non-obvious invariants & gotchas

- **`engine` and `fill` are mutually exclusive.** `session_active` is
  `engine.is_some() || fill.is_some()`; `start`/`fill` refuse when one is already
  running. `stop` abandons a fill or ends a rewrite — to the user, the same act
  (*"Session stopped"*).
- **A fill is a rewrite to the command layer.** Because `engine_mut` reaches into
  a `FillSession::Rewrite`, every session command works during a fill unchanged;
  only `done` is fill-specific, and `store` is refused in a 0-cell fill (nothing
  to store).
- **`apply` owns the canonical messages.** *"Applied …"*, *"Filled …"*, *"Stored
  …"*, *"Backward mode on"* are set here, so all three front-ends print the same
  words. The renderers ([[interactive-repl]] `richtext`) only style them.
- **Read-only queries bypass `Session`.** `types`/`type`/`homology`/`cell` are
  served from the store by each adapter; `apply` returns *"Not a session command"*
  if one reaches it, by construction it never should.
- **Filling re-evaluates, and may fail.** An inconsistent fill makes
  re-evaluation error; `finalize_fill` reports it and keeps the fill session so
  the user can retry, rather than discarding work.

## Mathematics

This layer carries no mathematics of its own — it sequences the operations of
[[rewriting]] and [[partial-map|map]]-building into a session. A free session
assembles an $(n{+}1)$-[[diagram|proof]] from rewrite steps ([[interactive-engine]]);
a fill builds the image $F(x)$ of one map [[hole|hole]] — itself either a rewrite
proof or a chosen $0$-cell — and commits it back into the map. The matching and
pushout each step invokes live in [[core-matching]]; the hole datum and its
commit/cascade machinery in [[core-partial-map]] and [[hole]]. See
[[interactive-repl]] for the terminal front-end and shared renderer, and
[[interactive-daemon-web]] for the wire protocol and browser adapter that drive
this same `Session`.
