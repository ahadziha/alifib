# Interactive Rewriting

`alifib` provides an in-process terminal **REPL**, a **web GUI** sharing an
HTTP kernel with shell clients, and **serve** / **do** for shell or agent use
without a browser. A static WebAssembly build runs the same kernel in the
browser. All use the same line grammar and command evaluator.

## REPL

```
alifib repl <file> [<line>] [--emacs]
```

Loads `<file>` into the idle state: inspection commands work immediately. Use
`start <type> <source> [<target>]` to begin a rewrite session (target is optional).
The optional first line runs once after loading:

```sh
alifib repl examples/TRS.ali 'start Unit (split merge) id'
```

`--emacs` selects Emacs keybindings; the default is vi mode. The outer shell
quotes pass a single line argument. Inside that line, use parentheses, not
quotes, to group a multi-word diagram.

## Line grammar

A line is a chain of **parts** separated by `;`. Each part has a command word
followed by argument **words**, separated by any whitespace. Both splits happen
only at top level, outside:

- alifib string literals (`"…"` and `'…'`, with the language's backslash escapes);
- nested `(* … *)` comments, as the lexer reads them;
- balanced brackets: `(…)`, `[…]`, and `{…}`.

Bracketed words are kept verbatim, including their quotes and comments. No
comment begins inside a string; inside a comment, quotes and brackets have no
special meaning. For example, `start T (a (* author's note *) #0 b)` has two
arguments. At top level, a comment separates words like whitespace and is
dropped. Empty parts are skipped.

The whole line is scanned and every part parsed before anything runs. An
unterminated string or comment, an unclosed or stray bracket, or mismatched
brackets such as `(…]` reject the **whole line**, so even earlier parts do not
run. Otherwise parts run in order, stopping at the first usage or execution
error; earlier successful parts are not rolled back.

The four argument kinds are:

| Kind | Commands | Meaning |
|------|----------|---------|
| Name looked up | Type argument of `start`, `resume`, `type`, `homology` | The word is looked up exactly, not rewritten or checked against an identifier regex; names may begin with digits, as in `0Simplex`. |
| Name introduced | `store` | Must be one name as the lexer reads it (`language::is_name`), because it becomes `let <name> = …`. Keywords, `(p)`, and `"p"` are not names. |
| Diagram | Source/target of `start`, proof/target of `resume` | The word is an alifib expression, verbatim. Group a multi-word diagram with parentheses: `start T (a #0 b) c`. `"aaa"` and `'aaa'` remain string literals, not the name `aaa`. |
| Path | `load`, `save` | The word itself, or the decoded contents of a whole string literal: `load "my project/F.ali"`. Language escapes apply (`\n`, `\t`, `\r`; otherwise `\c` reads as `c`). |

`quit` / `exit` / `q` are REPL-local and `clear` is browser-local, only when the
trimmed **whole line** is that word. The kernel does not recognise them:
`show; quit` runs `show`, then errors; `clear; types` errors immediately.

## Commands

### Without an active rewrite

Except for `load`, `help`, and the local exit command, these require loaded
source, but do not require an active rewrite session.

| Command | Description |
|---------|-------------|
| `load [<path>]` | Load a disk file, or repeat the last disk load attempt |
| `save [<path>]` | Write the running source to a path, or to its root file (disk sessions only) |
| `types` | List types available in the loaded module |
| `type <name>` | Inspect a type: generators, diagrams, maps |
| `homology <name>` | Compute cellular homology of a type |
| `start <t> <s> [<g>]` | Start a rewrite session from an initial diagram (target optional) |
| `resume <t> <p> [<g>]` | Resume a session from a diagram `<p>`, replaying its steps (target optional) |
| `holes` | List holes and constraints of maps whose definitions the root source holds |
| `fill <n>` | Start a hole-filling session for hole `<n>` |
| `backward [on\|off]` | Show or toggle backward rewrite mode (when idle) |
| `status` / `show` | Session state, or module path when idle |
| `print` | Print the running source |
| `stop` | End the active session (or abandon a fill) |
| `help` / `?` | Show command list |
| `quit` / `exit` / `q` | Exit the terminal REPL (whole line only) |

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
❯ start Unit (split merge) id
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

`save [<path>]` writes the running source with stored definitions appended as
`@TypeName\nlet name = <expr>` blocks and completed fills incorporated. With no
path it writes the disk root file. For browser-owned source, use the editor
instead: `save`, with or without a path, is refused. `print` returns the running
source as transcript output; it is a query and never writes into the editor.

### Resuming proofs

`resume <type> <proof> [<target>]` reopens a stored proof as a live session: it
decomposes the diagram into its rewrite steps, applies them all, and lets you
undo, continue, or branch. `<proof>` is any diagram name or expression in the
type. Toggle `backward on` first to run from the proof's output boundary toward
its input; `<target>` is the goal you intend to reach, supplied separately.

### Filling holes

A partial map clause `gen => ?` leaves a generator's image open — a *hole*. A map
may carry holes and still be well-formed; you close them interactively. Holes can
also be *inherited*: a map written as a composition `F.G`, or as an extension of
another map, carries the still-open images of the maps it is built from (a
generator left a hole by `G`, or one whose image `F` cannot yet determine).

`holes` lists open holes of maps whose **definitions occur in the root's
source**, numbered, each shown as
`?name : in → out` (or just `?name` for a 0-cell), followed by any *constraints*
imposed by conditional pending assignments (the equations `F(x.side) = a.side`).

The definition, not an included alias, determines ownership: a map defined in
the root under an included type is editable there too; a map merely imported
from another file is not. Matching uses the whole type name and the module's
store key, not a shortened display label or just the last dotted component.
Listings are ordered by type, map, dimension, and source-generator name, so
faces precede holes depending on them. `fill` and `done` use the same ownership
lookup as `holes`.

`fill <n>` opens a filling session for hole `n`. Filling the image of an `m`-cell
`x` of a map `F : D → T` means building `F(x)`:

- for `m ≥ 1` it is an ordinary **rewrite session** from `F(x.in)` to `F(x.out)` —
  the full `apply`/`auto`/`undo`/`proof`/… command set works exactly as in a
  free rewrite;
- for a 0-cell it is the **choice** of one of `T`'s 0-cells (`apply <k>` picks the
  `k`-th candidate; `undo` reopens them). The choice is the one step, so `undo`
  and `redo` with a step number behave as in a rewrite of at most one step.

A hole whose image depends on other unfilled holes cannot be filled until those
are; the REPL reports which to fill first.

`done` finalises the fill: it splices `x => <proof>` into the map's definition —
replacing an explicit `x => ?` in place, or appending a clause (opening a
`[ … ]` extension on a bracket-less map such as `= F.G`) — and locates that
definition wherever it is written, whether in a separate `@T` block or inline in
the type body (`T <<= { … let H … }`). It then re-evaluates the file, so the hole
is gone and the source is the durable record. An inconsistent fill is rejected at
re-evaluation and the session is kept so you can retry.

---

## Loading and reloading

`load <path>` loads a file and its dependencies without starting a rewrite.
Every successful load starts idle with **backward off**; `start`, `resume`, and
`fill` use that session's backward setting. A bare `load` repeats the last
accepted disk load attempt, so it both picks up edits and retries a failed load.
It is refused after an in-memory Evaluate (press Evaluate instead).

A **refused** load is not an attempt: a nonexistent path, an out-of-scope path,
or a bare load after an in-memory load leaves the old session, revision, and
stamps unchanged. Once a load is accepted, the old session is freed first. A
read, parse, or interpretation failure then leaves **nothing loaded**, with new
stamps and diagnostics. A bare `load` can retry that disk file after it is fixed.
`start` and `resume` use the loaded source; they do not reread disk files.

## Shell and agent use

```
alifib serve [<dir>] [--bind <addr>]
alifib web [<dir>] [--bind <addr>]
alifib do [--at <addr>] <line>
```

Both servers default to directory `.` and address `127.0.0.1:8000`. The directory
must exist. It is passed explicitly (the process working directory is not
changed): relative `load`/`save` paths start there, it replaces the working
directory as the first include search path, and under `web` it roots the file
picker. Different projects can use different binds, selected by `do --at`.

`serve` has no browser interface or file picker; its session cannot later be
opened as a browser page. `web` adds that interface to the same HTTP kernel.
Neither is a multi-session service: the browser and every `do` client share
**one** session, without session IDs.

```sh
# Terminal 1: use `web` instead to share with a browser
alifib serve examples

# Terminal 2: quote the whole line for the shell
alifib do 'load TRS.ali'
alifib do 'start Unit (split merge) id'
alifib do 'show'                  # read the menu before every apply n
alifib do 'apply 0'
alifib do 'store myproof; save Proof.ali'
# After editing the disk file (also retries a failed load):
alifib do 'load'
```

`do` prints replies like the REPL, including source-bearing diagnostics, and
exits unsuccessfully on a command, connection, or protocol error. Connection and
protocol errors, including an HTTP error status and its body, go to stderr. It sends
neither `expect` nor `page` (below); use `show` for the current state. An agent's
`load` replaces the browser's session. A browser Evaluate replaces the agent's,
subject to its stale-state guard. Afterwards the source lives in the editor's
memory: `save` and bare `load` are refused, which is how an agent learns of it.
A named `load <path>` takes the session back, discarding that in-memory session.

## Web GUI

`alifib web <dir>` serves the browser and same-origin JSON API. For remote use:

```sh
# Remote machine:
alifib web examples --bind 127.0.0.1:8000
# Local machine:
ssh -L 8000:127.0.0.1:8000 user@remote-host
```

Open `http://127.0.0.1:8000` locally. Forward the **same port number** that the
server bound: the loopback `Host` check (below) accepts only that port, so a page
opened through `ssh -L 9000:127.0.0.1:8000` at `localhost:9000` is refused (403).
The browser has a REPL panel with the same
commands and a whole-line `clear` command (also the Clear button). GUI controls
issue equivalent lines. Evaluate sends the active source, all open tabs, and
all indexed files to the kernel; the kernel's loader, not JavaScript, resolves
includes. An indexed file that cannot be fetched is skipped with a note, and an
unavailable index sends no examples; neither fails the Evaluate. The static WASM build uses the same JSON calls without HTTP.

The browser is **not live**. It catches up when it calls (an action or a missing
view) or regains focus; there is no timer, command feed, or shared transcript.
It shows only its own replies in its transcript, and other clients' effects as
state/module changes. Every answer is committed, and each display is from one
cut of the kernel. A view not yet held draws a fallback (the current diagram
for a preview, nothing for an item) until the next answer supplies it.

Browser actions capture the instance and revision **shown when the user acts**.
If another client has moved the session, a guarded action is refused with
`not run: the session changed since this page last showed it`, and the page
shows the new state. Quick successive actions can therefore also be refused:
for example, turning backward on and immediately pressing Start before the
first answer commits. A single line `backward on; start T s t` avoids that gap.
Refused commands are not automatically retried; act again on the refreshed
state if appropriate. Transport failures likewise do not replay mutations.

Only a **successful Evaluate** links an editor tab to the current server
instance and load generation. `done` / `store` change `ModuleInfo.source`, not
merely a command's output. On a later answer, even an agent's edit can reach
that linked tab. The browser settles the write synchronously at commit time:
the tab must still be open, its text must equal what was last sent or received,
and the instance/generation must still match. It edits the tab **in place,
never switching tabs**, or reports why it could not write: the tab was closed
or edited since, the source was not loaded by Evaluate on this page, or the
server restarted or the session was reloaded after that Evaluate. A page that did not Evaluate
the session does not silently acquire a link. `print` only prints; it never
links or edits a tab.

## JSON call reference

The canonical wire is [`src/interactive/rpc.rs`](../src/interactive/rpc.rs).
HTTP accepts one JSON call at `POST /api`; WASM exposes
`WasmKernel.call(request)` with a JSON string in and a JSON string out. There
are no separate view or command-specific HTTP endpoints.

Calls have the shape `{"method":"eval","args":{"line":"types"}}`:

| Method | Required arguments | Optional arguments |
|--------|--------------------|--------------------|
| `eval` | `line`: command line string | `expect`, `page` |
| `load_source` | `source`: root source string | `modules`: `{relativePath: source}` (default `{}`), `name`: root filename (default `source.ali`), `expect`, `page` |
| `poll` | `page` | None |

A disk `load` is a **line command**, e.g.
`{"method":"eval","args":{"line":"load F.ali"}}`, not a fourth method.
`load_source` is Evaluate: it uses an in-memory loader, with the root source
overriding the same key in `modules`. An empty or absent `name` uses `source.ali`;
a name without `.ali` has that suffix appended. It never loads dependencies
from disk.

### State, replies, and the clock

**State and Answer are separate.** A session's `State` says what it is; a
command's internal `Answer` adds a message or a result, never both, and an
`edited` flag. Its wire
`Reply` is rendered transcript data, not an embedded state or source:
`{input, generation, status, message, rendered, edited, diagnostics?}`. `status`
is `ok` or `error`. An `ok` reply has exactly one of `message` and `rendered`;
an `error` reply has a `message` and no `rendered`. `input` names the part
that ran (empty for Evaluate); diagnostics carry the source lines they cite.

A decoded call answers an object with `status: "ok"`, `instance`, `revision`,
`generation`, and `module_revision`. `eval` and `load_source` also have a
`replies` array (including command errors/refusals); `poll` has no `replies`.
Malformed calls instead answer `{"status":"error","message":"…"}` without
running anything. Check reply statuses even when the outer status is `ok`.

`instance` is an **opaque string identity**: compare it, never parse or compute
with it. It changes on a server restart; WASM uses `"0"`. The revision and its
two stamps are numbers on one clock:

- `revision` advances for each accepted load attempt and each command that may
  mutate a loaded session, even when the mutation fails. Queries and `save`
  do not advance it.
- `generation` is the revision of the last load attempt (successful or failed).
- `module_revision` is the revision of the last load attempt or successful
  source/store edit (`done`, `store`).

Thus `generation ≤ module_revision ≤ revision`. Refused loads and stale-call
refusals do not advance them.

The optional `expect` is exactly `{instance, revision}`. If either differs,
`eval` refuses the whole line when **any parsed part** is guarded: a mutation
or `save`. Read-only lines (`help`, `types`, `print`, etc.) still run. Every
`load_source` is guarded: an Evaluate must not discard an unseen session.
Absent `expect` means no stale-state guard, as with `do`.

### Page and one-cut answers

`page` describes what a client shows and what its choices ask to draw:

```json
{
  "seen": {"instance":"opaque-server-id","revision":12,"module_revision":9},
  "draw": [
    {"view":{"of":"current"},"held":true},
    {"view":{"of":"preview","choice":0},"held":false}
  ]
}
```

`seen` is optional and has exactly those **three** fields, not `generation`.
`draw` defaults to `[]`; `held` defaults to false. Each `held` says the client
already holds that view's result in the cut named by `seen`.

The answer is one cut, computed after the call's own commands, with nothing
interleaving between its counters, state, module, and views. Without `page`,
only the counters and own replies come. With `page`:

- No `seen`, or another instance: send `state` and the loaded `module`.
- Same instance: send `state` if `revision` differs; send `module` if
  `module_revision` differs. With nothing loaded, `module` is omitted: the
  state says `{"mode":"unloaded"}`.
- When state is sent (first contact, restart, or revision change), answer
  **all** `draw` entries, including held entries. Otherwise answer only unheld
  entries, leaving held entries' places `null`.
  `views` preserves `draw` order with exactly one place per entry whenever
  present; it is absent if state has not moved and all entries are held.
  With changed state and empty `draw`, it is `[]`.
- Each computed view place is `{"status":"ok","data":…}` or
  `{"status":"error","message":"…"}`. An unavailable view (no current rewrite,
  missing target/item, etc.) is an error result, not a null place. Error
  results can also be held. No separate view call exists.

For a well-formed client record, a changed module stamp also means changed
state. `state` is tagged by `mode`:

| Mode | Other fields |
|------|--------------|
| `unloaded` | None |
| `idle` | `module`, `backward` |
| `rewrite` | `activity`, `backward`, `rewrite` |
| `fill` | `activity`, `backward`, `hole`, `by` (`{"rewrite":…}` or `{"zero_cell":…}`) |

`activity` is a number that changes whenever a rewrite, resume, or fill
begins within a load, so a client can tell `stop; start …` from a step.

`rewrite` contains `step_count`, `can_undo`, `can_redo`, `current`, `initial`, `target`
(nullable), `target_reached`, `parallel`, and `rewrites`. A diagram summary has
`label`, `dim`, `cell_count`, and `cells_by_dim`; menu indices belong only to
this state. The definitions in
[`response.rs`](../src/interactive/response.rs) specify the fill and menu data.

`module` is a `ModuleInfo`: `{base, root, types, holes, constraints, source}`.
`source` is the running root source including stored proofs and completed fills;
`base` (nullable) is for shortening display labels only. Module identifiers
(`root`, a type/hole's `module`) are **full store paths**. Type names are whole
names within that module, possibly dotted; do not substitute a display label,
file stem, or final name component for either identity.

### Views

Each `draw` entry's `view` is tagged by `of`
([`view.rs`](../src/interactive/view.rs)):

| `of` | Additional fields |
|------|-------------------|
| `current`, `target`, `proof` | None |
| `preview` | `choice`: rewrite menu index |
| `item` | `module`, `type_name`, `item`, optional `boundary` |
| `map_image` | `module`, `type_name`, `map`, `generator`, optional `boundary` |

Every view's `data` is an object with a `strdiag` field. `item` and
`map_image` add `dim`, `label`, `input`, and `output`; `proof` adds
`step_count` and `output_boundary_map`.

`boundary` is `{dim, side}` with `side` `input` or `output`. Supply the full
module store path and whole type name, not the shortened accordion labels.
Views are drawn in the same cut as the state/module beside them; a view-only
answer is of the unchanged cut the caller already shows.

## Server concurrency and trust boundary

Every connection has its own thread; there is no cap on connections. If a
thread cannot be started, or a connection cannot be accepted, the server drops
it and pauses briefly. One mutex serialises calls on the shared `Rpc`/kernel,
locked only while calling it. A long `auto` can delay other API calls, but not
assets or the read-only file picker. An idle connection holds up only itself. A
kernel panic poisons that lock: the call that panicked and every later call are
answered HTTP 500, `the kernel panicked; restart the server`.

The request reader applies to **every route**, not just `/api`:

- Deadlines, none of which bounds a computation: the request line and headers
  must arrive within **10 seconds** of the first read; each later body read and
  each write then has **10 seconds**. A connection that closes, resets, or stays
  silent before its first byte is closed without an answer; one that times out
  mid-request is answered 408.
- Request line plus headers: **64 KiB** (431 if exceeded); body:
  **64 MiB** (413). Bounds/framing are checked before body allocation/read.
- `Host` is required; a POST needs `Content-Length`. Transfer encoding,
  malformed framing, invalid UTF-8, invalid percent encodings, or duplicate
  `Host`, `Content-Length`, `Transfer-Encoding`, `Content-Type`, or `Origin`
  headers are rejected (400). Request paths are percent-decoded
  **once**, after removing the query, before routing/path validation.
- On loopback, **every route** checks `Host` against `localhost:<port>` or the
  bound socket address (`127.0.0.1:<port>` / `[::1]:<port>`), ignoring case;
  at port 80 their forms without the port are also allowed. The port must be the
  bound one, so an SSH tunnel must forward the same port number. This protects
  the picker too.
- `POST /api` additionally requires `application/json` (case-insensitive,
  parameters ignored) and, if present, `Origin` equal to `http://<Host>`
  (ignoring case); failures are 403. There is no preflight/CORS access or token authentication.

Filesystem policy is `FsAccess` in
[`kernel.rs`](../src/interactive/kernel.rs), covering every disk command path:
named `load`/`save`, the remembered bare-load path, and the root for bare `save`.
Both read and write paths are canonicalised, even without a directory bound:

- **Read:** canonicalise the whole existing file. A missing path is refused.
- **Write:** canonicalise an existing entry whole, including a symlink. Only a
  missing entry uses its canonical parent plus filename. The parent must exist;
  a dangling symlink is refused, not treated as a new file.

The REPL is `Unrestricted`: relative paths use its working directory, and no
`.ali` extension is required (e.g. `save notes.txt`). A loopback server is
`Within(CanonicalDir)`: resolve against its canonical project directory, then
require an `.ali` file beneath it. Symlink escapes are refused. A non-loopback
server is `Denied("disk access is disabled on a non-loopback server")`; WASM is
`Denied("the browser build has no disk")`. These are disk-load refusal reasons.
An in-memory session's `save` first reports that the source belongs to the
browser's editor, irrespective of scope.

In-memory Evaluate still works on any bind. Under `web`, the read-only picker
still serves the directory's `.ali` files on any bind, with name validation and
canonical-in-root checks; scanning ignores symlinks. **This is not an include
sandbox**: disk-loaded modules keep the loader's usual include search paths,
including `ALIFIB_PATH`. Nor does it protect against a local adversary swapping
files between check and use. The request checks address hostile web pages;
non-loopback binding is not an authenticated multi-user deployment.

---

## Persistence

A session has no file format of its own. Its durable form is the diagram it is
building — the `(n+1)`-cell assembled from the steps applied so far.

- **Save** — `store <name>` registers the running proof as a named diagram in
  the active type; `save [<path>]` for a disk session writes the `.ali` source
  with those definitions appended. The proof is then an ordinary diagram in the
  source.
- **Reopen** — `resume` decomposes such a diagram back into its rewrite steps,
  with every step applied, so you can undo, continue, or branch from it. A
  forward session resumes from the diagram's input boundary, a `backward` one
  from its output; the target is supplied separately (it is the goal, not
  inferred from the diagram).

So a session round-trips entirely through `.ali` — there is no separate
session-file format.
