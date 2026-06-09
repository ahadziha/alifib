---
kind: impl
status: stable
last-touched: 2026-06-09
code: [src/aux/mod.rs, src/aux/id.rs, src/aux/error.rs, src/aux/loader.rs, src/aux/path.rs, src/aux/bitset.rs, src/aux/intset.rs, src/aux/graph.rs]
---

# aux — the substrate everything else stands on

> No mathematics of its own, but every other module leans on it. `aux` is the
> bag of primitives: how cells are named (`id`), how diagnostics travel
> (`error`), how source files become ASTs (`loader`, `path`), and the three
> integer-set representations (`bitset`, `intset`, `graph`) that back the hot
> loops of [[core-ogposet]], [[core-matching]] and [[analysis]].

`src/aux/mod.rs` is the module root. It is split cleanly between the *public*
surface (`id`, `error`, `loader`, `path`) used across the [[interpreter]] and the
*crate-private* data structures (`bitset`, `intset`, `graph`, all
`pub(crate)`) used only inside `core`/`analysis`. The root also re-exports the
identifier zoo (`pub use id::{GlobalId, HoleId, LocalId, ModuleId, Tag}` — where
`HoleId` names a [[hole]]'s metavariable and `Tag::Hole(HoleId)` is its
paste-tree leaf — `pub use error::Error`) and carries one stray helper,
`dim_subscript` (dimension → Unicode subscript digits), which currently has
**no callers**: its boundary-slot renderers died with the old hole-inference
pass, and being `pub` it draws no dead-code warning.

## What it owns

| Module | Responsibility |
|---|---|
| `id.rs` | the identifier kinds (`GlobalId`, `HoleId`, `LocalId`, `ModuleId`) + `Tag`, their union |
| `error.rs` | `Error` (message + notes) and `report_load_file_error` |
| `loader.rs` | `Loader` — read source, resolve `include <Name>` directives, return `LoadedFile` |
| `path.rs` | canonicalisation and search-path dedup |
| `bitset.rs` | `BitSet` — dense membership over `0..N` for traversal scratch |
| `intset.rs` | `IntSet = Vec<usize>` kept sorted/deduped + merge ops |
| `graph.rs` | `DiGraph` + topological-sort (single and enumerated) |

## Key public types

- **`GlobalId(usize)`** (`id.rs`) — process-unique, allocated atomically from a
  single `static AtomicUsize` via `GlobalId::fresh` (`Ordering::SeqCst`). Opaque;
  never construct directly. `Display` prints `#n`. This is the spine of the
  global store's cell identity.
- **`HoleId(usize)`** (`id.rs`) — a metavariable: the unknown image of a domain
  generator under a partial map with holes. Its own atomic counter
  (`Ordering::Relaxed` — uniqueness is all that matters); `Display` prints `?n`.
  Lives only inside `Tag::Hole`.
- **`LocalId = String`**, **`ModuleId = String`** — type aliases. A `ModuleId` is
  *always* a canonical absolute path (`std::fs::canonicalize`), so two spellings
  of one file never become two modules.
- **`Tag`** = `Local(LocalId)` | `Global(GlobalId)` | `Hole(HoleId)` — the
  identifier union that threads through elaboration. `Local` tags are scoped to
  the enclosing type or module complex; `Global` tags name finalised cells;
  `Hole` tags appear *only* inside the boundary paste trees of a
  `core::map_hole::MapHole` — never as a key in a real map, a complex generator,
  or a built diagram's labels. Central to the [[interpreter]] lookup chain. Its
  `Ord` is total and *segregated by variant*: `Local < Global < Hole`, before
  comparing payloads.
- **`Error { message, notes }`** (`error.rs`) — a diagnostic accumulator;
  `with_note` chains. `report_load_file_error` is the only printer, fanning a
  `LoadFileError` out to stderr (and into `language::report_errors` for parse
  failures).
- **`Loader`** (`loader.rs`) — search paths + a pluggable `read_file` closure
  (`Arc<dyn Fn>`). Three constructors: `Loader::default` seeds cwd +
  `ALIFIB_PATH` + extras; `default_with_root_source` overlays one in-memory root
  source on a real filesystem (how `interactive::engine::reevaluate` re-checks an
  edited buffer without writing it to disk — see [[interactive-engine]]);
  `with_virtual_files` swaps in a pure in-memory map for tests (it starts with
  *no* search paths — virtual includes resolve only via the parent-dir
  prepending of `with_parent_dir`, with no canonicalisation). Besides `load`,
  the parse-only `load_only_root` skips dependency resolution — the
  `--ast`/`--print` path in [[cli]].
- **`LoadedFile`**, **`ModuleResolutions`**, **`ResolvedModule`** — the loader's
  output: root program, source, the `(parent, module_name) → canonical_path`
  resolution map, and dependency modules in topological (leaves-first) order.
- **`path`** (`path.rs`) — three free functions, no types. `canonicalize` is the
  best-effort variant (falls back to the input string); `canonicalize_existing`
  is the strict variant (`Result`, no fallback); `normalize_search_paths`
  canonicalises a path list (best-effort) and dedups while preserving order. The
  loader keys modules on the strict canonical path — the same value
  `engine::load_file_context` binds as `canonical_path` and uses as the store
  key ([[interactive-engine]]).
- **`BitSet`** *(internal)* — `Vec<u64>` words + cached `count`. Word-level
  `union` / `difference_inplace`; `reset` / `copy_from` reuse the allocation.
- **`IntSet = Vec<usize>`** — not a struct, a contract: sorted and deduplicated.
  Free functions `insert`, `union`, `difference`, `is_disjoint`, `collect_sorted`
  all assume and preserve that invariant. `intersection` exists with the same
  signature but is `#[allow(dead_code)]` and has **no callers anywhere** (not even
  tests) — kept deliberately, see [[source-drift]]; do not treat it as live.
- **`DiGraph`** *(internal)* — nodes `0..n`, both `successors` and `predecessors`
  stored as `IntSet`s so edges traverse either direction in O(degree).

## Data flow — loading a file

`loader.rs` is the only module with real control flow. One call, `Loader::load`:

```
load(path)
  │  read_file_at  ── canonicalize_existing (real) | as-is (virtual)
  ▼  (canonical_path, source)
  │  with_parent_dir ── prepend file's dir + same-named subdir to search paths
  ▼
language::parse(source)            ─→ Parse error → LoadFileError::Parse
  ▼  Program
resolve_all_modules(loader, canonical_path, program)
  │   DFS over language::collect_includes
  │   find_file(module_name) ── try each search dir for "<name>.ali"
  │   visited set: re-encounter on stack ⇒ LoadFileError::Cycle
  ▼
LoadedFile { canonical_path, source, program, resolutions, dep_modules }
```

The DFS is in `resolve_recursive` (internal): for each include it finds the
file, recurses into its dependencies *first*, then `insert_module` +
`register_resolution`. Registering only after the subtree is stored is the
invariant that keeps a resolution from ever pointing at an unstored program.
`ModuleStore::into_parts` drains `dep_order` to hand back modules leaves-first.

## Include resolution — the search-path precedence

`loader.rs` owns the *filesystem* side of the [[module-system]]: turning a bare
`include <Name>` into a concrete `.ali` file. (The *semantic* side — splicing the
resolved module's generators into scope — lives in the [[interpreter]] and
[[module-system]].) The directories searched for `<Name>.ali` are assembled by
`Loader::with_parent_dir`, in this strict precedence order:

1. **The including file's own directory.** `Foo.ali`'s includes are first sought
   in the directory holding `Foo.ali`.
2. **A same-named subdirectory.** `Foo.ali` may keep private submodules in a
   `Foo/` directory beside it and include them by bare name: `include Aux` from
   `…/Foo.ali` resolves `…/Foo/Aux.ali`. (The stem is `file_stem`; the subdir is
   `parent.join(stem)`.)
3. **The inherited search paths** — cwd seeded by `Loader::default`, then the
   `ALIFIB_PATH` directories (`:`-separated on Unix, `;` on Windows, via
   `Loader::path_separator`/`split_paths`), then any `extra_search_paths`.

`find_file` walks this list and the **first** directory holding `<Name>.ali`
wins (closest-wins). Because each module recurses with its *own*
`with_parent_dir` loader, two files in different directories that both
`include Aux` may resolve to different `Aux.ali` files — pinned by
`virtual_loader_subdirectory_resolution` (`tests/interpreter.rs`), where sibling
roots `A.ali` and `B.ali` each resolve `Aux` to their own `A/Aux.ali` and
`B/Aux.ali`. The same-named-subdirectory rule on the real filesystem is pinned by
`submodule_in_same_named_directory` (`SubMod.ali` resolves `Aux` from `SubMod/`).
Duplicate directories are collapsed by `path::normalize_search_paths` (or a plain
`HashSet` filter for the virtual loader), preserving order, so a directory that
appears under two spellings is searched once.

`ModuleResolutions` is the durable output: a nested map keyed by
`(parent canonical path, module name)` → the dependency's canonical path, looked
up later by `resolve(parent, name)` without allocation. Cycles are caught by the
`visited` set in `resolve_all_modules`/`resolve_recursive` — re-encountering a
path still on the recursion stack yields `LoadFileError::Cycle`.

## Non-obvious invariants & gotchas

- **`GlobalId` is monotone and never reused.** A single global atomic counter,
  never reset. IDs are unique for the process lifetime, not per-store — two
  stores built in one run draw from the same well.
- **`Tag`'s ordering segregates kinds.** `Local(_) < Global(_) < Hole(_)`
  always, before comparing payloads. Anything that sorts `Tag`s (canonical
  orderings, dedup) inherits this; do not assume lexical order across arms.
- **`ModuleId` must be canonical or modules duplicate.** `path::canonicalize`
  *falls back to the input string* on failure (for paths that may not exist);
  `canonicalize_existing` *errors* instead. The loader uses the strict variant
  for module cache keys precisely so a fallback can never silently fork a module
  into two. Picking the wrong one is the classic latent bug here.
- **`BitSet::contains` is bounds-safe; `insert`/`remove` are not.** `contains`
  guards `w < self.bits.len()`; the mutators index directly. Size the universe
  correctly via `new` / `reset` before inserting. Its raison d'être is the
  pre-allocated scratch pool reused across iterations in `ogposet::traverse`
  (`scratch_in` / `scratch_out` in `src/core/ogposet.rs`); `reconstruct.rs` also
  uses it for downset/image sets (`embedding_to_bitsets`) — the bitset.rs rustdoc
  claiming traverse-exclusivity is stale.
- **`IntSet` is a discipline, not a type.** The alias buys nothing the compiler
  enforces; every mutation must go through `intset::insert` or stay sorted by
  construction. The payoff is O(n+m) merges and binary-search membership on the
  small (1–8 element) face/coface sets that fill `Diagram`/`Ogposet` — see
  `cofaces_to_top` and the `faces_*` builders in `src/core/diagram.rs`.
- **`DiGraph::add_edge` is idempotent** (sorted insert dedups), and the dual
  adjacency means `add_edge(u,v)` writes to *both* `successors[u]` and
  `predecessors[v]`.
- **Two topological-sort entry points, one caller.** `topological_sort` is plain
  Kahn (single order, `Err(())` on cycle). `try_topological_sorts` *enumerates*
  orders by backtracking, calling `f` on each until one returns `Some`, capped by
  a `limit` (errs with `"cycle"` or `"exhausted"`). Both call sites are in
  `src/core/reconstruct.rs`: the enumerator with `limit = 10_000` to find a
  topological order whose layering realises correctly, falling back to the
  single sort otherwise.

## Mathematics

This module is **pure infrastructure** — it realises no mathematical object. Its
bridge is a **support relationship**: the data structures here are the
representation choices that make the genuine mathematics tractable, not the
mathematics itself.

- `bitset` and `intset` are the membership/adjacency representations underneath
  [[oriented-graded-poset|oriented graded posets]]: an OGP's faces and cofaces
  are stored as sorted `IntSet`s, and `BitSet` provides the reusable scratch sets
  for the closure/traversal that computes its sub-posets and boundaries
  (`src/core/ogposet.rs`). See [[core-ogposet]].
- `graph::DiGraph` and its topological sorts back the **flow-graph machinery**:
  `src/core/flow.rs` builds a `DiGraph` whose edges record
  $\partial^+_k(x) \cap \partial^-_k(y) \neq \varnothing$, and
  `src/core/reconstruct.rs` layers a diagram by topologically sorting the maximal
  flow graph. The flow graph is the lens that turns subdiagram matching into
  labelled subgraph matching — see [[core-matching]] and [[flow-graph]]. The same
  `DiGraph` also underlies the string-diagram layering in [[analysis]].

None of these structures *is* an OGP or a flow graph; each is the integer-set
plumbing those constructions are expressed in. For the identifier and error
plumbing's role in the elaboration pipeline, see [[interpreter]] and
[[core-complex]] (where `Tag` keys the lookup chain — the names that label
generating [[atom|atoms]]). The one genuinely semantic identifier is
`Tag::Hole(HoleId)`: it *is* the metavariable of a [[hole]], representable as an
ordinary paste-tree leaf precisely so the hole machinery needs no special cases.

The `loader`/`path` half realises the file-system side of the
[[module-system]]: `include <Name>` is the language's import form, and the
search-path precedence above is *how a name becomes a file*. Splicing that
resolved file's contents into the importing scope is the module system's
*semantic* side — see [[interpreter]] and [[module-system]].
