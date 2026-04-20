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

```
cargo build --release
```

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
alifib web [--bind <addr>]
```

This serves the files in `web/frontend` and runs a single long-lived Alifib
session in the server process. By default it binds to `127.0.0.1:8000`.

Typical remote workflow:

```sh
# on the remote machine
alifib web --bind 127.0.0.1:8000

# on your local machine
ssh -L 8000:127.0.0.1:8000 user@remote-host
```

Then open `http://127.0.0.1:8000` in your local browser.

### Session workspace

```
alifib session <file> --type <t>
```

An interactive session for building up definitions incrementally within a type.
Unlike the REPL, no initial source diagram is required. Use `goal <name> : <src> -> <tgt>`
to enter a guided proof sub-loop, then `export` or `export <path>` to save results.

## Repository layout

```
src/           Interpreter source (language/, core/, interpreter/, output/)
web/           Browser frontend and WASM bindings
examples/      Example .ali files
docs/          Grammar, interpreter description, formal semantics (LaTeX)
trs/           Plugin: convert term rewriting systems to alifib (see trs/README.md)
INTERACTIVE.md Full reference for the REPL, web GUI, daemon, and session workspace
```
