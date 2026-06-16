---
kind: impl
status: stable
last-touched: 2026-06-13
code: [src/core/partial_map.rs, src/interpreter/partial_map.rs, src/core/map_hole.rs]
---

# core-partial-map — structure-preserving maps between complexes

> A `PartialMap` is a partial function on *generators* that lifts, by pasting, to
> a total function on the [[diagram|diagrams]] they build. The core module is the
> bare data structure and its three operations — extend, apply, and a *hole-less*
> compose — so it stands alone as a map algebra. The interpreter module is the
> language above it: it turns `let total`, `attach … along`, anonymous maps, and
> `for`-blocks of clauses into a `PartialMap`, fabricates the missing entries that
> `attach` needs, and owns the *hole-aware* composition (`compose_with_holes`)
> behind the dotted `F.G` form.

Two files, two altitudes. `src/core/partial_map.rs` knows nothing about syntax,
scopes, or types — it is a `HashMap<Tag, (CellData, Diagram)>` with a cellular-map
invariant. `src/interpreter/partial_map.rs` is the evaluator: it reads AST
clauses `lhs => rhs`, resolves names against a scope, and grows a core
`PartialMap` clause by clause, *smartly* inferring boundary entries the user
omitted. Keep the two apart: the core never infers, the interpreter never pastes.

## Key public types

| Symbol | File | Role |
|---|---|---|
| `PartialMap` | core | `table: Tag→(CellData, Diagram)`, `by_dim` index, `cellular` flag |
| `PartialMap::{empty, of_entries}` | core | constructors |
| `PartialMap::{extend, apply, compose}` | core | the structural operations; `compose` is the *hole-less* committed composite |
| `compose_with_holes` | interpreter | *hole-aware* composition for the dotted `F.G` form |
| `PartialMap::{insert_raw, is_defined_at, image, cell_data}` | core | low-level access |
| `Entry` *(internal)* | core | one row: a generator's `CellData` plus its image `Diagram` |
| `EvalMap { map, domain, holes }` | `interpreter/types.rs` | an evaluated map, its domain `Arc<Complex>`, and any unfilled [[hole|holes]] |
| `MapHole` | `core/map_hole.rs` | one pending assignment: pure hole or conditional, with boundary paste trees |
| `interpret_partial_map`, `interpret_pmap_def` | interpreter | entry points: AST → `EvalMap` |
| `interpret_def_pmap`, `interpret_def_pmap_module` | interpreter | `let [total] F :: D = …` bindings (type / module domain) |
| `MapBuild` *(internal)* | interpreter | the incremental build state: committed `PartialMap` + pending `holes` |
| `assign_cell`, `ensure_defined`, `commit`/`commit_one`, `cascade` *(internal)* | interpreter | the smart `lhs => rhs` extender and its hole machinery |
| `check_map_totality` *(internal)* | interpreter | the `total` keyword; counts a holed generator as covered |

## What the core owns

A `PartialMap` is defined on a finite set of generating cells. Each `Entry`
records the source cell's `CellData` (its $\partial^-$/$\partial^+$ boundaries,
or `Zero` for a 0-cell) and the image [[diagram]] it is sent to. The whole point
is the **cellular map law**: if a cell is in the domain, every cell in its
boundary is too, and the image's boundaries are the map applied to the source's
boundaries. `extend` is the only constructor that *checks* this law;
`insert_raw` and `of_entries` trust the caller.

The `cellular` boolean is a performance flag, not a correctness one: it is true
exactly when *every* image is a single generating cell (`image.is_cell() &&
image.dim() == dim`). When set, `apply` can skip pasting entirely.

### `extend` — the gatekeeper

`PartialMap::extend(f, tag, dim, cell_data, image)` consumes `f` and returns a
grown map or an `Error`. It rejects a redefinition, rejects an image whose
dimension exceeds the source, and — the heart of it — for a `Boundary` cell at
dimension $\dim$ it runs `check_boundary_match` twice, at $k = \dim - 1$:

$$
f(\partial^-_{k}\,a) \;\stackrel{?}{=}\; \partial^-_{k}\,f(a)
\qquad
f(\partial^+_{k}\,a) \;\stackrel{?}{=}\; \partial^+_{k}\,f(a)
$$

`check_boundary_match` applies `f` to the given boundary diagram, normalises
both sides (`Diagram::normal` vs `Diagram::boundary_normal`), and compares with
`Diagram::equal`. A mismatch is the user-facing *"input boundaries do not match"*
or *"output boundaries do not match"* (one per sign). Dimension/CellData
mismatches (a 0-cell carrying boundary data — *"0-cell cannot have boundary
data"* — or a higher cell with `CellData::Zero` — *"higher-dimensional cell has
no boundary data"*) are caught directly in `extend`'s `match (dim, &cell_data)`,
not in `check_boundary_match`.

### `apply` — lift to a total function on diagrams

`PartialMap::apply(f, diagram)` is where partial-on-generators becomes
total-on-diagrams. A [[diagram]] is a paste tree of generator leaves;
`find_undefined` first walks the tree (`diagram.tree(Sign::Input, top_dim)`) and
errors with the *first* leaf outside the domain — *"diagram value outside of
domain of definition"*. Then two paths:

- **cellular fast path** (`f.cellular`): no pasting. `remap_tag` rewrites each
  leaf label to its image's `top_label`, memoised in a `cache`; `map_tree`
  rewrites the paste history trees through that same cache; `Diagram::make`
  reassembles with the *original* `shape`. This is the in-place relabelling the
  module doc-comment advertises.
- **general path** (`apply_tree`): recurse the tree, look up leaf images
  directly, and recompose interior nodes with `Diagram::paste`. This genuinely
  rebuilds the image diagram from its pieces.

### Two composes — `compose` (core, hole-less) and `compose_with_holes` (interpreter)

The core `PartialMap::compose(g, f)` is the plain partial-function composite over
**committed** entries: it keeps `a ↦ g(f(a))` exactly when `f(a)` lies wholly in
`g`'s domain (`apply(g, ·)` succeeds), dropping the rest, and recomputes
`cellular`. It is pure core — no store, no holes — so `core` is a self-contained
map algebra for callers using these structures independently of the interpreter
(`compose_chains_committed_images`, `compose_drops_entry_whose_image_escapes_domain`).
It may have no in-crate caller; that is deliberate, not rot.

alifib's own dotted `F.G`, though, needs holes to **propagate** — and that is a
re-elaboration, not a HashMap composite. So the hole-aware version lives in
`src/interpreter/partial_map.rs` as `compose_with_holes(context, f, g)`
(computing $f \circ g$, domain $= g$'s), backing the dot-access `base.rest`
form. It rebuilds the composite through the same `MapBuild`/`assign_cell` path,
walking $g$'s domain in ascending dimension (`compose_generator`): for each
generator it resolves $g$'s image, then maps that image's leaves under $f$
(`image_under`, returning `Reach::{Image, Hole, Undefined}`). A fully-resolved
image is assigned (committing or recording a conditional); a pure hole on any
leaf stays a pure hole; an undefined leaf drops the generator. So holes and
conditionals **propagate** rather than being silently forgotten —
`composition_propagates_inner_map_holes`,
`composition_propagates_outer_map_holes`,
`composition_propagates_outer_map_conditionals` (`tests/interpreter.rs`).

## What the interpreter adds

The interpreter turns syntax into a core `PartialMap`. An `EvalMap` bundles the
map with its domain `Complex` (as an `Arc`, so pointer equality can detect "same
domain" — used in `interpret_assign`'s map–map case).

### From clause to entry — `MapBuild` and `assign_cell`

A map extension — grammar `prefix? [ clause, … ]`: square brackets,
comma-separated clauses, the optional prefix map before the bracket
(`<PMapExt>` in `docs/GRAMMAR.md`) — is interpreted into a `MapBuild`: the
committed core `PartialMap` paired with a `holes: Vec<MapHole>` of pending
entries (see [[hole]]). Each clause `lhs => rhs` runs through `interpret_partial_map_clause`:
a bare-`?` right-hand side is the pure-hole assignment (`assign_cell(.., None)`
for a cell, `hole_map_image` for a whole map); otherwise both sides are evaluated
and `interpret_assign` dispatches on `Diag`/`Map` (a whole map can be assigned
pointwise via `extend_matching_map_images`; `f => point` collapses a map to a
constant 0-cell via `extend_map_to_constant`). The diagram case lands in
`assign_cell`, the *smart* extender and the real reason the interpreter exists:

1. The lhs must be a single cell; take its `top_label` tag and `top_dim` $d$.
2. If already committed, accept iff the existing image is `Diagram::isomorphic` to
   the new one, else *"same generator is mapped to multiple diagrams"*; a second
   pure hole on the same cell is idempotent.
3. **Case-1 (sound) inference.** If the image is known and a whole boundary is a
   *single* cell, that face's image is forced — read off the known image's
   boundary — and assigned recursively. No choice, so it commits.
4. **Force every face.** Each still-undefined `boundary_dependencies` entry is
   `ensure_defined` — assigned a hole, which `assign_cell` may itself resolve by
   collapse inference. A cell's faces are thus always committed or pending, never
   silently absent.
5. **Commit, collapse, or hole.** With a known image and every face committed,
   `commit`. With *no* image (a hole or forced face), `collapsed_boundary_image`
   may find a boundary already mapped below dimension $d-1$ — then the only
   possible image is that collapsed diagram, so infer it. Otherwise
   `transport_cell_boundaries` (rewriting each boundary leaf to its committed
   image's paste tree, or a `Tag::Hole` metavariable) and `upsert_entry` a
   pending entry — conditional if the image is known, pure hole if not. (A
   known-image cell with unmapped faces is *not* collapse-checked; any
   incompatibility surfaces when the conditional commits.)

`commit_one` hands the determined entry to the core `PartialMap::extend` (which
re-checks the law), closes the matching pending entry, and **substitutes** its
image's paste tree for that source's `Tag::Hole` leaf in every remaining hole's boundary trees;
`cascade` then commits any conditional whose `MapHole::deps` have closed,
repeating until none remain. A cascade-commit failure is re-blamed on the pending
assignment, not the innocent face that closed last (`blame_pending`, using the
clause spans `MapBuild` stamps on each pending entry).

So the user writes only the top cells; the interpreter fills in the boundary
entries the cellular-map law demands, leaving holes where information is genuinely
incomplete. The core checking still runs — the interpreter infers, it does not
bypass.

**Behavioural evidence** (integration fixtures in `tests/interpreter.rs`):
`collapsed_boundary_infers_image` and `collapse_inference_cascades_through_implicit_faces`
(collapse inference, with the cascade through forced faces);
`dimension_lowering_case1_is_sound` (case-1 inference parametric in the *source*
dimension — a 2-cell to a 1-cell sends `x.in` to `boundary Input 1 e`, not `e.in`);
`inferred_assignment_fills_hole`, `fill_is_order_independent`, and
`prefix_extension_fills_holes` (filling and cascade regardless of clause order);
`redundant_hole_then_value_commits` / `hole_on_defined_generator_is_noop` (the
`?`-then-value idempotence); `wrong_filler_blames_pending_assignment` and
`inconsistent_fill_is_error` (the `blame_pending` re-aiming). The interactive
counterparts live in `tests/fill.rs` (`fill_one_dim_hole_via_rewrite`,
`fill_identity_hole_at_step_zero`, `fill_zero_cell_then_dependent_becomes_available`,
`constraint_from_pending_assignment`).

### `attach … along` is realised here

`attach Name :: Type along [ … ]` (`AttachStmt` in `language/ast.rs`) is *not*
handled in this module but in `interpreter/include.rs`
(`interpret_attach_instr`), which leans on it:

- `resolve_attach` evaluates the optional `along [ … ]` block via
  `interpret_pmap_def`, producing the partial map `f` from the attachment type's
  complex (the domain) into the current scope. No `along` ⇒ the empty map.
  Holes are rejected here — *"Holes (`?`) are not supported in `attach`
  clauses"* — because the fabrication below needs every named image concrete.
- `extend_scope_with_attached_generators` then walks the attachment's generators
  in dimension order; for each one **not** already in `f`'s domain it
  *fabricates* an image: `mapped_cell_data` runs `PartialMap::apply(f, ·)` on the
  source boundaries to get the image `CellData`, mints a fresh generator
  (`Tag::Global` or a local cell depending on `Mode`), and records it with
  `PartialMap::insert_raw` (no re-check — the boundaries were just produced *by*
  the map, so they are compatible by construction).

The net effect: `attach` glues a fresh copy of `Type` into the scope, identifying
exactly the cells the `along` map names and creating new cells for the rest. An
`include` (no `along`) registers an `identity_map` instead. This is the
language-level pushout-flavoured operation, distinct from the ogposet pushout in
[[core-matching]].

### Holes — pending assignments, not a solver

A clause RHS may be a hole `?` (or a whole map may be holed pointwise,
`<map> => ?`); `?` is *only* a clause RHS, never a diagram component — see
`<PMapClause>` vs `<DComponent>` in `docs/GRAMMAR.md`. A hole is a pending
entry of the `MapBuild` — there is no separate
constraint-collection-and-solve phase — carried out of interpretation on
`EvalMap::holes` and stored on the map (`Complex::map_holes`).
A hole's boundaries are kept as paste trees with `Tag::Hole` metavariable leaves,
never realised, so a hole can later be filled by a non-round diagram; its
outstanding dependencies are exactly those metavariable leaves (`MapHole::deps`).
The local inferences above (case-1, collapse, forced faces) and the `cascade`
that commits ready conditionals are all the resolution that happens at
interpretation time; whatever survives is an *open hole*, a normal state of the
map, filled later by the interactive `fill` workflow. See [[hole]] for the model
and [[interactive-session]] for filling.

## Data flow

```
AST (Spanned<ast::PartialMap> | PartialMapDef)
        │  interpret_partial_map / interpret_pmap_def
        ▼
eval_partial_map ──Basic──▶ name lookup (scope.find_map) | anon map | paren
        │  └─Dot──▶ compose_with_holes(base, rest)   (interpreter; propagates holes)
        ▼  Ext block:
initial_eval_map (prefix? → empty | reinterpreted map)
        │
        ▼  eval_pmap_clauses: for each clause / for-block (threading a MapBuild)
interpret_partial_map_clause ──▶ interpret_assign ──▶ assign_cell
        │                            (Diag/Map dispatch)    │ case-1 + ensure_defined faces
        │                                                   ▼
        │                              commit ─▶ PartialMap::extend  ← law check
        │                                  └─▶ cascade ready conditionals
        │                              else upsert_entry ─▶ MapHole (pending)
        ▼
EvalMap { map: PartialMap, domain: Arc<Complex>, holes: Vec<MapHole> }
```

## Non-obvious invariants and gotchas

- **`cellular` is a fast-path flag, never a contract.** It is recomputed on every
  `extend`/`insert_raw`/`compose`. A map can be non-cellular and perfectly valid;
  it just forces `apply` down the pasting path. Don't read `cellular == false` as
  "ill-formed".
- **`insert_raw` skips the law check on purpose.** Its single call site,
  `extend_scope_with_attached_generators`, feeds it images *derived from the
  map* (`mapped_cell_data` applied the map to the source boundaries), so the
  boundary match holds by construction. Likewise `of_entries` trusts the caller;
  its one user is `identity_map`, whose images are the classifiers themselves.
  The interpreter's incremental build, by contrast, commits only through
  `PartialMap::extend` (`commit_one`) — there is no raw path there. New callers
  of either must uphold the law or the cellular-map invariant rots silently.
- **`remap_tag` panics off-domain.** The cellular path's `expect("tag in domain
  (verified by find_undefined)")` is load-bearing: `apply` *must* run
  `find_undefined` first. The panic message names the contract.
- **Two composes — pick by whether holes matter.** Core `compose` is the
  hole-less committed composite (pure, store-free, standalone). The interpreter's
  `compose_with_holes` backs the dotted `F.G`: same drop-where-the-image-escapes
  rule, but it carries pending holes and conditionals through, so a composite of
  maps-with-holes is itself a map-with-holes. Reach for the core one only when you
  genuinely have committed maps and no interpreter context.
- **`assign_cell` infers boundaries; `PartialMap::extend` still checks them.**
  The smartness is purely additive — it computes missing entries (or records them
  as [[hole|holes]]), then submits each committed entry to the same gate a
  hand-written map would face.
- **A committed image never contains a metavariable.** `commit_one` `debug_assert!`s
  this: holes live only in *pending* boundary trees, and a hole is filled by
  substituting its filler before commit. If a `Tag::Hole` reaches `PartialMap`,
  something committed too early.
- **`attach` without `along` ≠ `include`.** Both add generators, but `attach`
  starts from the *empty* map and fabricates every image fresh; `include`
  registers an `identity_map`. Conflating them confuses which generators are
  shared.
- **`extend` guards dimension-*raising* only.** `image.dim() > dim` is rejected (a
  $k$-generator may not map above dimension $k$); there is no lower-bound guard,
  because dimension-*lowering* is legitimate — a 1-cell whose endpoints both map to
  a 0-cell `p` collapses to `p` itself, and collapse inference
  (`collapsed_boundary_image`) produces such images on purpose (see
  [[0001-no-identities]]; tests `collapsed_boundary_infers_image`,
  `dimension_lowering_case1_is_sound`). The structural constraint on a cell is
  roundness of its boundaries, [[0002-round-boundaries]] — a *shape* property
  checked at cell construction, not at pasting and not by `extend`.
- **Holes survive both name lookup and composition.** `eval_partial_map_basic`'s
  `Name` arm carries the stored `Complex::map_holes` onto `EvalMap::holes` —
  that is why `F [ … ]` *fills* a map-with-holes rather than extending only its
  hole-free part (`prefix_extension_fills_holes`). The `Dot` arm goes through
  `compose_with_holes`, which propagates the inner map's holes and conditionals
  into the composite (`composition_propagates_inner_map_holes` and siblings) — a
  composite no longer forgets pending entries.
- **`total` counts a holed generator as covered.** `check_map_totality` treats a
  generator as covered if it is committed *or* has a pending entry, so
  `let total F :: D = [ x => ? ]` is well-formed (`total_map_accepts_holes`). The
  keyword catches *missed* generators, not deliberate placeholders.

## Mathematics

This module realises the [[partial-map]] concept: a structure-preserving partial
function on the generators of a source [[directed-complex]] (the domain is a type,
not necessarily regular; the [[diagram|diagrams]]/shapes it maps are the
[[regular-directed-complex|regular]] ones), lifted by pasting to a total map on
those diagrams, subject to the cellular-map [[boundary]] law
$f(\partial^\pm_k a) = \partial^\pm_k f(a)$. `apply`
is the lift; `extend` is the law; `compose` is the (partial) composition that
makes complexes-and-maps a category — at the committed, hole-less level in core,
and hole-aware (for the dotted `F.G`) as the interpreter's `compose_with_holes`.
The interpreter's `attach … along` is the
language surface: a refinement of one complex onto another, gluing along the named
map. For the matching/pushout maps used in [[rewriting]] — a different, ogposet-level
notion of map — see [[core-matching]]; for the cells being mapped, [[core-diagram]];
for the scope and `find_map`/`MapDomain` plumbing, [[core-complex]] and
[[interpreter]].
