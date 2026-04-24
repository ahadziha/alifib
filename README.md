# alifib

An interpreter for a language of pasting diagrams in directed complexes,
based on Amar Hadzihasanovic's theory of molecules.

## Overview

`alifib` lets you define algebraic structures using higher-dimensional
generators and diagram composition, check that they are well-formed, and assert
equations between them. A *regular directed complex* is the shape of a pasting
diagram; a *molecule* is a shape built inductively by gluing atoms. The
interpreter elaborates type definitions, checks boundaries, resolves included
modules, and verifies that partial maps are structure-preserving.

## Interfaces at a glance

| Command | What it is | See |
|---------|------------|-----|
| `alifib <file>` | One-shot interpret / `--ast` / `--print` / `--bench` | [Interpreter](#interpreter) |
| `alifib repl <file>` | Interactive terminal REPL with readline, history, `store`/`save` | [REPL](#repl) |
| `alifib serve` | JSON-lines daemon on stdin/stdout for editor plugins and agents | [Daemon](#daemon) |
| `alifib web [<dir>]` | Localhost HTTP server + browser GUI (SSH-tunnel friendly) | [Web GUI](#web-gui) |
| Static WASM build | Same frontend, interpreter in the browser (GitHub Pages etc.) | [Local preview](#local-preview) |

## Language

Source files use the `.ali` extension. The core constructs are:

**Type blocks** — declare algebraic structures with generators:

```ali
@Type
Ob <<= {
  pt,
  ob : pt -> pt
},

Magma <<= {
  attach Ob :: Ob,
  m : Ob.ob Ob.ob -> Ob.ob
}
```

A bare name like `pt` declares a 0-cell. `name : src -> tgt` declares a cell
with a source and target diagram. `attach T :: S` imports a copy of type `S`
under the name `T`.

**Partial maps** — structure-preserving assignments between types, written with
`along [ gen => diagram, ... ]`. These are used by `attach` to identify
generators across types.

**Diagram expressions** — vertical composition is written by juxtaposition
(`f g`), horizontal composition by `f #0 g` (pasting along a 0-cell boundary),
and grouping with parentheses.

**Modules** — `.ali` files can include other files. The interpreter resolves the
full dependency graph before elaboration. Set `ALIFIB_PATH` to a
colon-separated list of directories to search for included files.

See `examples/` for more, and `docs/grammar.md` for the full grammar.

## Building

The repository is a Cargo workspace. The default target is the `alifib` CLI
binary — everything else (web server, wasm bindings, plugins) is opt-in.

```
cargo build --release              # builds the `alifib` CLI binary
cargo test                         # runs the CLI + library tests
cargo build --release --workspace  # also builds plugins and the web server
```

The WebAssembly crate at `web/wasm/` is intentionally outside the workspace
and builds separately through `wasm-pack` (see [Web GUI](#web-gui) below).

## Usage

### Interpreter

```
alifib <input.ali> [-o <output.ali>] [--ast] [--bench N]
```

- `-o / --output` — write output to a file instead of stdout
- `--ast` — print the parsed AST instead of interpreting
- `--bench N` — run N times and print average wall time in milliseconds

```
cargo run --release -- examples/Frobenius.ali
cargo run --release -- examples/YangBaxter.ali
```

### REPL

An interactive terminal session for building proof diagrams step by step.

```
alifib repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]
```

Example:

```
alifib repl examples/Idem.ali --type Idem --source lhs --target rhs
```

The REPL has two phases:

- **Setup** — select a type (`@ Idem`), then set `source` and `target` diagram
  names. When all three are set the session starts automatically.
- **Rewriting** — apply rewrites with `apply <n>`, undo with `undo`, inspect
  the running proof with `proof`, name and store it with `store <name>`.

Key commands:

| Command | Description |
|---------|-------------|
| `@ <type>` | Select a type |
| `source <name>` / `target <name>` | Set source and target |
| `apply <n>` | Apply rewrite at index `n` (alias `a`) |
| `undo` | Undo last step (alias `u`) |
| `rules` | List available rewrite rules (alias `r`) |
| `proof` | Show the running proof diagram (alias `p`) |
| `store <name>` | Register proof as a first-class generator |
| `save <path>` | Write source file with stored definitions appended |
| `help` | Full command list |

See `INTERACTIVE.md` for the complete reference.

### Daemon

A JSON-lines subprocess server for editor and AI integration.

```
alifib serve [<file> --type <t> --source <s> [--target <t>]]
```

One JSON object per line in each direction on stdin/stdout. Example:

```sh
echo '{"command":"init","source_file":"examples/Idem.ali","type_name":"Idem","source_diagram":"lhs"}' \
  | alifib serve
```

Key requests:

```json
{"command":"init","source_file":"...","type_name":"...","source_diagram":"..."}
{"command":"step","choice":0}
{"command":"undo"}
{"command":"store","name":"myproof"}
{"command":"types"}
{"command":"type","name":"Idem"}
{"command":"cell","name":"idem"}
{"command":"shutdown"}
```

Every response is `{"status":"ok","data":{...}}` or `{"status":"error","message":"..."}`.
The `data` object always includes the current session state (step count, current diagram,
available rewrites). Informational commands add extra fields: `types`, `type_detail`, `cell_detail`.

See `INTERACTIVE.md` for the full protocol reference.

### Web GUI

A localhost-only browser GUI, intended for SSH-tunneled use in the style of a
small notebook server.

```
alifib web [<examples-dir>] [--bind <addr>]
```

This serves the files in `web/frontend` and runs a single long-lived Alifib
session in the server process. By default it binds to `127.0.0.1:8000` and
scans `./examples/` for `.ali` files. The scan is re-done on every request,
so editing or adding `.ali` files on disk shows up without restarting.

Typical remote workflow:

```sh
# on the remote machine
alifib web --bind 127.0.0.1:8000

# on your local machine
ssh -L 8000:127.0.0.1:8000 user@remote-host
```

Then open `http://127.0.0.1:8000` in your local browser.

The editor supports live syntax highlighting, loading/saving `.ali` files from
your local machine, and a dropdown listing every `.ali` file in the
examples directory. Files there are also importable as modules — any
`include <Name>` in the editor is resolved against the same directory.

Subdirectories under `<examples-dir>` are allowed for organisation, but the
module name is always the file's bare stem: `topics/braided/YangBaxter.ali`
is `include YangBaxter`. Two files sharing a stem (case-insensitively)
anywhere in the tree is a loud error — the server surfaces it on
`/examples/index.json` and the deploy workflow fails the build, so you
find out at scan time instead of via silent shadowing later.

#### Local preview

The frontend assets (`index.html`, `app.js`, `style.css`) are embedded into
the binary at compile time, so there is no separate build step for the HTTP
mode:

```sh
just web                         # scans ./examples/ by default
just web docs/teaching-deck       # or any other directory
just web --bind 127.0.0.1:8080   # pass-through args
```

For the WASM-backed deployment (used when `web/frontend/` is hosted as
static files — e.g. GitHub Pages), build the bundle and mirror the examples
directory:

```sh
just web-wasm                    # wasm-pack + copy examples/ into web/frontend/
just web-static [port]           # python3 -m http.server (default 8000)
```

`just web-wasm` populates `web/frontend/examples/` with the repo's `.ali`
files and writes an `examples/index.json` manifest.  Both the HTTP mode and
the WASM deployment expose the same `examples/index.json` + `examples/Name.ali`
URL scheme, so the frontend fetches them identically in either environment.

## Repository layout

```
src/           alifib library crate (language/, core/, interpreter/, output/)
cli/           alifib-cli crate — produces the `alifib` binary
web/
  frontend/    Browser frontend (index.html, app.js, style.css)
  shared/      alifib-web-shared crate — runtime example-directory scanner
  server/      alifib-web-server crate — localhost HTTP server for `alifib web`
  wasm/        alifib-wasm crate — WebAssembly bindings (built via wasm-pack)
examples/      Example .ali files (served by `alifib web` at runtime)
docs/          Grammar, interpreter description, formal semantics (LaTeX)
plugins/trs/   Plugin: convert term rewriting systems to alifib
INTERACTIVE.md Full reference for the REPL, web GUI, and daemon
```
