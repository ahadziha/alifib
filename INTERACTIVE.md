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

Loads `<file>`. If `--type`, `--source`, and `--target` are all supplied, the
session starts immediately. Otherwise the REPL enters a setup phase where you
set them interactively before rewriting begins.

`--emacs` selects Emacs keybindings; the default is vi mode.

### Setup commands (always available)

| Command | Description |
|---------|-------------|
| `@ <type>` | Select a type from the loaded file |
| `types` | List all types defined in the file |
| `type <name>` | Inspect a type: generators, diagrams, maps |
| `homology <name>` | Compute cellular homology of a type |
| `source <name>` | Set the source diagram (starts session when all three are set) |
| `target <name>` | Set the target diagram (starts session when all three are set) |
| `status` / `show` | Show setup state (module, type, pending source/target) |
| `print` | Print the full source file |
| `rules` | List generators in the selected type |
| `clear` | Destroy engine and type selection, return to setup |
| `help` / `?` | Show command list |
| `quit` / `exit` / `q` | Exit |

### Rewriting commands (require active session)

| Command | Aliases | Description |
|---------|---------|-------------|
| `apply <n>` | `a <n>` | Apply rewrite at index `n` |
| `undo` | `u` | Undo the last step |
| `undo <n>` | `u <n>` | Undo back to step `n` (0 = reset to source) |
| `undo all` / `restart` | | Reset to source diagram |
| `show` / `status` | | Redisplay current diagram and available rewrites |
| `rules` | `r` | List rewrite rules at current dimension |
| `history` | `h` | Show the sequence of moves applied so far |
| `proof` | `p` | Show the running (n+1)-dim proof diagram and its source/target |
| `store <name>` | | Store the current proof as a named diagram in the type |
| `save <path>` | | Write the original source file with stored definitions appended |
| `load <path>` | `l <path>` | Load and replay a session file, replacing current state |

### Display

After each `apply` or `undo`, the current diagram and available rewrites are
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
Otherwise the daemon starts blank and waits for an `init` request.

### Requests

```json
{"command":"init","source_file":"...","type_name":"...","source_diagram":"...","target_diagram":"..."}
{"command":"resume","session_file":"..."}
{"command":"step","choice":0}
{"command":"undo"}
{"command":"undo_to","step":2}
{"command":"show"}
{"command":"save","path":"..."}
{"command":"list_rules"}
{"command":"history"}
{"command":"store","name":"myproof"}
{"command":"types"}
{"command":"type","name":"Idem"}
{"command":"homology","name":"Idem"}
{"command":"shutdown"}
```

`target_diagram` in `init` is optional. All commands except `init` and `resume`
require a session to be active.

### Responses

Every response is one of:

```json
{"status":"ok","data":{...}}
{"status":"error","message":"..."}
```

The `data` object includes:

| Field | Description |
|-------|-------------|
| `step_count` | Number of steps applied |
| `current` | Current diagram (`DiagramInfo`) |
| `source` | Source diagram (`DiagramInfo`) |
| `target` | Target diagram (omitted if not set) |
| `target_reached` | Whether current equals target |
| `rewrites` | Available rewrites — each has `rule_name`, `match_positions`, `match_display`, source/target `DiagramInfo` |
| `proof` | Running proof summary: dim, step_count, source/target labels (omitted if no steps taken) |
| `history` | Move list (only in response to `history` or `resume`) |
| `rules` | All rewrite rules (only in response to `list_rules`) |
| `types` | Type summaries (only in response to `types`) |
| `type_detail` | Full type info (only in response to `type`) |
| `homology` | Homology groups (only in response to `homology`) |

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

**`homology`** — computes the cellular homology of a named type. Returns an
array of `{dim, display}` objects, one per dimension that has generators.

---

## Session workspace

```
alifib session <file> --type <t>
```

Loads `<file>`, finds type `<t>`, and starts an interactive session for
building up definitions incrementally within that type. Unlike the REPL,
this does not require an initial source diagram — you work with the full
type environment.

### Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `let <name> = <expr>` | | Add a `let` binding (full alifib syntax); validated by re-interpretation |
| `goal <name> : <src> -> <tgt>` | `g ...` | Prove an (n+1)-cell interactively; registers it as a generator on success |
| `show` | | List all additions made this session |
| `export` | `e` | Print the session additions (for pasting into the source file) |
| `export <path>` | `e <path>` | Write the full modified source (original + additions) to `<path>` |
| `help` | `?` | Print this list |
| `quit` | `exit`, `q` | Exit the session |

### Goal sub-loop

Typing `goal f : lhs -> rhs` enters a rewrite sub-loop (identical to the
REPL) with `lhs` as source and `rhs` as target. The additional commands
`done` / `accept` (aliases `d`, `a`) close the goal and register the proof
as a generator; `abandon` discards it and returns to the session prompt.

### Export

`export` (no path) prints only the new lines added this session — suitable
for pasting into the original `.ali` file's type block. `export <path>`
writes the entire modified source (original file with additions injected).

---

## Session files

Both interfaces can save and load session files (JSON). A session file
records the source file path, type name, diagram names, and the ordered
list of moves (choice index + rule name). It is sufficient to fully replay
a session from scratch.

```json
{
  "source_file": "examples/Category.ali",
  "type_name": "Category",
  "source_diagram": "lhs",
  "target_diagram": "rhs",
  "moves": [
    {"choice": 0, "rule_name": "assoc"},
    {"choice": 0, "rule_name": "unit_l"}
  ]
}
```

REPL: `save <path>` / `load <path>`.  
CLI: `alifib rewrite init/step/undo/show/done --session <path>`.  
Daemon: `{"command":"save","path":"..."}` / `{"command":"resume","session_file":"..."}`.
