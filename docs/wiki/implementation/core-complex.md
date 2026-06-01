---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/core/complex.rs]
---

# core-complex — `Complex`

> A `Complex` is the local sky over one type or module: every generator, every
> let-bound [[diagram]], every [[partial-map|map]], and every locally-scoped cell
> visible while that type or module is elaborated. It is a pure namespace with
> redundant indices and debug-checked invariants — no mathematics of its own, the
> carrier on which the mathematics is hung.

## What it owns

`Complex` is the *environment* threaded through elaboration of a single type or
module body. It answers "what is in scope, by name, by tag, or by dimension?"
It stores no global state: cells whose boundary lives in the global tables are
referenced by `Tag::Global`; cells private to this body carry `Tag::Local` and
keep their boundary data here. Every mutator re-runs `Complex::assert_invariants`
(internal) in debug builds, so the redundant indices below can never silently
disagree.

## Key public types

| Type | Role |
|---|---|
| `Complex` | the namespace itself: `generators`, `diagrams`, `maps`, `local_cells`, `indices`, `used_names` (all fields private) |
| `MapDomain` | the source side of a stored map — `Type(GlobalId)` or `Module(ModuleId)` |
| `Generators` *(internal)* | three parallel indices over generators: `by_name`, `by_tag`, `by_dim`, plus `classifiers` and a `next_order` counter |
| `GeneratorEntry` *(internal)* | a generator's `tag`, `dim`, and `insertion_order` |
| `LocalCells` *(internal)* | `by_id` (name → `LocalCellEntry`, which wraps the cell's `CellData`) and a `by_dim` index for body-scoped cells |

Five maps, five flavours of inhabitant:

- **Generators** — named cells with a `Tag` and a dimension. Registered by
  `Complex::add_generator(name, tag, classifier)`, which derives `dim` from
  `classifier.top_dim()` and (debug-)asserts `classifier.top_label() == Some(tag)`.
  Looked up by `find_generator` → `Some((&Tag, dim))`, by tag via
  `find_generator_by_tag`, iterated by `generators_iter` / `generators_iter_by_dim`.
- **Diagrams** — `add_diagram(name, diagram)` / `find_diagram`. Holds both
  classifiers *and* let-bound values (see below).
- **Maps** — `add_map(name, domain, map)` stores a [[partial-map|`PartialMap`]]
  with its `MapDomain`; `find_map` → `Some((&PartialMap, &MapDomain))`.
- **Local cells** — `add_local_cell(name, dim, data)` keeps a `CellData` whose
  boundary lives only here, reached by `find_local_cell`.
- **Indices** — `add_index(name, Vec<String>)` / `find_index`: named string lists
  (e.g. the `"thin"` index read in `src/interactive/web.rs`).

## Data flow — generator vs let-binding

The single most load-bearing fact, and the one the prior draft got wrong:
`add_generator` does **not** also store a diagram. Storing the classifier as a
retrievable diagram is the *caller's* second call. The interpreter consistently
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
let-binding; `generators_iter()` yields only generators. This pairing is enacted
at `src/interpreter/eval.rs` (local cells: `add_generator` then `add_diagram`
then `add_local_cell`; module root and type definition do the same), in
`src/interpreter/global_store.rs::insert_global_cell` (a free function, not a
method: `add_generator` then `add_diagram`), and in
`src/interpreter/include.rs`. The classifier is the
boundary-encoding [[diagram]] for the generator.

A generator additionally records an **insertion order** (`generator_order`),
consumed by `src/output/normalize.rs` to emit generators in declaration order
rather than the `BTreeMap`'s lexical order.

## Boundary extraction

Generator boundaries are reachable two ways. From the classifier diagram
directly, slicing its $(\dim - 1)$-[[boundary]] with `Diagram::boundary` /
`boundary_normal` (the $k=0$ case has no boundary — guard with `checked_sub(1)`,
as `src/output/normalize.rs::cell_from_diagram` does). Or, going through the
global store, `GlobalStore::cell_data_for_tag(complex, tag)`:

- `Tag::Global(gid)` → the global cell/type's `CellData`
  (`CellData::Zero` for a $0$-cell, `CellData::Boundary { boundary_in,
  boundary_out }` for higher, where *in* is the source $\partial^-$ and *out* the
  target $\partial^+$);
- `Tag::Local(name)` → `complex.find_local_cell(name)` — exactly the
  body-scoped boundary stored by `add_local_cell`.

## Non-obvious invariants and gotchas

- **`add_generator` ≠ classifier-as-diagram.** The classifier is held in
  `generators.classifiers` and returned by `classifier(name)`; it is a *separate*
  store from `diagrams`. `find_diagram(name)` only resolves a generator's
  classifier because the caller also called `add_diagram`. Forget that second
  call and the generator becomes invisible to diagram lookup.
- **`used_names` ignores generators.** `add_diagram`, `add_map`, `add_index`
  populate `used_names`; `add_generator` does *not*. So `name_in_use` answers
  "taken by a diagram/map/index" — but since a generator always also gets an
  `add_diagram` (see the data-flow above), its name lands in `used_names` that
  way. The one site that checks generators explicitly anyway is
  `src/interactive/engine.rs` (`name_in_use(n) || find_generator(n).is_some()`);
  `src/interpreter/types.rs::ensure_name_free` checks only `name_in_use`.
- **Three indices, one truth.** `Generators` keeps `by_name`, `by_tag`,
  `by_dim`, and `classifiers` redundantly for O(1) lookup each way.
  `assert_invariants` (internal) enforces their agreement in debug builds:
  `by_name.len() == classifiers.len()`, every `by_name` entry has a matching
  `by_tag` and `by_dim` membership, and every `local_cells.by_dim` membership has
  a `by_id` entry. Trust the asserts, not your memory, when adding a mutator.
- **`by_name` is a `BTreeMap`, `by_tag`/`classifiers` are `HashMap`.** Iteration
  order over generators is lexical by name unless you go through
  `generator_order`. `by_dim` values are `BTreeSet` (sorted) for generators but
  plain `HashSet` for local cells — don't assume a stable order for the latter.
- **`add_index` is the lone mutator that skips `assert_invariants`.** It only
  touches `indices` and `used_names`, both unconstrained, so there is nothing to
  check.

## Mathematics

A `Complex` realises no theorem; it is the *ambient namespace* in which the
combinatorics of a [[regular-directed-complex]] are assembled and named. Its
generators are the [[atom|atoms]] (each carries a classifier [[diagram]] encoding
its boundary), and the diagrams it stores are [[molecule|molecules]] built by
pasting those atoms. The maps are [[partial-map|partial maps]] between the
underlying complexes. So the bridge here is a *support* relationship: `Complex`
holds the data the mathematics operates on, rather than implementing an operation
of the theory. See [[core-diagram]] for `Diagram` / `CellData` / `Sign`,
[[core-matching]] for how `classifier` and `find_generator_by_tag` feed the
rewriting engine, and [[interpreter]] for how a `Complex` sits inside
`GlobalStore`.
