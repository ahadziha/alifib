---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/core/diagram.rs, src/core/ogposet.rs]
---

# core-diagram — `Diagram`, `CellData`, `Sign`

`src/core/diagram.rs` is where a [[molecule]] becomes a runtime value. A
**`Diagram`** is a labelled [[oriented-graded-poset]] carrying a record of how it
was pasted together. Three things travel in lockstep:

```rust
pub struct Diagram {
    shape: Arc<Ogposet>,              // the combinatorial substrate
    labels: Vec<Vec<Tag>>,            // labels[dim][pos]
    paste_history: Vec<BoundaryHistory>, // paste_history[dim]
}
```

The shape lives in `src/core/ogposet.rs` (see [[oriented-graded-poset]]); `labels`
names every cell with a `Tag` (a generator's global id, or a local name); and
`paste_history` remembers, per dimension, the input/output **paste trees** so a
diagram can later be re-derived from its generators
([[core-paste-tree|`realise_tree`]]).

## Key public types

- **`CellData`** — the boundary spec of a single generating cell:
  - `Zero` — a 0-cell (a point), no boundaries;
  - `Boundary { boundary_in, boundary_out }` — an $n$-cell ($n>0$) whose input
    and output are themselves $(n-1)$-`Diagram`s. This is exactly an [[atom]]'s
    globular data. See [[boundary]].
- **`Sign`** — `Input` | `Output`, the polarity selecting a [[boundary]]. The same
  two sides as `ogposet::Sign` minus its `Both`; the diagram-level `Sign` is always
  exactly one side, and `Sign::as_ogposet_sign` maps each to the matching ogposet
  sign.
- **`Diagram`** itself — constructed via `Diagram::cell`, composed via
  `Diagram::paste`, sliced via `Diagram::boundary` / `Diagram::boundary_normal`.

## Construction: cells

`Diagram::cell(tag, &CellData)` is the only way to mint an [[atom]]:

- `CellData::Zero` → `cell0`: a one-point [[oriented-graded-poset]]
  (`Ogposet::point`) labelled `tag`.
- `CellData::Boundary { .. }` → `cell_n`: checks the input and output boundaries
  are **parallel** (`Diagram::parallelism` — same dimension, both round, equal
  boundary shape and labels), takes their `pushout` to glue the shared boundary,
  then bolts one new top cell on at dimension $d+1$ spanning them — input boundary
  below, output above (`build_cell_shape`). The new cell's
  `paste_history` is a `Leaf(tag)` at the top (`build_cell_paste_history`) — this
  is the invariant that `is_cell` checks.

## Pasting: $\#_k$

`Diagram::paste(k, u, v)` realises composition along the $k$-[[boundary]]:

1. `pastability(k, u, v)` checks $\partial^+_k u$ and $\partial^-_k v$ agree in
   both shape and labels (via `ogposet::boundary_traverse`).
2. `pushout` glues `u.shape` and `v.shape` along that shared boundary.
3. `merge_pushout_labels` routes each side's labels into the merged cell
   positions; `paste_histories` builds the combined `paste_history` (`paste_tree`
   inherits below $k$, joins into a `Node { dim: k, .. }` above $k$).

The result is a new `Diagram` whose history records the $\#_k$ that produced it.

## Boundaries: $\partial^\pm_k$

`Diagram::boundary(sign, k, &d)` returns the $(sign, k)$-boundary as a fresh
diagram. It clamps `k` to the diagram's top dimension (`effective_k`), asks
`ogposet::boundary` for the sub-shape and its embedding, then `pullback_labels`
relabels the sub-shape and `boundary_history` clamps the history to match.

> **Gotcha — history must clamp with the shape.** Both `boundary` and
> `boundary_normal` compute `effective_k = k.min(top_dim)` and pass *that* to
> `boundary_history`. Keeping the caller's larger `k` would manufacture phantom
> history levels and break the core invariant that `shape`, `labels`, and
> `paste_history` all have equal length. The regression test
> `boundary_normal_clamps_history_to_top_dim` pins this.

`Diagram::boundary_normal` is the same but returns the *normalised* boundary
(canonical cell order, via `ogposet::boundary_traverse`).

## Top dimension and queries

- `dim()` returns the `isize` shape dimension; **negative means the empty
  diagram**. `top_dim()` clamps that to a `usize` (0 for empty).
- `top_label` / `labels_at` / `all_labels` read the label arrays.
- `is_round` (the directed-sphere condition — input and output interiors stay
  disjoint at every dimension; delegates to `ogposet::is_round`), `is_normal`
  (canonical cell order), `is_cell` (top paste tree is a single `Leaf`).
  Roundness gates **cell construction** (`parallelism` rejects non-round input or
  output boundaries), not `paste` — `pastability` checks only boundary agreement.
- `equal` is structural (same shape, same labels); `isomorphic` is up to the
  canonical shape isomorphism (`ogposet::find_isomorphism` + `pullback_labels`).

## Non-obvious invariants & gotchas

- **Three arrays, one length.** `Diagram::well_formed` (asserted in debug by
  `Diagram::make`) requires `labels.len() == paste_history.len() == sizes.len()`
  and `labels[d].len() == sizes[d]` for every $d$; for non-empty diagrams the top
  label row must be non-empty (classifier lookup depends on it).
- **`paste_history[d]` is not indexed by cell.** It stores exactly one
  `BoundaryHistory` (an input/output paste-tree pair) per dimension — the history
  of the whole $d$-boundary, not of individual cells.
- **`missing_tree()` sentinel.** Where history is genuinely absent the code falls
  back to `Leaf(Tag::Local("?"))`; `hist_tree_or_top` instead reuses a side's own
  top tree to preserve lower-dimensional cell identity in mixed-dimension pastes.
- **`whisker_rewrite`** is the rewriting-specific constructor: it builds the
  $(n+1)$-cell $S = U \cup_V R$ for a [[rewriting|rewrite step]] by pushing out the
  match embedding against the rule cell's input embedding
  (`cell_with_input_embedding`), then splices the rule's output labels in place of
  the matched segment in the dim-$n$ output tree.

## Mathematics

A `Diagram` is a labelled [[molecule]] — see [[diagram]] for the conceptual
account of pasting ($\#_k$), top dimension, and boundaries
$\partial^-_k$/$\partial^+_k$. The substrate is the [[oriented-graded-poset]]; the
boundary operators are [[boundary]]; `CellData` is the globular data of an
[[atom]]; `whisker_rewrite` realises [[rewriting]]. A `Diagram` is stored and
named inside a [[core-complex|Complex]]; its `paste_history` is a
[[core-paste-tree|`PasteTree`]] per dimension, and `realise_tree` (there) inverts
pasting.
