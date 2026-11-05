# The examples manifest

This document describes how the web GUI discovers and serves `.ali` example
files. The index feeds the Examples dropdown and the files sent to the kernel
on Evaluate, in both the HTTP and static WASM deployments. Only the kernel
loader resolves `include <Name>`.

## What the manifest is

A "manifest" is a small index file that lists what's available in a collection.
In our case it's `examples/index.json`, a JSON object whose `modules` object maps
each example's **name** to its **relative path** from the examples root. A name
is the file's path under the root with the `.ali` suffix removed (forward
slashes):

```json
{
  "modules": {
    "Theory":  "Theory.ali",
    "TRS":     "TRS.ali",
    "TRS/Aux": "TRS/Aux.ali"
  }
}
```

A failed scan is never a manifest: the server answers it with HTTP 500 and
`{"error": "<message>"}`, so no example name can be mistaken for an error.

The frontend fetches this once on boot, populates the dropdown with the keys,
and fetches a selected file at `examples/<relpath>`. It refreshes the index on
every Evaluate and fetches every indexed file not supplied by an open tab.

## Why we need it

A browser running a static page can't enumerate a directory over HTTP. A request
to `fetch('examples/')` against a generic static server either 404s or returns an
opaque `index.html`. The frontend therefore needs an explicit list.

The same file works in both modes:

- **`alifib web [<dir>]`** serves `GET /examples/index.json` dynamically — the
  server rescans on every index request, so the next refresh or Evaluate sees
  disk edits without a restart. The default directory is `.`, and it must exist.
- **GitHub Pages / static hosting** ships the manifest under `examples/`,
  generated at build time by the deploy workflow.

The URL scheme (`/examples/index.json`, `/examples/<relpath>`) is identical in
both cases; the same picker/fetch code works against either host.

## Rules

The Rust scanner in [`cli/src/examples.rs`](../cli/src/examples.rs) is used by
both the running server and the static builder in
[`cli/examples/static_examples.rs`](../cli/examples/static_examples.rs):

1. **Recursive.** Subdirectories under the root are traversed; symlinks and
   other non-file/non-directory entries are ignored.
2. **Segment rule.** Every directory name and `.ali` file stem must be a name
   as the lexer reads it (`language::is_name`), not a keyword. This is not an
   ASCII identifier regex: names such as `0Simplex` are accepted. Non-name
   directories are skipped silently; `.ali` files with invalid stems are skipped,
   and the scan returns them for its caller to report on stderr.
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
3. `<Name>.ali` at the in-memory loader's top level (the project root in the
   manifest).

The first match wins. The kernel's loader implements this, not JavaScript (see
[`src/aux/loader.rs`](../src/aux/loader.rs)). A disk-loaded module searches its
own directory, its same-named subdirectory, then the loader search paths
(the server directory followed by `ALIFIB_PATH` under a server). Evaluate uses
an in-memory loader with all the supplied files, without disk search.
Because resolution is scoped this way, the **same stem can name different files
in different modules**: `Monoidal/Aux.ali`, `Bicategory/Aux.ali`, and
`TRS/Aux.ali` are three distinct submodules, and each parent module's
`include Aux` finds its own.

## URL scheme

| Method | Path | Returns |
|--------|------|---------|
| GET | `/examples/index.json` | Manifest (`{"modules": {name: relpath}}`), or HTTP 500 with `{"error": …}` |
| GET | `/examples/<relpath>` | File contents, `text/plain` |
| POST | `/api` | One `Rpc` call, including `load_source` for Evaluate |

`<relpath>` is the same slash-separated path that appears as a manifest value
(e.g. `TRS/Aux.ali`). Each segment is revalidated at request time, and the file
is canonicalised within the root. Request paths are percent-decoded once before
validation. See [the interactive reference](../docs/INTERACTIVE.md#server-concurrency-and-trust-boundary)
for the server's checks and limits on every route. `serve` has no picker routes.

## Frontend data flow

1. **Boot** — the frontend in `web/frontend/src/app.js` fetches
   `examples/index.json` and fills the dropdown with path-qualified names. An
   index error is surfaced in the REPL log and leaves the dropdown empty.
2. **User picks an example** — its contents are fetched at the manifest's
   relative path and opened in an editor tab.
3. **User hits Evaluate** — capture the active source and every open tab under
   its unique `.ali` filename. Refresh the index and fetch every indexed file
   not already supplied by a tab, concurrently and afresh for this Evaluate; a
   file that cannot be fetched is skipped with a note. Open tabs take
   precedence over indexed files with the same path; there is no transitive
   JavaScript include scan or per-session include-content cache.
4. **Call** — send `{"method":"load_source","args":{"source":…, "name":…,
   "modules":…,"expect":…, "page":…}}` through the one `POST /api` under HTTP,
   or through `WasmKernel.call` in the static build. `modules` is the
   `{relativePath: contents}` map of all tabs and indexed files.
5. **Kernel** — `Kernel::open_text` installs the root source at `name` in that
   map and creates `Loader::in_memory`. Only this loader resolves includes;
   failures are answered as diagnostics. Successful Evaluate links that editor
   tab to the new load generation; an error or stale-state refusal does not.

The picker index is not a live session feed. Disk edits are picked up on a later
fetch/Evaluate; a browser sharing an HTTP session catches up on its next call
or when it regains focus, not continuously.

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
  "modules": {
    "Bicategory/Aux": "Bicategory/Aux.ali",
    "Monoidal":       "Monoidal.ali",
    "Monoidal/Aux":   "Monoidal/Aux.ali"
  }
}
```

Inside `Monoidal.ali`, `include Aux` resolves to `Monoidal/Aux` — its own
same-named subdirectory — while the unrelated `Bicategory/Aux` is never in scope.
The directory tree is meaningful: it both *names* the modules and *scopes* their
includes.

## Two producers, one format

- **`ExampleSet::scan` ([`cli/src/examples.rs`](../cli/src/examples.rs))** — used
  by `alifib web` for each index request. Returns `io::Result<Scan>`: the sorted
  entries and the skipped files. Startup canonicalises the project directory and
  refuses a nonexistent root; later scan I/O failures are answered HTTP 500.
  Qualified paths need no deduplication.
- **[`cli/examples/static_examples.rs`](../cli/examples/static_examples.rs)** —
  used by `just web-wasm` and the GitHub Pages workflow. Scans once with the
  server's own scanner, mirrors that scan's `.ali` tree into the destination, and
  writes `<dest>/index.json` from the same scan; a scan error fails it before
  anything is written. The destination must be disjoint from the source and
  either missing, empty, or an earlier mirror (holding only `.ali` files, their
  directories, and a top-level `index.json`); anything else is refused rather
  than replaced. There is no second Python
  scanner or naming rule.

```sh
cargo run -p alifib-cli --example static_examples -- examples web/frontend/examples
```
