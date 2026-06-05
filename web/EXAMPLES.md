# The examples manifest

This document describes how the web GUI discovers and serves `.ali` example
files — the same machinery feeds the Examples dropdown, the `include <Name>`
resolution, and the static GitHub Pages deployment.

## What the manifest is

A "manifest" is a small index file that lists what's available in a collection.
In our case it's `examples/index.json`, a JSON object mapping each example's
**name** to its **relative path** from the examples root. A name is the file's
path under the root with the `.ali` suffix removed (forward slashes):

```json
{
  "Theory":  "Theory.ali",
  "TRS":     "TRS.ali",
  "TRS/Aux": "TRS/Aux.ali"
}
```

The frontend fetches this once on boot, populates the dropdown with the keys,
and — when the user picks one, or a file `include`s another — fetches the file
at `examples/<relpath>`.

## Why we need it

A browser running a static page can't enumerate a directory over HTTP. A request
to `fetch('examples/')` against a generic static server either 404s or returns an
opaque `index.html`. The frontend therefore needs an explicit list.

The same file works in both modes:

- **`alifib web [<dir>]`** serves `GET /examples/index.json` dynamically — the
  server rescans the directory on every request, so edits on disk appear without
  a restart.
- **GitHub Pages / static hosting** ships the manifest as a committed artefact in
  `dist/`, generated at build time by the deploy workflow.

The URL scheme (`/examples/index.json`, `/examples/<relpath>`) is identical in
both cases, which is why the frontend contains no branching for "am I talking to
a server or a static host."

## Rules

Enforced identically by the Rust scanner (`web/shared/src/lib.rs`) and the deploy
script (`scripts/build_examples_manifest.py`) — both use the same recursive walk
and the same segment rule:

1. **Recursive.** Subdirectories under the root are traversed.
2. **Segment rule.** Every path segment — each directory name and the file
   stem — must match `[A-Za-z_][A-Za-z0-9_]*`, exactly the language's identifier
   rule. A file or directory with an invalid segment is skipped with a warning
   (it could never be `include`d anyway).
3. **Name = relative path minus `.ali`.** `Theory.ali` → `Theory`,
   `TRS/Aux.ali` → `TRS/Aux`. This is both the manifest key and the dropdown
   label. Names are filesystem paths, hence unique by construction: there is **no
   duplicate check and no global uniqueness requirement on stems** — `TRS/Aux.ali`
   and `Bicategory/Aux.ali` coexist happily.
4. **Canonicalise-in-root.** The server refuses to read any path whose
   canonicalised form escapes the root directory, so `..`, absolute paths, and
   symlink escapes are all rejected.

## How `include <Name>` resolves

`include` takes a **bare identifier**, not a path, and it is resolved **relative
to the including file's location** — trying, in order:

1. a sibling `<Name>.ali` in the including file's own directory;
2. `<Name>.ali` in the including file's **same-named subdirectory** (so `Foo.ali`
   may keep private submodules in a `Foo/` directory);
3. `<Name>.ali` at the root.

The first match wins. This mirrors how the command-line interpreter resolves
includes — own directory → same-named subdirectory → `ALIFIB_PATH` (see
`src/aux/loader.rs`) — and is implemented in the frontend by `resolveIncludeKey`.
Because resolution is scoped this way, the **same stem can name different files
in different modules**: `Monoidal/Aux.ali`, `Bicategory/Aux.ali`, and
`TRS/Aux.ali` are three distinct submodules, and each parent module's
`include Aux` finds its own.

## URL scheme

| Method | Path                    | Returns                                          |
| ------ | ----------------------- | ------------------------------------------------ |
| GET    | `/examples/index.json`  | Manifest (`{name: relpath}`) or `{ "error": … }` |
| GET    | `/examples/<relpath>`   | File contents, `text/plain`                      |
| POST   | `/api/load_source`      | `{ source, modules, source_name }` → load result |

`<relpath>` is the same slash-separated path that appears as a manifest value
(e.g. `TRS/Aux.ali`). Each segment is revalidated at request time — no path
traversal via request-path trickery. The `modules` field is the
`{relpath: contents}` map the frontend assembles from the editor's `include`s.

## Frontend data flow

1. **Boot** — `populateExamples()` in `web/frontend/src/app.js` fetches
   `examples/index.json` and fills the dropdown with the (path-qualified) names.
   If the manifest is an error object, the message is surfaced in the REPL log
   and the dropdown stays empty.
2. **User picks an example** — its contents are fetched at the manifest's
   relative path and dropped into the editor (cached per session, so each file
   is fetched once).
3. **User hits Evaluate** — `collectIncludeModules(source, parentKey)` scans the
   editor text for `include <Name>` directives; for each, `resolveIncludeKey`
   finds the right manifest entry (sibling → same-named subdirectory → root),
   and dependencies are fetched transitively. The resulting `{relpath: contents}`
   map is handed to the backend.
4. **Backend** — `load_source_with_modules` seeds a virtual loader with that map;
   the interpreter then resolves `include Name` exactly as it does against the
   real filesystem.

## Subdirectories in practice

Given an examples tree:

```
examples/
├── Monoidal.ali
├── Monoidal/
│   └── Aux.ali
└── Bicategory/
    └── Aux.ali
```

the manifest is:

```json
{
  "Bicategory/Aux": "Bicategory/Aux.ali",
  "Monoidal":       "Monoidal.ali",
  "Monoidal/Aux":   "Monoidal/Aux.ali"
}
```

Inside `Monoidal.ali`, `include Aux` resolves to `Monoidal/Aux` — its own
same-named subdirectory — while the unrelated `Bicategory/Aux` is never in scope.
The directory tree is meaningful: it both *names* the modules and *scopes* their
includes.

## Two producers, one format

- **`ExampleSet::scan` (Rust — `web/shared/src/lib.rs`)** — used by `alifib web`
  on every request. Returns `Result<Vec<ExampleEntry>, ScanError>`; a missing
  root is not an error (empty manifest), any other I/O failure is. Names are
  qualified paths, so there is nothing to deduplicate.
- **`scripts/build_examples_manifest.py`** — used by `just web-wasm` and the
  GitHub Pages workflow. Mirrors the source tree into a destination and writes
  `<dest>/index.json` by the same rules, so there is no dev-vs-CI drift.
