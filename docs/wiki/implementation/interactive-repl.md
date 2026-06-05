---
kind: impl
status: stable
last-touched: 2026-06-03
code: [src/interactive/repl.rs, src/interactive/cli.rs, src/interactive/richtext.rs, src/interactive/display.rs, src/interactive/render.rs]
---

# interactive-repl — the terminal front end and shared renderer

> The CLI REPL is now a *thin adapter*. `repl` reads a line, parses it (with the
> shared `command` parser), hands the resulting [`Request`] to
> [`Session::apply`][[interactive-session]], turns the `ResponseData` into a
> `RichText` with `richtext`, and styles it to the terminal with `display`.
> Command semantics and canonical messages live in [[interactive-session]]; the
> *layout* lives in `richtext`, shared verbatim with the web. This page covers the
> CLI-local pieces — the readline loop, argv parsing, the structured renderer, and
> the ANSI styler.

## What it owns

Five files, but only two of them are CLI-specific. `repl.rs` (gated behind
`#[cfg(feature = "cli")]`) is the read–eval loop; `cli.rs` parses the argv that
launches it and the other interactive subcommands. The remaining three are
**shared with the web**: `richtext.rs` is the single producer of display layout,
`display.rs` is the ANSI styler that renders `RichText` to a terminal, and
`render.rs` builds the bracketed match-display string. The session state machine
they drive is [[interactive-session]]; the rewrite arithmetic is
[[interactive-engine]].

## Key entry points

| Symbol | File | Role |
|---|---|---|
| `run_repl` | `repl.rs` | the loop for `alifib repl <file>`: load a `Session`, then read–eval forever |
| `handle_command` / `dispatch_request` / `to_request` | `repl.rs` | route a parsed `Command` to `Session::apply` or a local query, render the reply |
| `ReplHelper` / `make_editor` | `repl.rs` | the `rustyline` helper that paints the `❯` prompt, and the editor factory (vi default, `--emacs`) |
| `ReplArgs`, `ServeArgs`, `WebArgs`, `McpArgs` / `run_repl_cmd`, `run_serve_cmd` | `cli.rs` | parsed argv and dispatchers for the four interactive subcommands |
| `Role`, `Segment`, `RichText`, `RenderKind` | `richtext.rs` | the medium-neutral render tree and view selector |
| `render_kind_for` / `render_response` / `help` | `richtext.rs` | choose a view for a `Request`, produce its `RichText`, build the help |
| `Display` | `display.rs` | terminal sink; the **only** place ANSI codes live |
| `render_step` | `render.rs` | the `(a #0 [idem]) #0 b` bracketed match-display builder |

## Data flow — one keystroke to one screen

```
argv ──parse_repl_args──▶ ReplArgs ──run_repl_cmd──▶ run_repl
                                                       │
   Session::from_disk(file)                            │  [once]
   optional --type/--source/--target ─▶ Request::Start ─▶ Session::apply
                                                       │
        ┌───────────── 'repl loop ───────────────────┐
        rl.readline("❯ ")                             │
        line.split(';') ── each part ──▶ command::parse(part, Cli) ─▶ Command
                                                       │
   handle_command:                                     │
     Quit/Help/PrintFile/Types/Type/Homology  ← served locally
     else  to_request ─▶ Session::apply(req) ─▶ ResponseData
                                                       │
              render_kind_for(req) ─▶ Some(k): show(k, data)   │
                                      None:    print data.message
                                                       │
              show = display.style(render_response(k, data))
```

The loop is **command-buffered**: each input line is split on `;`, and every
semicolon-separated part is parsed and dispatched in turn, so `start C a b ; apply
0 ; show` runs as three commands. `run_repl` keeps a single piece of mutable
state across iterations — the `Session` — plus the `rustyline` editor; everything
else (the store, source, backward flag, active engine/fill) lives inside the
`Session`.

`handle_command` peels off the genuinely CLI-local commands first — `quit`,
`help` (rendered from `richtext::help(false)`), `print` (the running source), and
the read-only queries `types`/`type`/`homology` (served straight from
`session.store()` via the `*_from_store` / `build_homology_data` builders, no
session needed) — and routes **everything else** through `to_request` →
`Session::apply`. `start`/`resume` are built here because they also need the
session's `root_path`; the rest is the shared `Command::to_request`.

## `richtext.rs` — the shared renderer

The single producer that turns a `ResponseData` into a `RichText`: a `Vec` of
`Line`s, each a `Vec<Segment>`, each `Segment` a `(Role, String)`. `Role` names a
*semantic role*, not a colour — `Plain`, `Label`, `Value`, `Src` (input
boundary), `Tgt` (output boundary), `Section`, `Ok`, `Redex` (the matched redex)
— and each medium maps roles to its own styling. `render_kind_for(req)` chooses
the `RenderKind` view a request renders in (`State`, `Auto`, `Rules`, `History`,
`Proof`, `Store`, `Holes`, `Types`, `TypeDetail`, `Homology`), and
`render_response(kind, data)` produces the lines: the active-rewrite state with
its `[bracketed]` matches, a 0-cell fill's candidate list, the open-holes and
constraints listing, type detail, homology, and so on. `help(web)` builds the
command-list `RichText`, dropping the CLI-only rows for the web and adding the
web-only ones. Layout and wording live here once; the CLI and web differ only in
the role→style table.

## `display.rs` — the CLI ANSI styler

`Display { color: bool }`; `Display::new` sets `color` from
`stdout().is_terminal()`, so a redirected pipe gets clean plain text and a TTY
gets ANSI. `Display::style(&RichText)` maps each `Segment`'s `Role` to an ANSI
code (the 16-colour palette mirrors the web frontend's CSS), or to plain text
(with `[…]` brackets for `Redex`) when colour is off — this is the **only** place
in the codebase ANSI codes appear. The thin message methods `meta`/`error`/`file`
/`inspect_rich` print a one-line reply or an already-styled block; the inline
painter `acc` colours the `❯` prompt.

## `render.rs` — the match display

`render_step(step, scope)` walks a rewrite step's input [[core-paste-tree|paste
tree]], wrapping every leaf whose label is the applied rule in `[brackets]` and
chaining composition nodes as `(… #k …)`, matching the house $\#_k$ notation. Its
caller is the protocol's `RewriteInfo.match_display` builder ([[interactive-daemon-web]]),
which `richtext` then re-segments for display. It degrades to `"?"` on a step with
no input tree rather than panicking.

## `cli.rs` — argv

Hand-rolled argv parsing for the four interactive subcommands. Each
`parse_*_args` walks the slice once with a `next_arg` helper for flag values;
unknown `-flags` and repeated positionals are rejected with a usage string.
`parse_repl_args` requires `<file>` and accepts `--type`/`--source`/`--target`/
`--emacs`. `run_repl_cmd` and `run_serve_cmd` dispatch from here; `web`/`mcp`
parse their args here but launch elsewhere ([[web-backends]]).

## Non-obvious invariants and gotchas

- **`repl.rs` is feature-gated.** `#[cfg(feature = "cli")]` on the module — a
  build without the `cli` feature has no terminal REPL. The session core, daemon,
  and web do not depend on it. `cli.rs`, `richtext.rs`, `display.rs`, and
  `render.rs` are always compiled.
- **`Display::new` decides colour once**, at construction from `is_terminal()`;
  `alifib repl … | cat` yields plain text deterministically.
- **The renderer is medium-neutral; only `display` knows ANSI.** Changing a colour
  is one edit in `display.rs`; changing *what is shown* is one edit in
  `richtext.rs`, and it lands on the web at the same time.
- **Read-only queries never reach `Session::apply`.** `handle_command` serves
  `types`/`type`/`homology` from the store directly — they need no session — and
  `Session::apply` would refuse them (*"Not a session command"*) if they did.
- **`to_request` builds `start`/`resume` itself.** Those carry the source path the
  shared `Command::to_request` does not have; for every other command it defers to
  the shared mapping, seeded with the idle `backward` mode.

## Mathematics

This module has no mathematical content of its own — it is the I/O skin over the
session machinery, so its bridge is a **support** relationship. What it surfaces:
[[rewriting]] (the `apply`/`auto`/`random`/`rules`/`proof` commands drive and
display rewrite steps and the assembled proof) and [[diagram]] (everything shown
is a diagram — `render_step` and `render_diagram` from [[output]] turn a
[[core-paste-tree|PasteTree]] into a $\#_k$-chain of labels). The matching and
pushout happen in [[core-matching]] behind [[interactive-engine]]; the session
state these commands mutate is [[interactive-session]]; the non-terminal front
ends that share the same `richtext` renderer are [[interactive-daemon-web]].
