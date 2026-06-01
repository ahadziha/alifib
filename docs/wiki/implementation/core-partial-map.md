---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/core/partial_map.rs, src/interpreter/partial_map.rs]
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
| `EvalMap { map, domain }` | `interpreter/types.rs` | an evaluated map paired with its domain `Arc<Complex>` |
| `interpret_partial_map`, `interpret_pmap_def` | interpreter | entry points: AST → `EvalMap` |
| `extend_map_for_cell` | interpreter | the smart `lhs => rhs` extender (recurses on boundaries) |
| `enrich_holes` *(internal)* | interpreter | feeds boundary constraints to the hole solver |

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

### From clause to entry — `extend_map_for_cell`

A clause `lhs => rhs` becomes a call to `interpret_assign`, which dispatches on
whether each side evaluated to a `Diag` or a `Map` (a whole map can be assigned
pointwise; `f => point` collapses a map to a constant 0-cell). The diagram case
calls `extend_map_for_cell`, the *smart* extender and the real reason the
interpreter exists:

1. The lhs must be a single cell; take its `top_label` tag and `top_dim` $d$.
2. If already defined, accept iff the existing image is `Diagram::isomorphic` to
   the new one, else *"same generator is mapped to multiple diagrams"*.
3. Otherwise look at the cell's `boundary_dependencies` — the boundary tags not
   yet in the map — and **recursively extend the map for each** before adding
   the cell itself. The image of a boundary cell is inferred by
   `image_classifier_via_boundary`, which transports the focus tag through the
   shape isomorphism (`Diagram::map_tag_via_shape_iso`) between source and target
   boundaries and reads off the target's `classifier`.
4. Finally hand off to the core `PartialMap::extend`, which re-checks the law.

So the user writes only the top cell; the interpreter fills in the boundary
entries the cellular-map law demands. The core checking still runs — the
interpreter infers, it does not bypass.

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

### Holes and the constraint solver — `enrich_holes`

A clause RHS may be a hole `?`. After all clauses are processed,
`interpret_partial_map_ext` calls `enrich_holes`, which uses the
partially-built map to tell the inference solver about each hole's boundary. For
a **direct** hole (`arr => ?`, flagged by `is_pure_hole_diagram`) the map applied
to the source cell's boundary *is* the hole's boundary, so it emits a strong
`Constraint::BoundaryEq` (and a `DimEq`). For an **embedded** hole (`arr => ? g`)
the map only sees the composite's boundary, so it emits no solver constraint and
instead stashes a `PartialHint` for the renderer (to show `_` for unmapped
labels). A partial `apply` failure on a direct hole likewise downgrades to a
hint. See [[interpreter]] for the solver these constraints feed.

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
        ▼  eval_pmap_clauses: for each clause / for-block
interpret_partial_map_clause ──▶ interpret_assign ──▶ extend_map_for_cell
        │                                                   │ recurse on boundaries
        │                                                   ▼
        │                                          PartialMap::extend  ← law check
        ▼
enrich_holes  ──▶ Constraint::{BoundaryEq, DimEq} | PartialHint
        ▼
EvalMap { map: PartialMap, domain: Arc<Complex> }
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
- **`extend_map_for_cell` infers boundaries; `PartialMap::extend` still checks
  them.** The smartness is purely additive — it computes missing entries, then
  submits the whole thing to the same gate a hand-written map would face. There
  is no "trust me" path into the core except `insert_raw`.
- **`attach` without `along` ≠ `include`.** Both add generators, but `attach`
  starts from the *empty* map and fabricates every image fresh; `include`
  registers an `identity_map`. Conflating them confuses which generators are
  shared.
- **No identities — but the lower-dimension guard is missing.** `extend` rejects
  dimension-*raising* (`image.dim() > dim`) yet has **no** check against
  *lowering*: a 1-cell whose endpoints both map to the same 0-cell `p`, itself
  sent to `p`, passes — `0 > 1` is false, and `check_boundary_match` at $k = 0$
  compares `p` against `p`. So a degenerate 1-cell→0-cell map (an identity in
  disguise) is constructible today, contradicting [[0001-no-identities]]. The
  discipline is intended but only *accidentally* enforced — it fails only when the
  two endpoints map to *distinct* points. Tracked as a source-side gap; see
  [[source-drift]] and the warning in [[0001-no-identities]].

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
