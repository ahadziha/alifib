---
kind: impl
status: stable
last-touched: 2026-06-09
code: [cli/src/main.rs, cli/Cargo.toml]
---

# cli — the `alifib` binary

> The workspace's default binary (`default-members = ["cli"]`). One file,
> `cli/src/main.rs`, hand-parses argv into a `RunMode` and dispatches to the
> right entry point: interpret a file, dump its AST, pretty-print it, benchmark
> a reload, or hand off to one of the four interactive front ends. The
> mathematics lives elsewhere; this crate is the doorway.

## What it owns

The `alifib-cli` package builds the binary named **`alifib`** (the package is
renamed only because Cargo demands unique workspace names; see `cli/Cargo.toml`).
It owns argument parsing for the *batch* modes and the top-level subcommand
demux; it owns no domain logic. It depends on the `alifib` library with the
`cli` feature on (`alifib = { path = "..", features = ["cli"] }`), which is what
pulls in `rustyline` and compiles the REPL — see `Cargo.toml` `cli = ["dep:rustyline"]`.
It also depends on the three web workspace crates (`alifib-web-mcp`,
`alifib-web-server`, `alifib-web-shared`) so that the `web` and `mcp`
subcommands can launch their servers.

**Distinct from `src/interactive/cli.rs`.** That in-library module
(documented by [[interactive-repl]]) holds the `ReplArgs`/`ServeArgs`/`WebArgs`/
`McpArgs` parsers and the `run_repl_cmd`/`run_serve_cmd` dispatchers. This crate
*imports* and *calls* those — it parses only its own batch flags and the
subcommand keyword, then delegates everything interactive to the library.

## Key public symbols

| Symbol | Role |
|---|---|
| `main` | parse argv, build a `Loader`, dispatch on `RunMode`, `exit(1)` on error |
| `parse_args` *(internal)* | argv → `Args`; peels off `repl`/`web`/`mcp`/`serve` subcommands first, else walks the rest for batch flags |
| `RunMode` *(internal)* | the mode enum: `Interpret`, `Ast`, `Print`, `Bench(n)`, `Repl`/`Web`/`Mcp`/`Serve(args)` |
| `run_interpreter` *(internal)* | default mode: load + interpret a file, write the elaborated rendering (holes appear inline) |
| `run_ast` / `run_print` *(internal)* | parse-only modes: dump `Program::to_string` or `language::print_program` |
| `run_bench` *(internal)* | reload the file `n` times, print mean ms/reload |
| `run_web_cmd` / `run_mcp_cmd` *(internal)* | construct an `ExampleSet` and start the web / MCP server |
| `USAGE` *(internal)* | the one usage string: stdout on `-h`/`--help`, stderr when the input file is missing |

## Data flow

```
argv ──parse_args──▶ Args { input, output, mode: RunMode }
                          │
   subcommand?  repl / web / mcp / serve ─▶ parse_*_args (src/interactive/cli.rs)
                          │                       └─▶ run_repl_cmd / run_serve_cmd
                          │                            run_web_cmd / run_mcp_cmd
                          │
   else batch flags: -o/--output, --ast, --print, --bench N
                          │
   Loader::default(vec![]) ─┐
                          ▼
   Ast     ─▶ load_only_root ─▶ Program::to_string  ─┐
   Print   ─▶ load_only_root ─▶ language::print_program │─▶ write_output
   Interpret ─▶ InterpretedFile::load ─▶ .into_result ──┘   (path → fs::write, else println)
   Bench(n)  ─▶ InterpretedFile::load ×n ─▶ mean ms
```

The default mode (no subcommand, no `--ast`/`--print`/`--bench`) is
`Interpret`: `run_interpreter` calls `InterpretedFile::load(loader, input)`
([[interpreter]]) and writes the elaborated file (`file.to_string()`). Any
unfilled map [[hole|holes]] appear inline in that rendered output ([[output]]);
there is no separate hole-reporting pass.

## Subcommands and modes

| Invocation | Mode | Entry point |
|---|---|---|
| `alifib <file> [-o <out>]` | interpret + elaborate | `run_interpreter` → [[interpreter]] |
| `alifib <file> --ast` | dump parsed `Program` | `run_ast` → [[language-parser]] |
| `alifib <file> --print` | pretty-print | `run_print` → [[language-parser]] |
| `alifib <file> --bench N` | time `N` reloads | `run_bench` |
| `alifib repl <file> …` | live rewrite REPL | `run_repl_cmd` → [[interactive-repl]] |
| `alifib serve […]` | line-protocol daemon | `run_serve_cmd` → [[interactive-daemon-web]] |
| `alifib web [<dir>] [--bind]` | HTTP server | `run_web_cmd` → [[interactive-daemon-web]] |
| `alifib mcp [<dir>]` | MCP server | `run_mcp_cmd` → [[interactive-daemon-web]] |

`-o`/`--output` redirects the `Interpret`/`Ast`/`Print` text to a file (default
stdout); `--bench` ignores it and always prints the mean to stdout. `-h`/`--help`
prints `USAGE` and exits 0 (in the batch flag walk — `alifib repl --help` is the
library parser's business). A missing input file makes `USAGE` the error message
(stderr, exit 1); an unknown flag gets its own `Unknown option` message.

## Non-obvious invariants and gotchas

- **Subcommands are matched on `cli_args.first()`, before any flag walk.** If
  argv starts with `repl`/`web`/`mcp`/`serve`, `parse_args` hands the *rest* of
  the slice to the corresponding `parse_*_args` and returns immediately. So
  batch flags like `--ast` are only recognised in the no-subcommand path.
- **The interactive arg parsers live in the library, not here.** `main.rs`
  re-exports nothing; it imports `parse_repl_args` et al. from
  `alifib::interactive::cli`. Argument-parsing bugs for `repl`/`serve` belong to
  [[interactive-repl]], not this page.
- **`web`/`mcp` examples dir defaults to `"examples"`.** Both `run_web_cmd` and
  `run_mcp_cmd` do `args.examples_dir.unwrap_or_else(|| "examples".to_string())`
  relative to cwd.
- **`--bench` loads once, then times `n` reloads.** `run_bench` does a first
  `InterpretedFile::load(...).into_result()?` (so a load error aborts before
  timing) and then reloads the file `n` more times, printing the mean ms/reload.
  The timed reloads discard their results (`let _ =`) — an error mid-benchmark
  is silently timed, not reported. Files with unfilled holes elaborate fine and
  are timed like any other reload.
- **Batch mode flags are last-one-wins.** `--ast`, `--print` and `--bench N`
  each overwrite `mode`; `alifib f.ali --ast --print` pretty-prints.
- **A single `Loader::default(vec![])` is built in `main`** (no extra search
  paths) and shared by every batch mode; the interactive modes build their own
  loaders downstream.
- **Exit code is binary.** Every `run_*` returns `Result<(), ()>`; `main`
  `exit(1)`s on any `Err`, and the error text was already printed by the callee
  (`report_load_file_error`, `eprintln!`, or the library reporter).

## Mathematics

This crate has no mathematical content of its own — it is the process boundary
that *selects* which piece of alifib the user reaches. The bridge is a
**surfacing** relationship:

- [[module-system]] — every batch mode begins by loading a `.ali` file (the
  unit of the module system) through the `Loader`; `--ast`/`--print` expose the
  parsed [[language-parser|program]], the default mode elaborates it.
- [[rewriting]] — the `repl` and `serve` subcommands open a live rewrite
  session; `web`/`mcp` expose the same engine over HTTP / MCP.
- [[hole]] — the elaborated file lists any unfilled map holes inline; the
  interactive `fill` workflow ([[interactive-session]]) closes them.

See [[interpreter]] for the default elaboration path, [[interactive-repl]] for
the REPL and for the in-library arg parsers this binary calls, and
[[interactive-daemon-web]] for the `serve`/`web`/`mcp` servers.
