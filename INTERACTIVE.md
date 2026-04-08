# Interactive Rewriting

`alifib` provides two interfaces for constructing (n+1)-dimensional proof
diagrams step by step: a **REPL** for interactive use and a **daemon** for
editor and tooling integration.

---

## REPL

```
alifib repl <file> --type <t> --source <s> [--target <t>]
```

Loads `<file>`, finds type `<t>`, and starts a rewrite session from diagram
`<s>`. The optional `--target` names the goal diagram; the REPL will report
when it is reached.

### Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `<n>` | | Apply rewrite at index `n` |
| `step <n>` | `s <n>` | Apply rewrite at index `n` |
| `undo` | `u` | Undo the last step |
| `undo <n>` | `u <n>` | Undo back to step `n` (0 = fully reset to source) |
| `show` | | Redisplay current diagram and available rewrites |
| `rules` | `r` | List all rewrite rules (n+1 generators) with their boundaries |
| `info <name>` | `i <name>` | Show source → target of a named generator |
| `history` | `h` | Show the sequence of moves applied so far |
| `proof` | `p` | Show the running (n+1)-dim proof diagram and its source/target |
| `save <path>` | | Save the session to a JSON file |
| `load <path>` | `l <path>` | Load and replay a session file, replacing current state |
| `help` | `?` | Print this list |
| `quit` | `exit`, `q` | Exit the REPL |

### Display

The prompt shows the current step count. After each `step` or `undo`, the
current diagram and available rewrites are printed automatically. Rewrites
show bracket notation to indicate where in the current diagram each rule
matches:

```
rewrite[0]> 0
Applied idem (choice 0).

[1] id id

rewrites:
  [0] idem : [id id]  ->  id
```

The brackets `[id id]` indicate which top-dimensional cells are covered by
the match.

---

## Daemon

```
alifib serve [<file> --type <t> --source <s> [--target <t>]]
```

Runs a JSON-lines server on stdin/stdout. One JSON object per line in each
direction. Suitable for editor integration: spawn as a subprocess and
communicate via its stdio.

If `<file>`, `--type`, and `--source` are provided, the session is
pre-loaded and an initial state response is emitted before the request loop
starts. Otherwise the daemon starts blank and waits for an `Init` request.

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
{"command":"shutdown"}
```

`target_diagram` in `init` is optional. All commands except `init` and
`resume` require a session to be active.

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
| `current` | Current diagram (label, dim, cell_count, cells_by_dim) |
| `source` | Source diagram |
| `target` | Target diagram (omitted if not set) |
| `target_reached` | Whether current equals target |
| `rewrites` | Available rewrites, each with `rule_name`, `match_positions`, `match_display`, source/target `DiagramInfo` |
| `proof` | Running proof diagram summary: dim, step_count, source/target labels (omitted if no steps taken) |
| `history` | Move list (only included in response to `history` or `resume`) |

`DiagramInfo` has the fields `label` (space-separated top-level names),
`dim`, `cell_count`, and `cells_by_dim` (labels at every dimension from 0
to top).

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
