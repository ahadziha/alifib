# The examples manifest

This document describes how the web GUI discovers and serves `.ali` example
files — the same machinery feeds the Examples dropdown, the `include
<Name>` resolution, and the static GitHub Pages deployment.

## What the manifest is

A "manifest" is a small index file that lists what's available in a
collection. In our case it's `examples/index.json`, a JSON object mapping
each example's **stem** (the file's basename without the `.ali` extension,
e.g. `Theory`) to its **relative path** from the examples root:

```json
{
  "Theory":     "Theory.ali",
  "YangBaxter": "topics/braided/YangBaxter.ali"
}
```

The frontend fetches this once on boot, populates the dropdown with the
keys, and — when the user picks one or references one via `include` —
fetches the file at `examples/<relpath>`.

## Why we need it

A browser running a static page can't enumerate a directory over HTTP. A
request to `fetch('examples/')` against a generic static server either
404s or returns an opaque `index.html`. The frontend therefore needs an
explicit list.

The same file works in both modes:

- **`alifib web [<dir>]`** serves `GET /examples/index.json` dynamically —
  the server rescans the directory on every request, so edits on disk
  appear without a restart.
- **GitHub Pages / static hosting** ships the manifest as a committed
  artefact in `dist/`, generated at build time by the deploy workflow.

The URL scheme (`/examples/index.json`, `/examples/<relpath>`) is
identical in both cases, which is why the frontend contains no branching
for "am I talking to a server or a static host."

## Rules

Enforced identically by the Rust scanner (`web/shared/src/lib.rs`) and the
deploy script (`scripts/build_examples_manifest.py`) — both use the same
recursive walk, the same segment rule, and the same duplicate check:

1. **Recursive.** Subdirectories under the root are traversed. They exist
   for organisation only.
2. **Segment rule.** Every path segment (directory name and file stem)
   must match `[A-Za-z_][A-Za-z0-9_]*` — exactly the language's
   identifier rule. Anything else is skipped with a warning.
3. **Stems are globally unique.** A file's stem is its canonical name
   in the language — `include <stem>`. Two files anywhere in the tree
   sharing a stem (case-insensitively) is an error, not a shadow.
4. **Canonicalise-in-root.** The server refuses to read any path whose
   canonicalised form escapes the root directory, so `..`, absolute
   paths, and symlink escapes are all rejected.

When a duplicate is detected:

- The server's `/examples/index.json` returns a JSON error object
  (`{"error": "duplicate example stem `Foo`: a/Foo.ali, b/Foo.ali — rename one of them"}`),
  the dropdown stays empty, and the REPL log shows the message.
- The deploy script exits non-zero, failing the Pages build.

No silent shadowing, no "why does `include Foo` pick the other one."

## URL scheme

| Method | Path                                   | Returns                          |
| ------ | -------------------------------------- | -------------------------------- |
| GET    | `/examples/index.json`                 | Manifest (`{stem: relpath}`)     |
| GET    | `/examples/<relpath>`                  | File contents, `text/plain`      |
| POST   | `/api/load_source`                     | Accepts `{source, modules}`     |

`<relpath>` is the same slash-separated path that appears as a manifest
value (e.g. `topics/braided/YangBaxter.ali`). Each segment is revalidated
at request time — no path traversal via request-path trickery.

## Frontend data flow

1. **Boot** — `populateExamples()` in `web/frontend/app.js` fetches
   `examples/index.json` and fills the dropdown. If the manifest is an
   error object, the error is surfaced in the REPL log and the dropdown
   stays empty.
2. **User picks an example** — `fetchExample(name)` looks the stem up
   in the cached manifest, fetches the file at its relative path, and
   drops it into the editor. Contents are cached per-session so the
   file is fetched once.
3. **User hits Evaluate** — `collectIncludeModules(source)` scans the
   editor text for `include <Name>` directives, transitively fetches
   the referenced files through `fetchExample`, and hands the map to
   the backend as an `extra_modules` field on `/api/load_source`.
4. **Backend** — `WebRepl::load_source_with_modules` seeds the virtual
   loader with that map. The Rust interpreter resolves `include Name`
   by looking up `Name.ali` in the virtual loader, identical to how the
   CLI resolves files against the real filesystem.

## Subdirectories in practice

Given an examples tree:

```
examples/
├── Theory.ali
├── topics/
│   └── braided/
│       └── YangBaxter.ali
└── alg/
    └── Monoid.ali
```

The manifest is:

```json
{
  "Monoid":     "alg/Monoid.ali",
  "Theory":     "Theory.ali",
  "YangBaxter": "topics/braided/YangBaxter.ali"
}
```

In the editor, `include Monoid`, `include Theory`, `include YangBaxter`
all work the same as if they were flat under the root. The directory
tree is invisible to the language.

If you later add `topics/Monoid.ali`, the scanner notices the duplicate
stem and refuses the whole manifest until one of the two files is
renamed.

## Two producers, one format

### `ExampleSet::scan` (Rust — `web/shared/src/lib.rs`)

Used by `alifib web` on every request. Returns
`Result<Vec<ExampleEntry>, ScanError>`. Missing root is not an error
(empty manifest); any other I/O failure is. Duplicate detection uses
case-folded stems.

### `scripts/build_examples_manifest.py`

Used by `just web-wasm` and the GitHub Pages workflow. Mirrors the
source directory into a destination, writes `<dest>/index.json`. Fails
with exit 1 on duplicate stems. The two implementations apply the
same rules so there's no dev-vs-CI drift.

## Extending later

The current design maps `stem → relpath`. If we ever want hierarchical
module names (e.g. `include topics.braided.YangBaxter`), the
generalisation is a compatible widening of the manifest format
(`dotted → relpath`) and a grammar change in the parser — it does not
require reorganising the filesystem or the URL scheme. This is why we
did not reach for dotted names pre-emptively; the current layout
accepts that extension cleanly if the need actually arises.
