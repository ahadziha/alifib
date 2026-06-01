---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/interactive/repl.rs, src/interactive/cli.rs, src/interactive/render.rs, src/interactive/display.rs]
---

# interactive-repl — the readline front end

> Four files turn a `.ali` file into a live rewriting session. `cli` parses the
> argv that launches it, `repl` is the read–eval loop and command dispatcher,
> `render` turns engine state into strings, and `display` is the single throat
> through which every byte of output passes. The arithmetic of rewriting lives
> in [[interactive-engine]]; this layer only drives it and shows the result.

## What it owns

This is the terminal interface to a [[rewriting|rewrite]] session: the surface a
human types at. It owns nothing mathematical — it holds a `RewriteEngine`
([[interactive-engine]]) and a `Display`, reads lines via `rustyline`, parses
them into a `Cmd`, mutates the engine, and prints. Every other front end
(`daemon`, `web`, the MCP server) shares the same engine but bypasses this
module entirely; `repl.rs` is the only one of these four files that is gated
behind `#[cfg(feature = "cli")]` (`cli.rs`, `render.rs`, and `display.rs` are
always compiled). The standalone `alifib` binary ([[cli]]) is what parses argv
and calls this module's `run_repl_cmd` / `run_serve_cmd` to launch a session.

## Key public types and entry points

| Symbol | File | Role |
|---|---|---|
| `run_repl` | `repl.rs` | the loop for `alifib repl <file>`; loads context, then read–eval forever |
| `dispatch_engine_cmd` | `repl.rs` | apply one engine-level command to the active session |
| `try_start_session` / `try_resume_session` | `repl.rs` | build the engine from an initial diagram (`start`) or a proof (`resume`) |
| `ReplHelper` / `make_editor` | `repl.rs` | the `rustyline` helper that paints the `❯` prompt, and the editor factory |
| `ReplArgs`, `ServeArgs`, `WebArgs`, `McpArgs` | `cli.rs` | parsed argv for the four interactive subcommands |
| `run_repl_cmd`, `run_serve_cmd` | `cli.rs` | the two dispatchers wired here |
| `print_state`, `print_history`, `render_step` | `render.rs` | engine state → coloured lines / highlighted expression |
| `Display` | `display.rs` | terminal sink; the **only** place ANSI codes live |

`Cmd` (`repl.rs`, internal) is the parsed-command sum type; `parse_command` is
the hand-rolled tokeniser that produces it.

## Data flow — one keystroke to one screen

```
argv ──parse_repl_args──▶ ReplArgs ──run_repl_cmd──▶ run_repl
                                                        │
   load_file_context(file) ─▶ (store, canonical_path, file_output)   [once]
   optional --type/--source/--target ─▶ try_start_session ─▶ engine
                                                        │
        ┌───────────── 'repl loop ────────────────────┐
        rl.readline("❯ ")                              │
        line.split(';')  ── each part ──▶ parse_command ─▶ Cmd
                                                        │
        Cmd::{Types,Status,Type,Homology,Start,Resume,Stop,…}  ← always available
        else require engine.as_mut():                   │
              Cmd::{Apply,Auto,Undo,Redo,Rules,…} ─▶ dispatch_engine_cmd
                                                        │
                            engine mutates ─▶ show_state ─▶ print_state
                                                        │
                                                   Display.meta/inspect…
```

The loop is line-buffered but **command-buffered too**: each input line is split
on `;`, and every semicolon-separated `part` is parsed and dispatched in turn
(`for part in line.split(';')`). So `start C a b ; apply 0 ; show` runs as three
commands.

`run_repl` keeps exactly four pieces of mutable state across iterations:
`engine: Option<RewriteEngine>` (None until `start`/`resume`, dropped on `stop`),
`stored_defs` (the `(type, name, expr)` triples accumulated by `store`, flushed
to disk by `save`), `backward` (the pre-session backward-mode flag), and the
reloadable `store: Arc<GlobalStore>`. The immutable pair
`(canonical_path, file_output)` is computed once by `load_file_context`;
`canonical_path` (not the raw `source_file` argument) is the engine's store key
— see [[interactive-engine]] `register_proof`.

## Commands

Two tiers, gated on whether a session exists:

- **Always available** — `types`, `type <name>`, `homology <name>`,
  `start <type> <source> [<target>]`, `resume <type> <proof> [<target>]`,
  `backward [on|off]`, `status`/`show`, `print`, `stop`, `help`/`?`,
  `quit`/`exit`/`q`.
- **Require an engine** — `apply <n> [<n2>…]` (`a`), `auto <n>`, `random <n>`,
  `parallel [on|off]`, `undo` (`u`) / `undo <n>` / `undo all`, `redo` /
  `redo <n>`, `restart`, `rules` (`r`), `history` (`h`), `proof` (`p`),
  `store <name>`, `save <path>`.

`resume` is open-ended without a target and works toward `<target>` with one. It
loads `<proof>` (a finished proof diagram) and decomposes it into the steps that
built it — see [[interactive-engine]] `resume`. There is no `load` command: a
session is restored by `resume`-ing a stored proof, not from a session file.

Dispatch order in `run_repl` is deliberate: always-available commands and the
two *error* arms (`Cmd::Unknown`, `Cmd::UsageError`) are matched **first**, so a
typo reports a clean error regardless of session state; everything else falls to
the catch-all `cmd => match engine.as_mut()` arm, which emits *no active
session* when there is no engine. `Store` and `Save` are peeled off there before
`dispatch_engine_cmd` because they need `store`/`stored_defs`/`source_file`,
which the engine alone does not hold.

## Rendering — `render.rs`

`render.rs` splits *pure string builders* (`render_step`) from *display
functions* (`print_state`, `print_history`) that take a `&Display`; no
`println!` appears in the file. `render_step` pulls the step's input paste tree
(`step.tree(Sign::Input, n+1)`), collects the top-dimension rule tags via
`step.labels_at(n+1)`, and walks the [[core-paste-tree|PasteTree]] with
`render_tree_highlighting`, wrapping every leaf whose tag is a rule in
`[brackets]`. Composition nodes render as `(… #k …)` chains, matching the house
$\#_k$ notation. `print_state` then hands that bracketed string to
`Display::colorize_match_display` and emits the `(idx) …` / `by rule : input ->
output` block, or the completed-proof line.

## Display — `display.rs`

`Display { color: bool }`; `Display::new` sets `color` from
`stdout().is_terminal()`, so a redirected pipe gets clean plain text and a TTY
gets ANSI. The palette is **semantic**, mirroring `web/frontend/style.css`: the
constants name *roles*, not hues — `C_DIM` (chrome/secondary text), `C_EM` (bold
emphasis), `C_ACCENT` (prompt + rewrite indices), `C_SEC` (section titles),
`C_SRC` (matched input pattern), `C_TGT` (rewrite output), `C_OK`, `C_ERR`,
`C_CELL`, and `RESET` — all defined here and nowhere else, so a reskin is one line
each. The method palette: `meta`/`error` (a dim `>> ` prefix, body green or red),
`inspect`/`cell`/`file` (yellow cell/type/source text), `inspect_rich` (dim `>> `
prefix, body left untouched so a caller can embed its own codes), the inline
*painters* `hi`/`dim`/`sec`/`ok`/`acc` and `paint_source`/`paint_target` (each
returns a coloured fragment, or the bare string when colour is off),
`colorize_match_display` (paint the outermost `[…]`), and `blank`.

## CLI — `cli.rs`

Hand-rolled argv parsing for the four interactive subcommands. Each
`parse_*_args` walks the slice once with an explicit iterator and a `next_arg`
helper for flag values; unknown `-flags` and repeated positionals are rejected
with a usage string. `parse_repl_args` requires the positional `<file>` and
accepts `--type`/`--source`/`--target`/`--emacs`. Only `run_repl_cmd` and
`run_serve_cmd` dispatch from here; `web`/`mcp` parse their args here but are
launched elsewhere.

## Non-obvious invariants and gotchas

- **`repl.rs` is feature-gated.** `#[cfg(feature = "cli")]` on the module *and*
  on `run_repl_cmd`'s import — a build without the `cli` feature has no REPL at
  all. The engine, daemon, and web paths do not depend on it.
- **`Display::new` decides colour once.** Detection is at construction from
  `is_terminal()`; piping `alifib repl … | cat` yields plain text deterministically.
- **The dispatcher routes errors before session-state.** `Unknown`/`UsageError`
  are matched ahead of the engine-required catch-all, so a malformed command
  never masquerades as "no active session".
- **`stop` only drops the engine.** `store`/`canonical_path`/`file_output`
  survive; the backward flag persists. A subsequent `start` reuses the loaded
  module.
- **`store` then `save` is two-step.** `store <name>` registers the proof in the
  engine *and* appends a `(type, name, expr)` triple to `stored_defs`; nothing
  hits disk until `save <path>`, which `write_updated_file` appends as `@Type\nlet
  name = expr` blocks to the original source.
- **`render_step` returns `"?"` on a missing input tree.** A step with no
  `Sign::Input` tree at dimension $n{+}1$ degrades gracefully rather than
  panicking — worth knowing when a rewrite renders as a bare `?`.
- **`render_match_highlight` is gone.** `mod.rs`'s submodule table still lists
  it; the live function is `render_step`. Trust the source, not the table.

## Mathematics

This module has no mathematical content of its own — it is the I/O skin over the
rewriting machinery, so its bridge is a **support** relationship, not a
realisation. What it *surfaces*:

- [[rewriting]] — the whole point of the session. `apply`/`auto`/`random` drive
  rewrite steps; `rules` lists the $(n{+}1)$-generators applicable at the current
  top dimension; `print_state` shows the candidate steps and, on completion, the
  assembled proof cell with its $\partial^-/\partial^+$ boundaries. The actual
  matching and pushout happen in [[core-matching]] behind [[interactive-engine]].
- [[diagram]] — everything shown on screen is a diagram. `render_step` and
  `render_diagram` (from [[output]]) turn a [[core-paste-tree|PasteTree]] into a
  $\#_k$-chain of labels; `print_history` lists each step's rule; boundaries are
  read with `Diagram::boundary` ([[core-diagram]]).

See [[interactive-engine]] for the session state these commands mutate,
[[output]] for `render_diagram`, and [[interactive-daemon-web]] for the
non-terminal front ends that share the same engine.
