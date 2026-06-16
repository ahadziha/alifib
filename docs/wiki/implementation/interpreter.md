---
kind: impl
status: stable
last-touched: 2026-06-13
code: [src/interpreter/mod.rs, src/interpreter/eval.rs, src/interpreter/global_store.rs, src/interpreter/types.rs, src/interpreter/resolve.rs, src/interpreter/binding.rs, src/interpreter/include.rs, src/interpreter/load.rs, src/interpreter/diagram.rs, src/interpreter/partial_map.rs]
---

# interpreter — from parsed `Program` to a `GlobalStore`

> The interpreter takes a parsed [[language-parser|`Program`]] and folds it into a
> persistent store of cells, types, and modules. It is a pure error-collecting
> tree-walk: nothing throws, every step returns an `InterpResult` carrying the
> advanced context plus accumulated errors. Diagram and map evaluation live in
> `diagram.rs` / `partial_map.rs` (the latter documented in [[core-partial-map]]);
> this page covers the spine — `eval`, the store, the type lookup chain,
> resolution, binding, the file-loading pipeline, and the dotted-expression
> evaluator in `diagram.rs`.

## What it owns

`src/interpreter/*` is the **semantic layer** between syntax and the core
algebra. It elaborates declarations (`generator`, `let`, `map`, `include`,
`attach`, `for`) into [[core-complex|Complexes]] of [[diagram|diagrams]], assigns
every global entity an opaque id, and threads a copy-on-write store through the
whole program. Unfilled `?` [[hole|holes]] in a map are not errors — they are
carried on the map itself (`Complex::map_holes`) for the interactive layer to
list and fill, not resolved by a solver pass. The entry
points are `interpret_program` (one `Program` against a `Context`) and
`InterpretedFile::load` (a file plus its whole dependency closure).

## Key public types

| Type / fn | Where | For |
|---|---|---|
| `interpret_program` | `eval.rs` | fold a `Program` into an `InterpResult` |
| `Context` | `types.rs` | the threaded read/write env: `current_module`, `Arc<GlobalStore>`, `resolutions`, `source` |
| `InterpResult` | `types.rs` | `{ context, errors }`; `#[must_use]`; merged with `merge` |
| `GlobalStore` | `global_store.rs` | persistent store of cells / types / modules |
| `interpret_diagram` | `diagram.rs` (re-exported) | evaluate a diagram expression |
| `EvalMap { map, domain, holes }` | `types.rs` | an evaluated [[partial-map|map]] and its unfilled [[hole|holes]] |
| `InterpretedFile` / `LoadResult` | `load.rs` | the load pipeline's success/failure outcome |

`mod.rs` keeps almost everything private: only these symbols (and the `load`
module) escape the crate boundary. The binding/resolve/include machinery is
`pub fn`s inside private modules — crate-internal glue.

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
  dependencies precede dependents; `modules_iter` exposes that order. A side table
  `module_names: HashMap<String, ModuleId>` maps a file's short name (stem, via
  `module_short_name` (internal)) to its canonical path.

`ModuleId` and `LocalId` are both just `String` (`src/aux/id.rs`); a `Tag` is
`Tag::Global(GlobalId)`, `Tag::Local(String)`, or `Tag::Hole(Box<Tag>)` — the last
is a map-hole metavariable wrapping its *source generator's* tag, for which
`cell_data_for_tag` returns `None`: a hole's boundary lives on its `MapHole`
record, not in the store.

**Mutation is copy-on-write.** `Context::state` is an `Arc<GlobalStore>`; writes
go through `state_mut` → `Arc::make_mut`, and module/type complexes are edited via
`modify_module` / `modify_type_complex`, each `Arc::make_mut`-ing the inner
`Complex`. Sharing the store by `Arc` is what lets a dependency be interpreted
once and then read cheaply by every importer (`Context::new_sharing_state`).

`insert_global_cell` (a free fn, not a method) is the one routine that mints a
generator *together with its classifier diagram* into a complex: allocate
`GlobalId::fresh()`, build the classifier, push generator + diagram into the given
`&mut Complex`, return `(gid, dim)`. The caller must then call `set_cell` to
finish registration — `register_generator` and the `Mode::Global` arm of
`interpret_complex_generator` are the two callers that honour this contract.
(Root cells, the type cells of `interpret_type_generator`, and `attach` images
mint their `GlobalId`s separately.) `register_proof_diagram` is the higher-level
door: it extracts the input/output boundaries at $\dim - 1$ via
`Diagram::boundary`, builds the `CellData`, and registers the finished proof term
as a first-class generator.

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

### Qualified names walk through module maps (non-obvious)

A dotted address like `A.Aux.Ob` is **not** a flat string key — `interpret_address`
(`resolve.rs`) treats every segment but the last as a *prefix* and walks it through
`resolve_address_prefix_scope` (internal). Starting in the current module's complex,
each prefix segment is looked up with `Complex::find_map`; its `MapDomain` must be
`Module(id)` (an `include` registers exactly such a map under its alias), and the
scope advances to `find_module_arc(id)`. A `MapDomain::Type` prefix, or a segment
that names no map, is rejected (*"Domain of `…` is not a module"* / *"Partial map
`…` not found"*). The final segment is resolved in the advanced scope by
`type_id_of_named_diagram`. So a qualified name is resolved **relative to the
alias maps in scope where it is written**, not against a global table — two
modules each including a module named `Aux` resolve `…Aux.Ob` to their own copy
(the [[module-system]] concept page covers the semantics).

Two related entry points sit alongside it: `resolve_module_domain` resolves the
single-segment name of a `:: Name` block (it rejects a dotted address) via
`resolve_module_by_name`; and `resolve_owner_type_id` falls back to the module's
unnamed root generator when the address is empty. The scoped *file* search that
backs `include <Name>` is owned by [[aux]] and consumed here through
`Context::resolutions`; `virtual_loader_subdirectory_resolution`
(`tests/interpreter.rs`) pins both the same-named-subdirectory file resolution and
the `A.Aux.Ob` / `B.Aux.Ob` qualified-name walk in one fixture.

## Data flow

```
InterpretedFile::load(loader, path)              load.rs
  │  loader.load → parsed root + dep_modules (topologically sorted, leaves first)
  ▼
  for each dep:  interpret_program(dep_ctx, dep.program)     ← state threaded via Arc
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
  │   ↳ ? holes         → recorded on the map (EvalMap::holes; see [[core-partial-map]])
  ▼
  InterpretedFile { state, source, path }
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
the store under `owner_type_id` — but not before `interpret_type_generator`
plants an **identity self-map** named after the type (`MapDomain::Type`) inside
it, which is what later lets `X.d` resolve in scopes built on `X`'s complex (a
name collision with an inherited map is an error).

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
  (`mapped_cell_data` → `extend_scope_with_attached_generators`) — a fresh
  `GlobalId` in `Mode::Global`, a local cell in `Mode::Local`. The map is grown
  in lockstep (`insert_raw`) so later generators can refer to earlier images. A
  dimension check rejects a mapped boundary of the wrong dimension, and an
  `along` map may not contain holes (*"Holes (`?`) are not supported in `attach`
  clauses"*).

### `for`-blocks expand textually

`expand_body` (`eval.rs`) substitutes `<var>` in the **raw body text** for each
index value (`exclude` indices are subtracted first, `resolve_index_values`),
re-joins with commas, and re-parses via `parse_complex_instrs` /
`parse_type_instrs` / `parse_local_instrs` / `parse_pmap_clauses` — the last for
`for` inside a map-extension block (`expand_pmap_for`, `partial_map.rs`); see
[[language-parser]]. Errors from the expanded fragment are relocated to the
`for`-block's own span by `relocate_errors`, so a diagnostic points at the loop,
not at synthetic text.

## Pasting: juxtaposition is principal pasting (`diagram.rs`)

A diagram AST node is dispatched by `interpret_diagram_as_term` to one of two forms.

- **Explicit paste** `lhs #k rhs…` → `interpret_paste`. The dimension `k` is parsed
  from the `#k` token by `parse_paste_dim`; the RHS is evaluated *first*
  (it determines the context for the left), then both are pasted with
  `Diagram::paste(k, …)` — pasting along the shared $k$-boundary, i.e. $\#_k$.
- **Juxtaposition** `f g h…` (a `PrincipalPaste` run) → `interpret_sequence_as_term`.
  A single expression is just its term; a multi-expression run is folded
  left-to-right and **pasted at the principal dimension**
  $k = \min(\dim f, \dim g) - 1$ — in code, `prev.top_dim().min(d_right.top_dim())
  .checked_sub(1)`, with a `None` (underflow below 0) raising *"principal paste
  dimension is below 0"*.

Juxtaposition is therefore **pasting, not composition**: `f g` is exactly `f #k g`
at $k = \min(\dim f, \dim g) - 1$, the codimension-1 boundary the two diagrams share.
There is no separate composition operator — every binary operation here is a paste
$\#_k$ along a shared $k$-boundary, with juxtaposition choosing $k$ implicitly.

Two more component forms live here:

- **`(run auto on d)`** (`DComponent::Run` → `interpret_run_auto`) evaluates `d`,
  then loops `greedy_parallel_auto_step` ([[core-matching]]) up to
  `AUTO_STEP_LIMIT` (1024) rounds, pasting each parallel rewrite step at the
  diagram's top dimension into a single proof diagram — the [[rewriting]] trace
  as one $(\!n{+}1)$-dimensional term. No applicable rule returns the input
  unchanged; still-rewritable at the limit is an error.
- **`assert`** (`interpret_assert` / `check_assert`) compares diagrams up to
  `Diagram::isomorphic`, and maps **pointwise** over their shared domain's
  generators — which must be the *same* `Arc<Complex>` by `Arc::ptr_eq`, else the
  sides are *"incomparable"*. `interpret_anon_map_component` deliberately swaps
  in the canonical store `Arc` for an address-only target so this pointer
  identity holds for anonymous maps.

## Dotted diagram expressions (`diagram.rs`)

`interpret_dexpr` evaluates a dotted expression like `F.G.d.in.out`. Every
well-formed one is a *prefix of [[partial-map|partial maps]]*, then a *single
basic [[diagram]]*, then a *suffix of boundary operators* (`.in` $=
\partial^-$, `.out` $= \partial^+$). It is computed in two passes —
`decompose` then `execute`:

- **`decompose`** collects the pieces into a `Decomp` (`Diagram { maps, diagram,
  diagram_span, bds }` or a non-empty `Map { maps }` chain) doing only
  cheap name lookups and map evaluation — **no composition, no application**. It
  mirrors the eager reading's scoping exactly: the whole-expression qualified-name
  fast path is retried at every prefix level against the outer scope, and fields
  after a map resolve in that map's domain.
- **`execute`** does the heavy work in the efficient order. The boundary suffix is
  taken in **one** `Diagram::boundary(last_sign, n − bds.len(), …)` call — only the
  final operator's polarity and the operator *count* matter, because the
  intermediate boundary ops collapse under the globular identities. Then the maps
  are applied to that (small) boundary from the innermost outward — for a
  diagram-valued expression no composite map is built (`PartialMap::apply`, on
  the concrete boundary). A **pure map chain** (a `Decomp::Map`, no diagram) is
  the exception: it *is* composed, innermost outward, via `compose_with_holes`,
  which propagates the maps' holes and conditionals into the resulting
  `Term::Map`.

The payoff is twofold: a boundary suffix collapses to a single direct call rather
than crawling one codimension per step, and a map need only be *total on the
boundary*, not on the whole diagram — a map partial on the interior but defined on
$\partial$ now succeeds where the eager order errored. Correctness rests on
boundary-preservation of maps, $\varphi(\partial x) = \partial(\varphi x)$. Pinned
by `boundary_suffix_collapses_to_one_direct_call`,
`boundary_underflow_is_rejected`, `maps_are_applied_after_the_boundary` (in
`diagram.rs`) and `delta_simplicial_identities_hold` (`tests/interpreter.rs`, the
pure map-chain form).

## Holes live on the map

A `?` hole is a *pending assignment* recorded directly on the map being built, not
resolved by a dedicated solver phase. `partial_map.rs` threads a `MapBuild`
through the clauses, committing what it can and leaving a `MapHole` where
information is incomplete; the leftover holes ride out on `EvalMap::holes` into the
type complex (`Complex::map_holes`). Resolution is the local inference (case-1,
collapse, forced faces) and conditional `cascade` of [[core-partial-map]] at
interpretation time, plus the interactive `fill` workflow ([[interactive-session]])
afterward. Note the asymmetry in name lookup: a named map used in *map* position
(`eval_partial_map_basic`) carries its stored holes — so `F [ … ]` extends a
map-with-holes, i.e. fills, and a dotted composite `F.G` propagates them through
`compose_with_holes` — while in *diagram* position (`interpret_dcomponent`), where
a map is *applied* to a concrete diagram, the holes play no part. There is no
separate `solve` pass, `InterpResult` carries
only `{ context, errors }`, and an `InterpretedFile` keeps no hole state of its
own. See [[hole]] for the full model.

## Non-obvious invariants & gotchas

- **Holes are not errors, and not root-only.** An unfilled hole is a normal state
  of a map in *any* module; it surfaces through `Complex::map_holes`, listed by
  the interactive `holes` command, not as an interpreter diagnostic.
- **`insert_global_cell` is half a registration.** It mutates the complex and
  returns `(gid, dim)`, but does **not** touch the store's `cells` table. Forget
  the follow-up `set_cell` and the generator exists in the complex yet is invisible
  to `find_cell`/`cell_data_for_tag`.
- **`cell_data_for_tag` searches cells *then* types** for a `Tag::Global`, the
  module's local cells for a `Tag::Local`, and returns `None` for a `Tag::Hole`.
  Types are reachable by tag too, because a type is also a generator.
- **`#[must_use]` on `InterpResult`** (and on `LoadResult`) — it carries errors
  that must be propagated; dropping one silently loses diagnostics.
- **Module order matters.** `modules` is an `IndexMap` in load order
  (dependency-before-dependent); [[output]]'s `normalize` renders in that order
  and `find_type_gid` returns the *first* match across it. A `HashMap` here would
  make both nondeterministic.
- **Map domains compare by pointer.** Map–map `assert` and `map => map` clauses
  require `Arc::ptr_eq` on the domain `Arc<Complex>`s — structural equality is
  never tried. The canonical-`Arc` swap in `interpret_anon_map_component` exists
  solely to keep this honest.
- **`assert_invariants` is debug-only.** It `debug_assert!`s `cells`/`cells_by_dim`
  consistency (every id in `cells_by_dim` is also in `cells`); module/type
  *presence* is checked separately by inline `debug_assert!`s in `modify_module` /
  `modify_type_complex`. Release builds trust the caller.
- **Type bindings forbid local labels.** `insert_type_binding` rejects diagrams or
  maps with `Tag::Local`s; a committed type complex must be self-contained.

## Mathematics

This layer is the bridge from concrete syntax to the algebra the rest of the
codebase computes with — most of its content is plumbing, but a few genuine
mathematical relationships run through it.

- **[[module-system]].** `@Type`/`@Local` blocks, `include`, and
  `attach … along` are the surface of alifib's module-and-type system. `include`
  realises the inclusion of one type's [[core-complex|Complex]] into another via an
  identity [[partial-map|map]]; `attach` realises a pushout-style extension along a
  given partial map (`extend_scope_with_attached_generators`). The `MapDomain::{Type,Module}`
  distinction (`src/core/complex.rs`, consumed by `resolve.rs`) is exactly the
  type-vs-module split of the system. See the open question
  [[module-open-semantics]] for the unbuilt `open` form.
- **[[diagram]].** Every generator's classifier and every `let` binding is a
  `Diagram`; a [[hole|hole's]] boundaries are deferred diagrams, kept as paste
  trees over metavariables until filled. Boundary reasoning is in terms of
  $\partial^\pm_k$ (`Diagram::boundary`/`boundary_normal`). See [[core-diagram]].
- **[[partial-map]].** `attach` and the recording of partial-map holes apply a
  [[partial-map|`PartialMap`]] to diagram boundaries (`PartialMap::apply`). The map
  machinery itself — refinement, totality, holes, application — is documented in
  [[core-partial-map]]; this page only *invokes* it.
- **[[rewriting]].** `(run auto on …)` turns the greedy parallel rewriting of
  [[core-matching]] into a first-class proof diagram by pasting the steps at the
  top dimension.

Pretty-printing is a support relationship, not a realisation: `GlobalStore`
implements `Display` by deferring to `normalize` in [[output]]
(`src/output/normalize.rs`), which flattens the id-keyed store into a name-keyed
render tree.
