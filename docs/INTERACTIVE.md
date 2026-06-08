# Interactive Rewriting

`alifib` provides several interfaces for constructing (n+1)-dimensional proof
diagrams step by step and for filling the `?` holes of partial maps: a **REPL**
for interactive use, a localhost **web GUI** for notebook-style browser use, a
**daemon** for editor and tooling integration, and an **MCP server** for AI
agents. All four share one command core, so the command set and its behaviour are
identical across them.

---

## REPL

```
alifib repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]
```

Loads `<file>` and enters **no-session** mode: inspection commands work
immediately. Use `start <type> <source> [<target>]` to begin a rewrite
session (target is optional). If `--type` and `--source` are given on the
command line, the session starts automatically.

`--emacs` selects Emacs keybindings; the default is vi mode.

Composite diagram expressions can be quoted with `'` or `"`:

```
start Unit 'split merge' id
```

### Always available

| Command | Description |
|---------|-------------|
| `types` | List all types defined in the file |
| `type <name>` | Inspect a type: generators, diagrams, maps |
| `homology <name>` | Compute cellular homology of a type |
| `start <t> <s> [<g>]` | Start a rewrite session from an initial diagram (target optional) |
| `resume <t> <p> [<g>]` | Resume a session from a diagram `<p>`, replaying its steps (target optional) |
| `holes` | List the open `?` holes (and constraints) of the module's maps |
| `fill <n>` | Start a hole-filling session for hole `<n>` |
| `backward [on\|off]` | Show or toggle backward rewrite mode (when idle) |
| `status` / `show` | Session state, or module path when idle |
| `print` | Print the full source file |
| `stop` | End the active session (or abandon a fill) |
| `help` / `?` | Show command list |
| `quit` / `exit` / `q` | Exit |

### Session commands (require active session)

| Command | Aliases | Description |
|---------|---------|-------------|
| `apply <n> [<n2> ...]` | `a` | Apply rewrite(s) at given indices |
| `auto <n>` | | Apply up to `n` rewrites automatically |
| `random <n>` | | Apply randomly selected rewrites automatically |
| `parallel [on\|off]` | | Show or toggle parallel rewrite mode (default: on) |
| `undo` | `u` | Undo the last step |
| `undo <n>` | `u <n>` | Undo back to step `n` (0 = reset to source) |
| `undo all` / `restart` | | Reset to source diagram |
| `redo` | | Redo the last undone step |
| `redo <n>` | | Redo forward to step `n` |
| `show` / `status` | | Redisplay current diagram and available rewrites |
| `rules` | `r` | List rewrite rules at current dimension |
| `history` | `h` | Show the sequence of moves applied so far |
| `proof` | `p` | Show the running (n+1)-dim proof diagram and its source/target |
| `store <name>` | | Store the current proof as a named diagram in the type |
| `done` | | Finalise a hole-filling session: splice the fill into the map and re-evaluate |
| `save <path>` | | Write the original source file with stored definitions appended |

### Display

Undo preserves a redo buffer: undone steps can be redone until a new rewrite
choice is made, which discards the buffer. There is no need to track the full
tree of histories — only the most recent linear history is kept.

After `start`, and after each `apply`, `undo`, or `redo`, the session state is
printed automatically: the step count, the current diagram, the target (if one
was given), and the available rewrites. There is no separate "applied"
confirmation line — the refreshed state *is* the feedback. Each rewrite shows its
rule and the diagram it would produce, then a `match:` line in which the matched
cells (the redex) are wrapped in brackets:

```
❯ start Unit 'split merge' id
step: 0
current: (split #1 merge)
target: id

available rewrites:
  [0] Split_Merge  (split #1 merge) → id
      match: [Split_Merge]

❯ apply 0
step: 1
current: id
target: id ✓ reached
no rewrites available
```

`match: [Split_Merge]` brackets the whole diagram, so all of `(split #1 merge)`
is the redex; when a rule matches only part of the current diagram the brackets
surround just those cells (e.g. `match: ((A.ob #0 [M.CodId.inv]) #1 M.mor)`).
Composition is written with the house `#k` notation, and `→` separates a rule's
input from its output. `no rewrites available` prints when nothing applies;
`target: … ✓ reached` marks the goal met, at which point `proof` shows the
completed (n+1)-cell:

```
❯ proof
proof : (split #1 merge) → id
  Split_Merge
```

### Storing proofs

`store <name>` stores the current running proof as a named diagram (let-binding)
in the active type. The diagram is visible in `type <name>` for the rest of
the session.

`save <path>` writes the original `.ali` source file with all stored definitions
appended as `@TypeName\nlet name = <expr>` blocks, making them permanent.

### Resuming proofs

`resume <type> <proof> [<target>]` reopens a stored proof as a live session: it
decomposes the diagram into its rewrite steps, applies them all, and lets you
undo, continue, or branch. `<proof>` is any diagram name or expression in the
type. Toggle `backward on` first to run from the proof's output boundary toward
its input; `<target>` is the goal you intend to reach, supplied separately.

### Filling holes

A partial map clause `gen => ?` leaves a generator's image open — a *hole*. A map
may carry holes and still be well-formed; you close them interactively.

`holes` lists every open hole in the module's maps, numbered, each shown as
`?name : in → out` (or just `?name` for a 0-cell), followed by any *constraints*
imposed by conditional pending assignments (the equations `F(x.side) = a.side`).

`fill <n>` opens a filling session for hole `n`. Filling the image of an `m`-cell
`x` of a map `F : D → T` means building `F(x)`:

- for `m ≥ 1` it is an ordinary **rewrite session** from `F(x.in)` to `F(x.out)` —
  the full `apply`/`auto`/`undo`/`proof`/… command set works exactly as in a
  free rewrite;
- for a 0-cell it is the **choice** of one of `T`'s 0-cells (`apply <k>` picks the
  `k`-th candidate; `undo` reopens them).

A hole whose image depends on other unfilled holes cannot be filled until those
are; the REPL reports which to fill first.

`done` finalises the fill: it appends `x => <proof>` to the map's definition and
re-evaluates the file, so the hole is gone and the source is the durable record.
An inconsistent fill is rejected at re-evaluation and the session is kept so you
can retry.

---

## Web GUI

```
alifib web [--bind <addr>]
```

Serves the browser GUI and a same-origin JSON API from one localhost process.
The process owns a single long-lived in-memory session, so this mode is best
thought of as a small notebook kernel rather than a multi-user web app.

If `--bind` is omitted, the server listens on `127.0.0.1:8000`.

Typical SSH-tunneled workflow:

```sh
# on the remote machine
alifib web --bind 127.0.0.1:8000

# on your local machine
ssh -L 8000:127.0.0.1:8000 user@remote-host
```

Then open `http://127.0.0.1:8000` locally.

The web GUI has its own REPL panel. After evaluating a source file, the REPL
enters no-session mode and accepts the same commands as the CLI REPL
(`types`, `type`, `homology`, `start`, `holes`, `fill`, `done`, `stop`, etc.).
One additional command is web-specific:

| Command | Description |
|---------|-------------|
| `clear` | Clear the REPL output (same as the Clear button) |

Sessions can also be started via the GUI controls (type selector + source/target
inputs + Start button), which echoes the equivalent `start` command in the REPL.

The web GUI uses the same rewrite engine and visualization helpers as the WASM
frontend, but keeps the live session state on the server side.

---

## Daemon

```
alifib serve [<file> --type <t> --source <s> [--target <t>]]
```

Runs a JSON-lines server on stdin/stdout. One JSON object per line in each
direction. Suitable for editor integration or AI tooling: spawn as a subprocess
and communicate via its stdio.

If `<file>`, `--type`, and `--source` are provided, the session is pre-loaded
and an initial state response is emitted before the request loop starts.
Otherwise the daemon starts blank and waits for a `start` request.

### Requests

```json
{"command":"load","source_file":"..."}
{"command":"start","source_file":"...","type_name":"...","initial":"...","target":"..."}
{"command":"resume","source_file":"...","type_name":"...","proof":"...","target":"...","backward":false}
{"command":"step","choice":0}
{"command":"undo"}
{"command":"undo_to","step":2}
{"command":"redo"}
{"command":"redo_to","step":4}
{"command":"show"}
{"command":"proof"}
{"command":"list_rules"}
{"command":"history"}
{"command":"holes"}
{"command":"fill","index":0,"backward":false}
{"command":"done"}
{"command":"store","name":"myproof"}
{"command":"save","path":"out.ali"}
{"command":"backward","on":true}
{"command":"types"}
{"command":"type","name":"Unit"}
{"command":"shutdown"}
```

`load` reads a file (and its dependencies) without starting a session — the
loaded-but-idle state from which `holes`/`fill` work. `start` begins a fresh
rewrite from `initial`; `resume` decomposes the diagram `proof` into its rewrite
steps and opens the session with all of them applied. `load`/`start`/`resume`
each (re)read the file from disk, so an edited source is picked up without a
separate reload. `target` is optional in both `start` and `resume`, and
`backward` (default `false`) makes `resume` run from the proof's output boundary
instead of its input. `holes` lists the module's open holes; `fill` starts a
filling session (a rewrite, or a 0-cell choice) and `done` finalises it, returning
the edited `source`. `proof` returns the current proof as a re-parseable
expression (for saving). `load`, `start`, `resume`, and the read-only queries
(`types`/`type`) need no active session; every other command does.

### Responses

Every response is one of:

```json
{"status":"ok","data":{...}}
{"status":"error","message":"..."}
```

The `data` object includes:

| Field | Description |
|-------|-------------|
| `step_count` | Number of active steps applied |
| `can_redo` | Whether undone steps can be redone |
| `current` | Current diagram (`DiagramInfo`) |
| `initial` | Initial diagram (`DiagramInfo`) |
| `target` | Target diagram (omitted if not set) |
| `target_reached` | Whether current equals target |
| `backward` | Whether this is a backward session |
| `rewrites` | Available rewrites — each has `rule_name`, `match_positions`, `match_display`, source/target `DiagramInfo` |
| `proof` | Running proof summary: dim, step_count, source/target labels (omitted if no steps taken) |
| `proof_expr` | The current proof as a re-parseable expression (only in response to `proof`) |
| `history` | Move list (only in response to `history` or `resume`) |
| `rules` | All rewrite rules (only in response to `list_rules`) |
| `types` | Type summaries (only in response to `types`) |
| `type_detail` | Full type info (only in response to `type`) |
| `message` | A canonical one-line result (`Applied …`, `Filled ?x with …`, `Stored …`) |
| `holes` / `constraints` | Open holes / conditional constraints (only in response to `holes`) |
| `fill` | The hole being built — type, map, domain, source, dim (present during a fill) |
| `zero_cell` | Candidate 0-cells and the current pick (present during a 0-cell fill) |
| `source` | The updated running source (in response to `done`/`save`) |
| `module` | The loaded module's path (reported by `show`/`status` when idle) |

`DiagramInfo` has: `label` (space-separated top-level names), `dim`,
`cell_count`, `cells_by_dim` (labels at every dimension from 0 to top).

### Command details

**`store`** — stores the current proof as a named diagram (let-binding) in the
active type. After success, `type` will show the new diagram.

**`holes`** — lists the module's open holes (in `data.holes`) and the constraints
imposed by conditional pending assignments (`data.constraints`), without needing an
active session.

**`fill`** — opens a filling session for the hole at `index` (as numbered by
`holes`). A hole on an `m`-cell with `m ≥ 1` becomes a rewrite session; a 0-cell
hole becomes a choice (`step`/`choice` picks a candidate). `data.fill` identifies
the hole; `data.zero_cell` carries the candidates of a 0-cell fill.

**`done`** — finalises the active fill: splices the proof into the map's
definition, re-evaluates, and returns the edited `source`. An inconsistent fill
errors and the session is kept.

**`types`** — lists all named types in the loaded source file. The `types` array
in the response contains `{name, max_dim, generator_count, diagram_count}` for
each type.

**`type`** — returns full detail for a named type: `generators` (with
optional source/target boundaries), `diagrams` (including stored proofs
with their expressions), and `maps`. Uses the live type complex for the current
session type, so diagrams added via `store` are immediately visible.

---

## MCP server

```
alifib mcp [<examples-dir>]
```

The same engine as the daemon, behind the [Model Context
Protocol](https://modelcontextprotocol.io) so an MCP-aware client (Claude
Desktop, Claude Code, Cursor, …) can discover and call it as tools. The wire
format is newline-delimited JSON-RPC 2.0 over stdin/stdout: logs go to stderr,
stdout carries protocol traffic only. The examples directory (default
`./examples/`) is auto-seeded as virtual `<Name>.ali` modules, so `include
<Name>` resolves in submitted source without shipping the module text.

The handshake is `initialize`, then any number of `tools/list` and `tools/call`
requests. Notifications (no `id`) are not answered.

```jsonc
// → initialize
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
// ← server info + capabilities
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05",
  "capabilities":{"tools":{}},"serverInfo":{"name":"alifib-mcp","version":"0.1.0"}}}

// → load a module, then start a session, then take a step
{"jsonrpc":"2.0","id":2,"method":"tools/call",
  "params":{"name":"load_source","arguments":{"source":"@Type\ninclude TRS,"}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call",
  "params":{"name":"start_session","arguments":{"type_name":"TRS.Unit","initial":"split merge"}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call",
  "params":{"name":"run_command","arguments":{"command":"step","choice":0}}}
```

### Tools

| Tool | Wraps |
|------|-------|
| `load_source` | `load` — parse/interpret source (auto-seeded modules merged in; `modules` overrides them) |
| `start_session` | `start` — begin a rewrite from `initial` (optional `target`, `backward`) |
| `resume_session` | `resume` — reopen a stored `proof` diagram as a live session |
| `run_command` | any other daemon command (see below) |
| `get_types` | the `types` query |
| `get_strdiag` | string-diagram data for a named generator/diagram (optional `boundary_dim`/`boundary_sign`) |
| `get_session_strdiag` | string-diagram data for the active session's current diagram |
| `get_rewrite_preview_strdiag` | the diagram a `choice` would produce, uncommitted |
| `list_examples` | the auto-seeded module list |

`run_command` is the catch-all for the rest of the command core above: the
`arguments` object **is** the command — set `command` to a snake_case name and
supply that command's fields alongside it (`{"command":"step","choice":0}`,
`{"command":"undo"}`, `{"command":"fill","index":0}`). The interactive
hole-filling workflow lives here — `{"command":"holes"}` →
`{"command":"fill","index":n}` → `{"command":"done"}` — exactly as in the daemon.
`start`/`resume`/`load` are *not* accepted through `run_command` (they need a
file path the kernel does not have in this mode); use the `start_session` /
`resume_session` / `load_source` tools instead. An escape hatch
`{"command_json":"<raw>"}` forwards an arbitrary JSON body verbatim.

### Results

Each `tools/call` returns a single text content block whose body is the **same
JSON envelope the daemon returns** (`{"status":"ok","data":{…}}` or
`{"status":"error","message":…}`) — so every `data` field documented under
[Responses](#responses) applies unchanged. An error envelope is additionally
flagged with MCP's `isError: true`, so a client can branch on it without parsing
the body.

```jsonc
{"jsonrpc":"2.0","id":4,"result":{
  "content":[{"type":"text","text":"{\"status\":\"ok\",\"data\":{…}}"}],
  "isError":false}}
```

---

## Persistence

A session has no file format of its own. Its durable form is the diagram it is
building — the `(n+1)`-cell assembled from the steps applied so far.

- **Save** — `store <name>` registers the running proof as a named diagram in
  the active type; `save <path>` (REPL) writes the `.ali` source with those
  definitions appended. The proof is then an ordinary diagram in the source.
- **Reopen** — `resume` decomposes such a diagram back into its rewrite steps,
  with every step applied, so you can undo, continue, or branch from it. A
  forward session resumes from the diagram's input boundary, a `backward` one
  from its output; the target is supplied separately (it is the goal, not
  inferred from the diagram).

So a session round-trips entirely through `.ali` — there is no separate
session-file format.
