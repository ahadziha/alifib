---
kind: impl
status: stable
last-touched: 2026-06-09
code: [src/core/complex.rs]
---

# core-complex — `Complex`

> A `Complex` is the local sky over one type or module: every generator, every
> let-bound [[diagram]], every [[partial-map|map]], and every locally-scoped cell
> visible while that type or module is elaborated. A pure namespace with
> redundant indices and debug-checked invariants — no mathematics of its own, the
> carrier on which the mathematics is hung.

## What it owns

`Complex` is the *environment* threaded through elaboration of a single type or
module body. It answers "what is in scope, by name, by tag, or by dimension?"
It stores no global state: cells whose boundary lives in the global tables are
referenced by `Tag::Global`; cells private to this body carry `Tag::Local` and
keep their boundary data here. Every mutator (bar one, see gotchas) re-runs
`Complex::assert_invariants` (internal) in debug builds, so the redundant
indices below can never silently disagree.

## Key public types

| Type | Role |
|---|---|
| `Complex` | the namespace itself: `generators`, `diagrams`, `maps`, `local_cells`, `indices`, `used_names` (all fields private) |
| `MapDomain` | the source side of a stored map — `Type(GlobalId)` or `Module(ModuleId)` |
| `Generators` *(internal)* | three parallel indices over generators: `by_name`, `by_tag`, `by_dim`, plus `classifiers` and a `next_order` counter |
| `GeneratorEntry` *(internal)* | a generator's `tag`, `dim`, and `insertion_order` |
| `MapEntry` *(internal)* | a stored map: the hole-free `PartialMap`, its `MapDomain`, and pending `holes: Vec<MapHole>` |
| `LocalCells` *(internal)* | `by_id` (name → `CellData`) and a `by_dim` index for body-scoped cells |

Five stores, five flavours of inhabitant:

- **Generators** — named cells with a `Tag` and a dimension. Registered by
  `Complex::add_generator(name, tag, classifier)`, which derives `dim` from
  `classifier.top_dim()` and (debug-)asserts `classifier.top_label() == Some(tag)`.
  Looked up by `find_generator` → `Some((&Tag, dim))`, by tag via
  `find_generator_by_tag`, iterated by `generators_iter` / `generators_iter_by_dim`;
  the classifier is returned by `classifier(name)`.
- **Diagrams** — `add_diagram(name, diagram)` / `find_diagram` / `diagrams_iter`.
  Holds both classifiers *and* let-bound values (see below).
- **Maps** — `add_map(name, domain, map, holes)` stores the *hole-free*
  [[partial-map|`PartialMap`]] with its `MapDomain` plus any unfilled `MapHole`s
  (`arr => ?` clauses). `find_map` → `Some((&PartialMap, &MapDomain))` — the
  hole-free part only; the pending entries surface through `map_holes(name)`.
  See [[core-partial-map]] for the [[hole]] machinery.
- **Local cells** — `add_local_cell(name, dim, data)` keeps a `CellData` whose
  boundary lives only here, reached by `find_local_cell`.
- **Indices** — `add_index(name, Vec<String>)` / `find_index`: named string lists
  (e.g. the `"thin"` index read in `src/interactive/web.rs`).

## Data flow — generator vs let-binding

The single most load-bearing fact: `add_generator` does **not** also store a
diagram. The classifier lives in `generators.classifiers`; making it retrievable
via `find_diagram` is the *caller's* second call. The interpreter consistently
issues both together:

```
generator (name : src -> tgt):
    add_generator(name, tag, classifier)   # generators.classifiers + indices
    add_diagram(name, classifier)          # so find_diagram(name) resolves too

let-binding (let name = expr):
    add_diagram(name, value)               # ONLY this; no generators entry
```

Both forms therefore answer `find_diagram(name)`. To tell them apart:
`find_generator(name)` is `Some((tag, dim))` for a generator and `None` for a
let-binding; `generators_iter()` yields only generators. The pairing is enacted
in `src/interpreter/eval.rs` (local cells: `add_generator`, `add_diagram`,
`add_local_cell`; module root and type definition likewise) and in
`src/interpreter/global_store.rs::insert_global_cell` (a free function, not a
method), which stores the optional proof-term diagram if given, else the
classifier: `add_diagram(name, diagram.unwrap_or(classifier))`.

**`attach`/`include` is the deliberate exception.**
`src/interpreter/include.rs::extend_scope_with_attached_generators` (and
`insert_generators_by_tag`) call `add_generator` *without* a paired
`add_diagram` (in `Mode::Local` they also `add_local_cell` the image boundary).
An attached generator is therefore reachable by name (`find_generator`), by tag,
and through its `classifier`, but **not** through `find_diagram`. Don't read
"every generator answers `find_diagram`" as universal: it holds for cells
declared in a type/module body, not for ones imported by attachment.

A generator additionally records an **insertion order** (`generator_order`),
consumed by `src/output/normalize.rs` (which sorts `generators_iter()` by
`generator_order`) to emit generators in declaration order rather than the
`BTreeMap`'s lexical order. The fixture round-trip tests
`delta_simplicial_identities_hold` and `magma_interpretation` exercise this
registration-and-normalisation path end-to-end.

## Boundary extraction

Generator boundaries are reachable two ways. From the classifier diagram
directly, slicing its $(\dim - 1)$-[[boundary]] with `Diagram::boundary` /
`boundary_normal` (the $k=0$ case has no boundary — guard with `checked_sub(1)`,
as `src/output/normalize.rs::cell_from_diagram` does). Or, going through the
global store, `GlobalStore::cell_data_for_tag(complex, tag)`:

- `Tag::Global(gid)` → the global cell/type's `CellData`
  (`CellData::Zero` for a $0$-cell, `CellData::Boundary { boundary_in,
  boundary_out }` for higher, where *in* is $\partial^-$ and *out* $\partial^+$);
- `Tag::Local(name)` → `complex.find_local_cell(name)` — exactly the
  body-scoped boundary stored by `add_local_cell`;
- `Tag::Hole(_)` → `None` — a metavariable's boundary lives on its `MapHole`
  record, not in any store.

## Non-obvious invariants and gotchas

- **`generators.classifiers` and `diagrams` are separate stores.**
  `find_diagram(name)` only resolves a generator's classifier because the caller
  also called `add_diagram` (see data flow above). Forget that second call and
  the generator becomes invisible to diagram lookup.
- **`used_names` ignores generators and local cells.** `add_diagram`, `add_map`,
  `add_index` populate `used_names`; `add_generator` and `add_local_cell` do
  *not*. So `name_in_use` answers "taken by a diagram/map/index". A
  body-declared generator's name lands in `used_names` anyway via its paired
  `add_diagram` — but an *attached* generator's does **not**. The one site that
  checks generators explicitly is `src/interactive/engine.rs`
  (`name_in_use(n) || find_generator(n).is_some()`);
  `src/interpreter/types.rs::ensure_name_free` checks only `name_in_use`.
- **Three indices, one truth.** `Generators` keeps `by_name`, `by_tag`,
  `by_dim`, and `classifiers` redundantly for O(1) lookup each way.
  `assert_invariants` (internal) enforces their agreement in debug builds:
  `by_name.len() == classifiers.len()`, every `by_name` entry has a matching
  `by_tag` and `by_dim` membership, every diagram/map name is in `used_names`,
  and every `local_cells.by_dim` membership has a `by_id` entry. Trust the
  asserts, not your memory, when adding a mutator.
- **`by_name` is a `BTreeMap`, `by_tag`/`classifiers` are `HashMap`.** Iteration
  order over generators is lexical by name unless you go through
  `generator_order`. `by_dim` values are `BTreeSet` (sorted) for generators but
  plain `HashSet` for local cells — don't assume a stable order for the latter.
- **`add_index` is the lone mutator that skips `assert_invariants`.** It only
  touches `indices` and `used_names`, both unconstrained, so there is nothing to
  check.

## Mathematics

A `Complex` realises no theorem; it is the *ambient namespace* of one type or
module — and a type is a [[directed-complex]], not necessarily regular. Its
generators are the [[atom|atoms]] (each carries a classifier [[diagram]] encoding
its boundary), and the diagrams it stores are [[molecule|molecules]] built by
pasting those atoms; atoms and molecules are the [[regular-directed-complex|regular]]
*shapes*. The maps are [[partial-map|partial maps]] between the underlying
complexes. So the bridge here is a *support* relationship: `Complex` holds the
data the mathematics operates on, rather than implementing an operation of the
theory. See [[core-diagram]] for `Diagram` / `CellData` / `Sign`,
[[core-matching]] for how `classifier` and `find_generator_by_tag` feed the
rewriting engine, and [[interpreter]] for how a `Complex` sits inside
`GlobalStore`.
