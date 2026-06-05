# alifib

> *A programming language where a program and its execution are terms in one
> syntax.*

**alifib** is an experimental programming language and interactive proof
assistant founded on *higher-dimensional rewriting*. Its types are not bags of
terms but **directed cell complexes** — spaces assembled from directed cells of
every dimension. A value is a **diagram** drawn in such a space; and — the idea
at the heart of the language — *a computation is itself a diagram, one dimension
up*. Running a program does not merely return an answer: it builds a **witness of
the whole computation**, a higher-dimensional term you can inspect, paste into
larger diagrams, and reason about.

We call this **computational transparency**. In an ordinary language the rules by
which terms reduce are meta-theoretical — they live in the compiler, outside the
language. In alifib the computation rules of a type are simply **extra generators
of that type**. So: well-typed terms are verified *programs*; well-typed *higher*
terms are verified *executions*.

There is a deeper shift underneath. Classical syntax represents a program as a
*term* — a tree. alifib represents it as a *diagram*, the natural generalisation
of a term once you leave the world of trees: **string diagrams** generalise terms
to non-cartesian settings (where you may not freely copy or discard), and
**pasting diagrams** generalise string diagrams to higher dimensions. The whole
language is built on a single combinatorial object — the diagram — and a single
operation on it: searching for one diagram inside another.

> **The full story** — what alifib is for, where it comes from, and the vision
> behind it — is in [`docs/CONCEPTS.md`](docs/CONCEPTS.md).
> **Try it in your browser**, no install: **<http://compose.ee/alifib>**.

## What it is for

A single type can be read in several ways at once — as a presentation of a
higher-categorical structure, as a rewrite system, and as a topological cell
complex — so one small language covers strikingly different ground:

- **Proof-relevant equational reasoning** in higher-categorical structures
  (monoidal categories, bicategories, …): equations are *directed* cells, and a
  proof is a diagram you build interactively. See `examples/EckmannHilton.ali`.
- **A metalanguage for abstract machines** defined by rewriting — Turing
  machines, term-rewriting systems, automata — where the *execution trace* is a
  first-class term, not an ephemeral side effect. See `examples/TM.ali`,
  `examples/BinaryNat.ali`.
- **Building finite cell complexes** and computing their invariants, e.g. the
  `homology` command in the REPL. See `examples/Delta_complexes.ali`.

## Background

The mathematics is Amar Hadzihasanovic's theory of **directed complexes** and
**molecules** — developed in his book *Combinatorics of higher-categorical
diagrams*, building on foundations laid by Richard Steiner. The data structures
and algorithms grew out of joint work with **Diana Kessler**; the
higher-categorical semantics (a model of (∞, n)-categories carried by directed
complexes) from joint work with **Clémence Chanavat**. alifib is developed as part
of [ARIA](https://www.aria.org.uk/)'s *Safeguarded AI* programme; the interpreter
and proof assistant are joint work with **Alex Kavvos** (University of Bristol),
with contributions from **Wessel de Weijer**.

The name is borrowed from *Alifib*, a song on Robert Wyatt's 1974 album
*Rock Bottom*.

> ⚠️ **Status: experimental research software.** The language, syntax, and
> interfaces are evolving; expect rough edges.

## In one paragraph, concretely

`alifib` lets you define structures using higher-dimensional generators and
pasting, check that they are well-formed, and assert (directed) equations between
them. A *molecule* is the shape of a pasting diagram, built inductively by pasting
*atoms* (the shapes of single cells); a *directed complex* — the shape of a type —
is a more general finite complex of such cells, not necessarily a single pasting. The
interpreter elaborates type definitions, checks boundaries, resolves included
modules, and verifies that maps between types are structure-preserving. The rest
of this README is the practical reference.

## Interfaces at a glance

| Command | What it is | See |
|---------|------------|-----|
| `alifib <file>` | One-shot interpret / `--ast` / `--print` / `--bench` | [Interpreter](#interpreter) |
| `alifib repl <file>` | Interactive terminal REPL with readline, history, `store`/`save` | [REPL](#repl) |
| `alifib serve` | JSON-lines daemon on stdin/stdout for editor plugins and agents | [Daemon](#daemon) |
| `alifib mcp [<dir>]` | Model Context Protocol server exposing the engine as tools for AI agents | [Web GUI](#web-gui) |
| `alifib web [<dir>]` | Localhost HTTP server + browser GUI (SSH-tunnel friendly) | [Web GUI](#web-gui) |
| Static WASM build | Same frontend, interpreter in the browser (GitHub Pages etc.) | [Local preview](#local-preview) |

## Language

Source files use the `.ali` extension. The core constructs are:

**Type blocks** — declare structures with generators. This is the setup for the
classic *Eckmann–Hilton argument*: a bicategory with one object, no non-identity
1-cells, and two 2-cells `a`, `b` of its identity (`examples/EckmannHilton.ali`):

```ali
@Type
include Bicategory,
let Object    = Bicategory.Object,
let 2Morphism = Bicategory.2Morphism,
let Equation  = Bicategory.Equation,

EckmannHilton <<= {
    pt,
    attach Pt :: Object along [ ob => pt ],

    a: Pt.id -> Pt.id,
    b: Pt.id -> Pt.id,

    attach A :: 2Morphism along [ Src => Pt.Id, Tgt => Pt.Id, 2mor => a ],
    attach B :: 2Morphism along [ Src => Pt.Id, Tgt => Pt.Id, 2mor => b ]
}
```

A bare name like `pt` declares a **0-cell**; `name : src -> tgt` declares a higher
cell with source and target diagrams (`a` and `b` are 2-cells — endomorphisms of
the identity on `pt`). `include M` pulls in another module and `let X = M.Y` makes
a local alias. `attach T :: S along [ g => d, ... ]` glues in a copy of type `S`
under the name `T`, the `along` clause identifying `S`'s generators with local
diagrams — here `attach Pt :: Object along [ ob => pt ]` makes `pt` play the role
of the object in a copy of `Object`, so that `Pt.id` is its identity 1-cell.

**Maps** — assignments from the generators of one type to diagrams in another,
written with the same `along [ gen => diagram, ... ]` syntax that `attach` uses
above. Continuing the example, the Eckmann–Hilton *goal* is a map into
`Bicategory.Equation` asserting that `a b` and `b a` are equal:

```ali
@EckmannHilton
let total Commutativity :: Equation = [
    lhs => a b,
    rhs => b a,
    dir => ?,
    inv => ?
]
```

**Holes** — the `dir => ?` and `inv => ?` clauses leave those images open: each is
a *hole*. A map may carry holes and still be `total` (the generator is covered,
its image merely pending). Some holes are inferred once the rest of the map is
fixed; the others are filled interactively in the REPL (`holes` / `fill <n>` /
`done`) — here the two holes *are* the proof that `a` and `b` commute, built by
hand. See also `examples/Hole_examples.ali`.

**Diagram expressions** — diagrams are built from generators by **pasting**, which
combines cells into a larger diagram. (Pasting never reduces a diagram to a single
cell; that would be *composition*, a higher-algebraic operation that plain alifib
types do not have.) `f #k g` pastes `f` and `g` along their shared `k`-dimensional
boundary; juxtaposition `f g` is *principal pasting*, shorthand for `f #k g` at
`k = min(dim f, dim g) - 1`; parentheses group.

**Modules** — `.ali` files can include other files. The interpreter resolves the
full dependency graph before elaboration. An `include <Name>` is resolved from the
including file's own directory, then a same-named subdirectory (so `Foo.ali` can
keep private submodules in a `Foo/` directory and `include` them by name), then
the directories in `ALIFIB_PATH` (a colon-separated list); the closest match wins.

See `examples/` for more, and `docs/GRAMMAR.md` for the full grammar.

## Building

The repository is a Cargo workspace. The default target is the `alifib` CLI
binary — everything else (web server, wasm bindings) is opt-in.

```
cargo build --release              # builds the `alifib` CLI binary
cargo test                         # runs the CLI + library tests
cargo build --release --workspace  # also builds the web server
```

The WebAssembly crate at `web/wasm/` is intentionally outside the workspace
and builds separately through `wasm-pack` (see [Web GUI](#web-gui) below).

## Usage

### Interpreter

```
alifib <input.ali> [-o <output.ali>] [--ast] [--print] [--bench N]
```

- `-o / --output` — write output to a file instead of stdout
- `--ast` — print the parsed AST instead of interpreting
- `--print` — pretty-print the source (re-emit the parsed program) instead of interpreting
- `--bench N` — run N times and print average wall time in milliseconds

```
cargo run --release -- examples/Monoidal.ali
cargo run --release -- examples/TRS.ali
```

### REPL

An interactive terminal session for building proof diagrams step by step.

```
alifib repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]
```

Example:

```
alifib repl examples/TRS.ali --type Unit --source 'split merge' --target id
```

After loading, the REPL is in **no-session** mode: inspection commands like
`types`, `type <name>`, `homology <name>`, and `holes` work immediately. Use
`start <type> <source> [<target>]` to begin a rewrite session (target is
optional), `resume <type> <proof> [<target>]` to reopen a stored proof
diagram as a live session, or `fill <n>` to start filling an open `?` hole.
Composite diagram expressions can be quoted:

```
start Unit 'split merge' id
```

If `--type` and `--source` are given on the command line, the session starts
automatically.

Key commands:

| Command | Description |
|---------|-------------|
| `start <t> <s> [<g>]` | Start a rewrite session (target optional) |
| `resume <t> <p> [<g>]` | Resume a session from a stored proof diagram (target optional) |
| `holes` | List the open `?` holes of the module's maps |
| `fill <n>` | Start a hole-filling session for hole `n` (a rewrite, or a 0-cell choice) |
| `apply <n>` | Apply rewrite at index `n` (alias `a`) |
| `undo [<n>]` | Undo last step, or back to step `n` (alias `u`) |
| `redo [<n>]` | Redo last undone step, or forward to step `n` |
| `rules` | List available rewrite rules (alias `r`) |
| `proof` | Show the running proof diagram (alias `p`) |
| `done` | Finalise a hole-filling session, splicing the fill into the map |
| `store <name>` | Store the current proof as a named diagram |
| `save <path>` | Write source file with stored definitions appended |
| `stop` | End the active session |
| `help` | Full command list |

See `docs/INTERACTIVE.md` for the complete reference.

### Daemon

A JSON-lines subprocess server for editor and AI integration.

```
alifib serve [<file> --type <t> --source <s> [--target <t>]]
```

One JSON object per line in each direction on stdin/stdout. Example:

```sh
echo '{"command":"start","source_file":"examples/TRS.ali","type_name":"Unit","initial":"split merge"}' \
  | alifib serve
```

Key requests:

```json
{"command":"start","source_file":"...","type_name":"...","initial":"..."}
{"command":"resume","source_file":"...","type_name":"...","proof":"..."}
{"command":"step","choice":0}
{"command":"undo"}
{"command":"redo"}
{"command":"proof"}
{"command":"holes"}
{"command":"fill","index":0}
{"command":"done"}
{"command":"store","name":"myproof"}
{"command":"types"}
{"command":"type","name":"Unit"}
{"command":"cell","name":"merge"}
{"command":"shutdown"}
```

Every response is `{"status":"ok","data":{...}}` or `{"status":"error","message":"..."}`.
The `data` object always includes the current session state (step count, current diagram,
available rewrites). Informational commands add extra fields: `types`, `type_detail`,
`cell_detail`, `holes`/`constraints` (the `holes` command), and `fill`/`zero_cell`
(during a hole-filling session). All three front-ends — REPL, daemon, web — share one
command core, so the command set and responses are identical across them.

See `docs/INTERACTIVE.md` for the full protocol reference.

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
examples directory. Files there are also importable as modules, resolved by the
same convention the interpreter uses (see [Modules](#language) above).

Subdirectories under `<examples-dir>` are traversed recursively. Each example's
name is its relative path minus the `.ali` suffix — `Theory` for `Theory.ali`,
`TRS/Aux` for `TRS/Aux.ali` — which is both its dropdown label and its key in
`/examples/index.json`. Every path segment must be a valid identifier
(`[A-Za-z_][A-Za-z0-9_]*`); other files are skipped with a warning. Because a
module resolves its `include`s from its own same-named subdirectory first, the
same stem may recur under different modules — `Monoidal/Aux.ali`,
`Bicategory/Aux.ali`, `TRS/Aux.ali` — without clashing, each module seeing its own.

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
  mcp/         alifib-web-mcp crate — Model Context Protocol server (`alifib mcp`)
  wasm/        alifib-wasm crate — WebAssembly bindings (built via wasm-pack)
editors/       Editor integrations (e.g. VS Code syntax highlighting)
examples/      Example .ali files (served by `alifib web` at runtime)
docs/          CONCEPTS (the vision), grammar, homology, interactive & testing guides
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE)), or
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
