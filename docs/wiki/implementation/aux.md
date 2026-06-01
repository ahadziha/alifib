---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/aux/mod.rs, src/aux/id.rs, src/aux/error.rs, src/aux/loader.rs, src/aux/bitset.rs, src/aux/intset.rs, src/aux/graph.rs, src/aux/path.rs]
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
identifier zoo (`pub use id::{GlobalId, LocalId, ModuleId, Tag}`, `pub use
error::Error`) and carries one stray helper, `dim_subscript`, that renders a
dimension as Unicode subscript digits for boundary slot display
(`src/output/normalize.rs`, `src/interpreter/inference.rs`).

## What it owns

| Module | Responsibility |
|---|---|
| `id.rs` | the three identifier kinds + `Tag`, the local/global union |
| `error.rs` | `Error` (message + notes) and `report_load_file_error` |
| `loader.rs` | `Loader` — read source, resolve `#use` includes, return `LoadedFile` |
| `path.rs` | canonicalisation and search-path dedup |
| `bitset.rs` | `BitSet` — dense membership over `0..N` for traversal scratch |
| `intset.rs` | `IntSet = Vec<usize>` kept sorted/deduped + merge ops |
| `graph.rs` | `DiGraph` + topological-sort (single and enumerated) |

## Key public types

- **`GlobalId(usize)`** (`id.rs`) — process-unique, allocated atomically from a
  single `static AtomicUsize` via `GlobalId::fresh` (`Ordering::SeqCst`). Opaque;
  never construct directly. `Display` prints `#n`. This is the spine of the
  global store's cell identity.
- **`LocalId = String`**, **`ModuleId = String`** — type aliases. A `ModuleId` is
  *always* a canonical absolute path (`std::fs::canonicalize`), so two spellings
  of one file never become two modules.
- **`Tag`** = `Local(LocalId)` | `Global(GlobalId)` — the identifier union that
  threads through elaboration. `Local` tags are scoped to the enclosing type or
  module complex; `Global` tags name finalised cells. Central to the
  [[interpreter]] lookup chain. Its `Ord` is total and *segregated*: all `Local`
  tags sort below all `Global` tags.
- **`Error { message, notes }`** (`error.rs`) — a diagnostic accumulator;
  `with_note` chains. `report_load_file_error` is the only printer, fanning a
  `LoadFileError` out to stderr (and into `language::report_errors` for parse
  failures).
- **`Loader`** (`loader.rs`) — search paths + a pluggable `read_file` closure
  (`Arc<dyn Fn>`). `Loader::default` seeds cwd + `ALIFIB_PATH` + extras;
  `with_virtual_files` swaps in an in-memory map for tests.
- **`LoadedFile`**, **`ModuleResolutions`**, **`ResolvedModule`** — the loader's
  output: root program, source, the `(parent, module_name) → canonical_path`
  resolution map, and dependency modules in topological (leaves-first) order.
- **`path`** (`path.rs`) — three free functions, no types. `canonicalize` is the
  best-effort variant (falls back to the input string); `canonicalize_existing`
  is the strict variant (`Result`, no fallback); `normalize_search_paths`
  canonicalises a path list (best-effort) and dedups while preserving order. The
  loader keys modules on the strict canonical path — the same value the engine
  binds as `canonical_path` and uses as the [[interactive-repl|REPL]] store key
  (`src/interactive/engine.rs` `load_file_context`).
- **`BitSet`** *(internal)* — `Vec<u64>` words + cached `count`. Word-level
  `union` / `difference_inplace`; `reset` / `copy_from` reuse the allocation.
- **`IntSet = Vec<usize>`** — not a struct, a contract: sorted and deduplicated.
  Free functions `insert`, `union`, `difference`, `is_disjoint`, `collect_sorted`
  all assume and preserve that invariant. `intersection` exists with the same
  signature but is `#[allow(dead_code)]` and has **no callers anywhere** (not even
  tests) — see `source-drift.md`; do not treat it as live.
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

## Non-obvious invariants & gotchas

- **`GlobalId` is monotone and never reused.** A single global atomic counter,
  never reset. IDs are unique for the process lifetime, not per-store — two
  stores built in one run draw from the same well.
- **`Tag`'s ordering segregates kinds.** `Local(_) < Global(_)` always, before
  comparing payloads. Anything that sorts `Tag`s (canonical orderings, dedup)
  inherits this; do not assume lexical order across the two arms.
- **`ModuleId` must be canonical or modules duplicate.** `path::canonicalize`
  *falls back to the input string* on failure (for paths that may not exist);
  `canonicalize_existing` *errors* instead. The loader uses the strict variant
  for module cache keys precisely so a fallback can never silently fork a module
  into two. Picking the wrong one is the classic latent bug here.
- **`BitSet::contains` is bounds-safe; `insert`/`remove` are not.** `contains`
  guards `w < self.bits.len()`; the mutators index directly. Size the universe
  correctly via `new` / `reset` before inserting. `BitSet` exists *only* to give
  `ogposet::traverse` pre-allocated scratch reused across iterations — see the
  `scratch_in` / `scratch_out` pool in `src/core/ogposet.rs`.
- **`IntSet` is a discipline, not a type.** The alias buys nothing the compiler
  enforces; every mutation must go through `intset::insert` or stay sorted by
  construction. The payoff is O(n+m) merges and binary-search membership on the
  small (1–8 element) face/coface sets that fill `Diagram`/`Ogposet` — see
  `cofaces_to_top` and the `faces_*` builders in `src/core/diagram.rs`.
- **`DiGraph::add_edge` is idempotent** (sorted insert dedups), and the dual
  adjacency means `add_edge(u,v)` writes to *both* `successors[u]` and
  `predecessors[v]`.
- **Two topological-sort entry points, two callers.** `topological_sort` is plain
  Kahn (single order, `Err(())` on cycle). `try_topological_sorts` *enumerates*
  orders by backtracking, calling `f` on each until one returns `Some`, capped by
  a `limit`. `src/core/reconstruct.rs` uses the enumerator with `limit = 10_000`
  to find a topological order whose layering realises correctly, and falls back
  to the single sort otherwise.

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
[[core-complex]] (where `Tag` keys the lookup chain).
