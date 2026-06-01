---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/interpreter/mod.rs, src/interpreter/eval.rs, src/interpreter/global_store.rs, src/interpreter/types.rs, src/interpreter/inference.rs, src/interpreter/resolve.rs, src/interpreter/binding.rs, src/interpreter/include.rs, src/interpreter/load.rs]
---

# interpreter — from parsed `Program` to a `GlobalStore`

> The interpreter takes a parsed [[language-parser|`Program`]] and folds it into a
> persistent store of cells, types, and modules. It is a pure error-collecting
> tree-walk: nothing throws, every step returns an `InterpResult` carrying the
> advanced context plus accumulated errors, holes, and constraints. Diagram and
> map evaluation live in `diagram.rs` / `partial_map.rs` (the latter documented in
> [[core-partial-map]]); this page covers the spine — `eval`, the store, the type
> lookup chain, inference, resolution, binding, the file-loading pipeline, and the
> dotted-expression evaluator in `diagram.rs`.

## What it owns

`src/interpreter/*` is the **semantic layer** between syntax and the core
algebra. It elaborates declarations (`generator`, `let`, `map`, `include`,
`attach`, `for`) into [[core-complex|Complexes]] of [[diagram|diagrams]], assigns
every global entity an opaque id, threads a copy-on-write store through the whole
program, and — for the root file only — runs a constraint solver that recovers
boundary information for `?` holes. The entry points are `interpret_program`
(one `Program` against a `Context`) and `InterpretedFile::load` (a file plus its
whole dependency closure).

## Key public types

| Type / fn | Where | For |
|---|---|---|
| `interpret_program` | `eval.rs` | fold a `Program` into an `InterpResult` |
| `Context` | `types.rs` | the threaded read/write env: `current_module`, `Arc<GlobalStore>`, `resolutions`, `source` |
| `InterpResult` | `types.rs` | `{ context, errors, holes, constraints }`; `#[must_use]`; merged with `merge` |
| `GlobalStore` | `global_store.rs` | persistent store of cells / types / modules |
| `interpret_diagram` | `diagram.rs` (re-exported) | evaluate a diagram expression |
| `InterpretedFile` / `LoadResult` | `load.rs` | the load pipeline's success/failure outcome |
| `inference::{HoleId, HoleEntry, SolvedHole, solve}` | `inference.rs` | hole constraints and the fixpoint solver |
| `PartialHint` | `types.rs` | renderer-only hint for a partially-covered hole |

Note `mod.rs` keeps almost everything private: only `Context`, `InterpResult`,
`interpret_program`, `GlobalStore`, `interpret_diagram`, the inference triple,
`InterpretedFile`/`LoadResult`, and `PartialHint` escape the crate boundary. The
binding/resolve/include machinery is `pub(super)`/`pub`-within-module glue.

## `GlobalStore` — the persistent state

`global_store.rs`. Three tables, all keyed by **opaque id, never by name**:

- `cells: HashMap<GlobalId, CellEntry>` — non-type generators; `CellEntry` wraps
  the `CellData` boundary spec. A parallel `cells_by_dim: HashMap<usize,
  Vec<GlobalId>>` indexes them by dimension (a debug invariant, `assert_invariants`,
  checks every id in `cells_by_dim` is present in `cells`).
- `types: HashMap<GlobalId, TypeEntry>` — each `TypeEntry` is `{ data: CellData,
  complex: Arc<Complex> }`: a type *is* a generator plus the whole [[core-complex|Complex]]
  accumulated inside its body.
- `modules: IndexMap<ModuleId, Arc<Complex>>` — **insertion-ordered** so
  dependencies precede dependents; `modules_iter` relies on this. A side table
  `module_names: HashMap<String, ModuleId>` maps a file's short name (stem, via
  `module_short_name` (internal)) to its canonical path.

`ModuleId` and `LocalId` are both just `String` (`src/aux/id.rs`); a `Tag` is
`Tag::Global(GlobalId)` or `Tag::Local(String)`.

**Mutation is copy-on-write.** `Context::state` is an `Arc<GlobalStore>`; writes
go through `state_mut` → `Arc::make_mut`, and module/type complexes are edited via
`modify_module` / `modify_type_complex`, each `Arc::make_mut`-ing the inner
`Complex`. Sharing the store by `Arc` is what lets a dependency be interpreted
once and then read cheaply by every importer (`Context::new_sharing_state`).

`insert_global_cell` (a free fn, not a method) is the one place a fresh global
generator is minted: allocate `GlobalId::fresh()`, build its classifier diagram,
push generator + diagram into the given `&mut Complex`, return `(gid, dim)`. The
caller must then call `set_cell` to finish registration — `register_generator`
and the `Mode::Global` arm of `interpret_complex_generator` are the two callers
that honour this contract. `register_proof_diagram` is the higher-level door:
it extracts the input/output $\partial_{n}$ via `Diagram::boundary`, builds the
`CellData`, and registers the finished proof term as a first-class generator.

### The type lookup chain (non-obvious)

There is **no `find_type_by_name`**. A type name resolves to its `TypeEntry` only
by going through a module's generator table:

1. `store.find_module(path)` → the module [[core-complex|`Complex`]];
2. `complex.find_generator(name)` → `(&Tag, dim)`;
3. unwrap `Tag::Global(gid)` (a `Tag::Local` here is an internal error);
4. `store.find_type(gid)` → `TypeEntry { complex, .. }`.

When the canonical path is unknown (e.g. from the REPL) `find_type_gid` walks
*every* module via `modules_iter` and returns the first matching generator's
`GlobalId`, sidestepping the canonical-path-vs-source-file key mismatch.
`resolve_module_by_name` does the short-name → `(path, Arc<Complex>)` hop used by
`:: Name` module domains in type blocks. The whole chain lives in `resolve.rs`
(`resolve_type_complex`, `resolve_owner_type_id`, `interpret_address`); the REPL's
`src/interactive/engine.rs` re-splits it into eager-load and interactive-resolve
halves — see [[interactive-engine]].

## Data flow

```
InterpretedFile::load(loader, path)              load.rs
  │  loader.load → parsed root + dep_modules (topologically sorted, leaves first)
  ▼
  for each dep:  interpret_program(dep_ctx, dep.program)     ← state threaded via Arc
  │  (holes in a dependency only WARN — inference is root-only)
  ▼
  interpret_program(root_ctx, root.program)
  │   initialize_module_context        ← mint root 0-cell + empty type
  │   interpret_items over blocks:
  │     @Type block    → interpret_type_inst   (objects, lets, maps, include, index, for)
  │     @Local block   → interpret_complex → interpret_local_inst (let/map/assert/index/for)
  │
  │   ↳ generators      → prepare_generator → insert_global_cell + set_cell
  │   ↳ let / map       → interpret_(let_diag|def_pmap) → binding.rs insert_*_binding
  │   ↳ include/attach  → include.rs (import generators, register inclusion map)
  │   ↳ ? holes         → add_hole + emit Constraints   (diagram.rs / partial_map.rs)
  ▼
  solve(hole_entries, constraints)     inference.rs  ← fixpoint, ROOT ONLY
  ▼
  InterpretedFile { state, solved_holes, source, path }
```

### Blocks, modes, and scopes

`eval.rs` dispatches each top-level `Block` to a `@Type` or `@Local` handler.
The crucial axis is `Mode` (`types.rs`):

- **`Mode::Global`** — definitions are committed to the store and visible to all
  modules; generators get fresh `GlobalId`s.
- **`Mode::Local`** — definitions stay in a temporary `Complex` inside a type
  body; generators get `Tag::Local` and are recorded with `add_local_cell` rather
  than the store.

A `@Type` block may only introduce **0-dimensional** generators (objects);
`interpret_type_generator` rejects any `CellData::Boundary` with *"Higher cells
in @Type blocks are not supported"*. Higher cells live inside a type's `{ … }`
body. Interpreting that body builds a `TypeScope { owner_type_id, working_complex }`
(`types.rs`); after the body is consumed, `working_complex` is committed back to
the store under `owner_type_id`.

### Iteration combinators (`binding.rs`)

Three folds thread state through a list of items, and they differ in error
discipline — a load-bearing distinction:

- `interpret_items` and `interpret_items_in_complex_scope` **collect all errors**
  and keep going, to surface as many diagnostics as possible per run.
- `interpret_items_in_type_scope` **breaks on the first error**: inside a type
  body later instructions depend on a consistent scope, so continuing past a
  failure would spew misleading cascades.

`binding.rs` also owns the `insert_*_binding` family. The subtle one is
`insert_type_binding`: a named diagram or map committed into a *type* must contain
**only global cells** — it refuses bindings with `has_local_labels()`, since a
stored type complex outlives the local scope its `Tag::Local`s name.

### `include` vs `attach` (`include.rs`)

Both import another type/module's generators under a prefix and register a map,
but they differ in what map:

- `include` / `include module` register an **identity inclusion** (`identity_map`)
  and copy generators verbatim (`prefixed_generators` → `insert_generators_by_tag`,
  which skips a tag already present). The two differ in two details:
  `interpret_include_module_instr` (top-level `include module`) always **skips the
  unnamed root** (`prefixed_generators(.., skip_empty_name = true)`) and defaults
  the alias to the module's name; `interpret_include_instr` (in-body `include`)
  copies the root too (`skip_empty_name = false`) and — via `resolve_include` —
  **requires an explicit alias for any dotted (non-local) type**.
- `attach … along m` registers the supplied [[partial-map|partial map]] `m` and,
  for every generator of the attachment **not** already in `m`'s domain, mints a
  *fresh image generator* whose boundary is `m` applied to the source boundary
  (`mapped_cell_data` → `extend_scope_with_attached_generators`). The map is grown
  in lockstep (`insert_raw`) so later generators can refer to earlier images. A
  dimension check rejects a mapped boundary of the wrong dimension.

### `for`-blocks expand textually

`expand_body` (`eval.rs`) substitutes `<var>` in the **raw body text** for each
index value, re-joins with commas, and re-parses via `parse_complex_instrs` /
`parse_type_instrs` / `parse_local_instrs` (see [[language-parser]]). Errors from
the expanded fragment are relocated to the `for`-block's own span by
`relocate_errors`, so a diagnostic points at the loop, not at synthetic text.

## Dotted diagram expressions (`diagram.rs`)

`interpret_dexpr` evaluates a dotted expression like `F.G.d.in.out`. Every
well-formed one is a *prefix of [[partial-map|partial maps]]*, then a *single
basic [[diagram]]*, then a *suffix of boundary operators* (`.in` $=
\partial^-$, `.out` $= \partial^+$). It is computed in two passes —
`decompose` then `execute` — instead of the old eager walk:

- **`decompose`** collects the pieces into a `Decomp` (`Diagram { maps, diagram,
  diagram_span, bds }`, a non-empty `Map { maps }` chain, or `Hole`) doing only
  cheap name lookups and map evaluation — **no composition, no application**. It
  mirrors the eager reading's scoping exactly: the whole-expression qualified-name
  fast path is retried at every prefix level against the outer scope, and fields
  after a map resolve in that map's domain.
- **`execute`** does the heavy work in the efficient order. The boundary suffix is
  taken in **one** `Diagram::boundary(last_sign, n − bds.len(), …)` call — only the
  final operator's polarity and the operator *count* matter, because the
  intermediate boundary ops collapse under the globular identities. Then the maps
  are applied to that (small) boundary from the innermost outward. No composite map
  is ever built.

The payoff is twofold: a boundary suffix collapses to a single direct call rather
than crawling one codimension per step, and a map need only be *total on the
boundary*, not on the whole diagram — a map partial on the interior but defined on
$\partial$ now succeeds where the eager order errored. Correctness rests on
boundary-preservation of maps, $\varphi(\partial x) = \partial(\varphi x)$. Pinned
by `boundary_suffix_collapses_to_one_direct_call`,
`boundary_underflow_is_rejected`, `maps_are_applied_after_the_boundary` (in
`diagram.rs`) and `delta_simplicial_identities_hold` (`tests/interpreter.rs`, the
pure map-chain form). This two-pass scheme replaced an earlier eager dot-access
walk that composed and applied maps codimension-by-codimension.

## Hole inference (`inference.rs`)

`?` holes are resolved in **two phases**, deliberately decoupled:

1. **Collection** — during interpretation, every site that constrains a hole
   pushes a typed `Constraint` into the `InterpResult` (no in-place solving).
   Emission lives in `diagram.rs` (paste/juxtaposition → `ConstraintOrigin::Paste`,
   boundary declarations → `Declaration`, assertions → `Assertion`) and
   `partial_map.rs` (`enrich_holes` → `ConstraintOrigin::PartialMap`).
2. **Solving** — after the whole root file is interpreted, `solve` runs a
   work-queue fixpoint over the collected constraints.

A `HoleId` is a process-unique atomic counter (mirrors `GlobalId`). A `BdSlot` is
a `(sign, dim)` pair naming one boundary; the *principal* slots of an $n$-cell
hole are $(\text{Input}, n{-}1)$ and $(\text{Output}, n{-}1)$. Constraints come
in three atomic flavours — `BoundaryEq`, `DimEq`, `Value` — composites are
decomposed at the emission site. `solve` derives follow-ups as it goes:

- a `Value` derives a `DimEq` from `top_dim()` plus `BoundaryEq` at the two
  principal slots (`solve_value_higher_dim`, `solve_value_infers_dim`);
- a `BoundaryEq` at $(s, k)$ cascades to `BoundaryEq` at every $(s', j<k)$ via
  `globular_sub_boundaries` (`solve_globular_cascade`).

Each slot/`dim`/`value` is set **at most once** (`Empty → Known`); later
agreeing constraints are dropped (first origin wins), disagreeing ones append to
`inconsistencies` rather than abort (`solve_conflicting_dim_eq`,
`solve_boundary_eq_conflict_records_inconsistency`). This monotone state machine
is what guarantees termination.

## Non-obvious invariants & gotchas

- **Inference is root-only.** `load.rs` runs `solve` solely on the root module's
  holes/constraints. A hole in a *dependency* is reported as a warning
  (`report_hole`), never solved — the user must close it.
- **`insert_global_cell` is half a registration.** It mutates the complex and
  returns `(gid, dim)`, but does **not** touch the store's `cells` table. Forget
  the follow-up `set_cell` and the generator exists in the complex yet is invisible
  to `find_cell`/`cell_data_for_tag`.
- **`cell_data_for_tag` searches cells *then* types** for a `Tag::Global`, and the
  module's local cells for a `Tag::Local`. Types are reachable by tag too, because
  a type is also a generator.
- **`#[must_use]` on `InterpResult`** — it carries errors and holes that must be
  propagated; dropping one silently loses diagnostics.
- **Module order matters.** `modules` is an `IndexMap`; dependency-before-dependent
  order is an invariant `modules_iter` and the load loop both rely on. A `HashMap`
  here would break topological replay.
- **`assert_invariants` is debug-only.** It `debug_assert!`s `cells`/`cells_by_dim`
  consistency (every id in `cells_by_dim` is also in `cells`); module/type
  *presence* is checked separately by inline `debug_assert!`s in `modify_module` /
  `modify_type_complex`. Release builds trust the caller.
- **Type bindings forbid local labels.** `insert_type_binding` rejects diagrams or
  maps with `Tag::Local`s; a committed type complex must be self-contained.

## Mathematics

This layer is the bridge from concrete syntax to the algebra the rest of the
codebase computes with — most of its content is plumbing, but three genuine
mathematical relationships run through it.

- **[[module-system]].** `@Type`/`@Local` blocks, `open`/`include`, and
  `attach … along` are the surface of alifib's module-and-type system. `include`
  realises the inclusion of one type's [[core-complex|Complex]] into another via an
  identity [[partial-map|map]]; `attach` realises a pushout-style extension along a
  given partial map (`extend_scope_with_attached_generators`). The `MapDomain::{Type,Module}`
  distinction in `resolve.rs` is exactly the type-vs-module split of the system.
  See the open question [[module-open-semantics]] for the `open`/`include` boundary.
- **[[diagram]].** Every generator's classifier, every `let` binding, and every
  hole's recovered boundary is a `Diagram`. Inference reasons purely in terms of
  $\partial^\pm_k$ on diagrams (`Diagram::boundary_normal`), and the globular
  cascade is the statement that fixing a $k$-boundary fixes all lower
  $j$-boundaries. See [[core-diagram]].
- **[[partial-map]].** `attach` and the inference of partial-map holes apply a
  [[partial-map|`PartialMap`]] to diagram boundaries (`PartialMap::apply`). The map
  machinery itself — refinement, totality, application — is documented in
  [[core-partial-map]]; this page only *invokes* it.

Pretty-printing is a support relationship, not a realisation: `GlobalStore`
implements `Display` by deferring to `normalize` in [[output]]
(`src/output/normalize.rs`), which flattens the id-keyed store into a name-keyed
render tree.
