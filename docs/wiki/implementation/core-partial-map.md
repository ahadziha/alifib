---
kind: impl
status: stable
last-touched: 2026-06-03
code: [src/core/partial_map.rs, src/interpreter/partial_map.rs, src/core/map_hole.rs]
---

# core-partial-map — structure-preserving maps between complexes

> A `PartialMap` is a partial function on *generators* that lifts, by pasting, to
> a total function on the [[diagram|diagrams]] they build. The core module is the
> bare data structure and its three operations — extend, apply, compose. The
> interpreter module is the language above it: it turns `let total`, `attach …
> along`, anonymous maps, and `for`-blocks of clauses into a `PartialMap`, and it
> fabricates the missing entries that `attach` needs.

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
| `PartialMap::{extend, apply, compose}` | core | the three structural operations |
| `PartialMap::{insert_raw, is_defined_at, image, cell_data}` | core | low-level access |
| `Entry` *(internal)* | core | one row: a generator's `CellData` plus its image `Diagram` |
| `EvalMap { map, domain, holes }` | `interpreter/types.rs` | an evaluated map, its domain `Arc<Complex>`, and any unfilled [[hole|holes]] |
| `MapHole` | `core/map_hole.rs` | one pending assignment: pure hole or conditional, with boundary paste trees |
| `interpret_partial_map`, `interpret_pmap_def` | interpreter | entry points: AST → `EvalMap` |
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

### `compose` — `g ∘ f`

`PartialMap::compose(g, f)` is *lossy by design*: it keeps an entry of `f` only
when `apply(g, f.image)` succeeds — i.e. when `f`'s image lies wholly in `g`'s
domain. Entries outside that intersection are silently dropped (so the result is
still partial). It recomputes `cellular` from the composed images. This is what
backs the dot-access `base.rest` form in the interpreter.

## What the interpreter adds

The interpreter turns syntax into a core `PartialMap`. An `EvalMap` bundles the
map with its domain `Complex` (as an `Arc`, so pointer equality can detect "same
domain" — used in `interpret_assign`'s map–map case).

### From clause to entry — `MapBuild` and `assign_cell`

A map extension is interpreted into a `MapBuild` — the committed core
`PartialMap` paired with a `holes: Vec<MapHole>` of pending entries (see
[[hole]]). Each clause `lhs => rhs` runs through `interpret_partial_map_clause`:
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
5. **Commit, collapse, or hole.** If the cell is now fully determined with a known
   image, `commit`. Else if `collapsed_boundary_image` finds a boundary that has
   dropped below dimension $d-1$, the only possible image is that collapsed
   diagram, so infer it. Otherwise `transport_cell_boundaries` (rewriting each
   boundary leaf to its committed image's paste tree, or a `Tag::Hole`
   metavariable) and `upsert_entry` a pending hole.

`commit_one` hands the determined entry to the core `PartialMap::extend` (which
re-checks the law), closes the matching pending entry, and **substitutes** its
image's paste tree for that metavariable in every remaining hole's boundary trees;
`cascade` then commits any conditional whose `MapHole::deps` have closed,
repeating until none remain. A cascade-commit failure is re-blamed on the pending
assignment, not the innocent face that closed last (`blame_pending`, the fix in
`a151779`).

So the user writes only the top cells; the interpreter fills in the boundary
entries the cellular-map law demands, leaving holes where information is genuinely
incomplete. The core checking still runs — the interpreter infers, it does not
bypass.

### `attach … along` is realised here

`attach Name :: Type along [ … ]` (`AttachStmt` in `language/ast.rs`) is *not*
handled in this module but in `interpreter/include.rs`
(`interpret_attach_instr`), which leans on it:

- `resolve_attach` evaluates the optional `along [ … ]` block via
  `interpret_pmap_def`, producing the partial map `f` from the attachment type's
  complex (the domain) into the current scope. No `along` ⇒ the empty map.
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
`<map> => ?`). There is **no longer a separate constraint-collection-and-solve
phase**: a hole is just a pending entry of the `MapBuild`, carried out of
interpretation on `EvalMap::holes` and stored on the map (`Complex::map_holes`).
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
        │  └─Dot──▶ PartialMap::compose(base, rest)
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
- **`insert_raw` skips the law check on purpose.** Both call sites
  (`extend_scope_with_attached_generators`, and the interpreter's incremental
  construction) feed it images they *derived from the map*, so the boundary
  match is already guaranteed. New callers must uphold that or the cellular-map
  invariant rots silently.
- **`remap_tag` panics off-domain.** The cellular path's `expect("tag in domain
  (verified by find_undefined)")` is load-bearing: `apply` *must* run
  `find_undefined` first. The panic message names the contract.
- **`compose` drops, never errors.** `g ∘ f` is defined only where `f`'s image
  lands in `g`'s domain; the rest vanishes silently. Surprising if you expect a
  total composite — by design for the dot-access form.
- **`assign_cell` infers boundaries; `PartialMap::extend` still checks them.**
  The smartness is purely additive — it computes missing entries (or records them
  as [[hole|holes]]), then submits each committed entry to the same gate a
  hand-written map would face. There is no "trust me" path into the core except
  `insert_raw`.
- **A committed image never contains a metavariable.** `commit_one` `debug_assert!`s
  this: holes live only in *pending* boundary trees, and a hole is filled by
  substituting its filler before commit. If a `Tag::Hole` reaches `PartialMap`,
  something committed too early.
- **`attach` without `along` ≠ `include`.** Both add generators, but `attach`
  starts from the *empty* map and fabricates every image fresh; `include`
  registers an `identity_map`. Conflating them confuses which generators are
  shared.
- **`extend` guards dimension-*raising* only — by design.** `image.dim() > dim`
  is rejected (a $k$-generator may not map above dimension $k$); there is no
  no-*lowering* guard, and that is **correct**, not a gap. Dimension-lowering maps
  are legitimate — a 1-cell whose endpoints both map to a 0-cell `p` collapses to
  `p` itself — and collapse inference (`collapsed_boundary_image`) produces them
  on purpose. An earlier draft wrongly called the absence of a lowering guard a
  correctness bug; see the corrected [[0001-no-identities]]. The genuine
  structural constraint on a cell is roundness of its boundaries,
  [[0002-round-boundaries]].

## Mathematics

This module realises the [[partial-map]] concept: a structure-preserving partial
function on the generators of a source [[regular-directed-complex]], lifted by
pasting to a total map on the [[diagram|diagrams]] it generates, subject to the
cellular-map [[boundary]] law $f(\partial^\pm_k a) = \partial^\pm_k f(a)$. `apply`
is the lift; `extend` is the law; `compose` is the (partial) composition that
makes complexes-and-maps a category. The interpreter's `attach … along` is the
language surface: a refinement of one complex onto another, gluing along the named
map. For the matching/pushout maps used in [[rewriting]] — a different, ogposet-level
notion of map — see [[core-matching]]; for the cells being mapped, [[core-diagram]];
for the scope and `find_map`/`MapDomain` plumbing, [[core-complex]] and
[[interpreter]].
