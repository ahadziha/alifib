# Interactive Rewriting

`alifib` provides several interfaces for constructing (n+1)-dimensional proof
diagrams step by step: a **REPL** for interactive use, a localhost **web GUI**
for notebook-style browser use, and a **daemon** for editor and tooling
integration.

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
start Idem 'id id id' id
```

### Always available

| Command | Description |
|---------|-------------|
| `types` | List all types defined in the file |
| `type <name>` | Inspect a type: generators, diagrams, maps |
| `homology <name>` | Compute cellular homology of a type |
| `start <t> <s> [<g>]` | Start a rewrite session from an initial diagram (target optional) |
| `resume <t> <p> [<g>]` | Resume a session from a diagram `<p>`, replaying its steps (target optional) |
| `status` / `show` | Session state, or module path when idle |
| `print` | Print the full source file |
| `stop` | End the active session |
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
| `save <path>` | | Write the original source file with stored definitions appended |

### Display

Undo preserves a redo buffer: undone steps can be redone until a new rewrite
choice is made, which discards the buffer. There is no need to track the full
tree of histories — only the most recent linear history is kept.

After each `apply`, `undo`, or `redo`, the current diagram and available rewrites are
printed automatically. Bracket notation shows where in the current diagram each
rule matches:

```
> apply 0
Applied idem.

[1] id id

rewrites:
  [0] idem : [id id]  ->  id
```

The brackets `[id id]` indicate which top-dimensional cells are covered by the
match.

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
(`types`, `type`, `homology`, `start`, `stop`, etc.). Two additional commands
are web-specific:

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
{"command":"store","name":"myproof"}
{"command":"types"}
{"command":"type","name":"Idem"}
{"command":"shutdown"}
```

`start` begins a fresh session from `initial`; `resume` decomposes the diagram
`proof` into its rewrite steps and opens the session with all of them applied.
`target` is optional in both, and `backward` (default `false`) makes `resume`
run from the proof's output boundary instead of its input. `proof` returns the
current proof as a re-parseable expression (for saving). All commands except
`start` and `resume` require a session to be active.

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

`DiagramInfo` has: `label` (space-separated top-level names), `dim`,
`cell_count`, `cells_by_dim` (labels at every dimension from 0 to top).

### Command details

**`store`** — stores the current proof as a named diagram (let-binding) in the
active type. After success, `type` will show the new diagram.

**`types`** — lists all named types in the loaded source file. The `types` array
in the response contains `{name, max_dim, generator_count, diagram_count}` for
each type.

**`type`** — returns full detail for a named type: `generators` (with
optional source/target boundaries), `diagrams` (including stored proofs
with their expressions), and `maps`. Uses the live type complex for the current
session type, so diagrams added via `store` are immediately visible.

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
