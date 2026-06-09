---
kind: impl
status: stable
last-touched: 2026-06-09
code: [src/core/diagram.rs]
---

# core-diagram — `Diagram`, `CellData`, `Sign`

`src/core/diagram.rs` is where a [[molecule]] becomes a runtime value. A
**`Diagram`** is a labelled [[oriented-graded-poset]] carrying a record of how it
was pasted together. Three arrays travel in lockstep:

- `shape: Arc<Ogposet>` — the combinatorial substrate ([[core-ogposet]]);
- `labels: Vec<Vec<Tag>>` — `labels[dim][pos]` names every cell with a `Tag`
  (a generator's global id, or a local name);
- `paste_history: Vec<BoundaryHistory>` — `paste_history[dim]` remembers, per
  dimension, the input/output **paste trees** so a diagram can later be re-derived
  from its generators ([[core-paste-tree|`realise_tree`]]).

The *shape* of any `Diagram` value is a [[regular-directed-complex|regular
directed complex]] (the shape of a [[molecule]]). The labels may identify
boundary cells, so a **type** built from generators is only a [[directed-complex]],
not necessarily regular — see [[diagram]].

## Key public types

- **`CellData`** — the boundary spec of a single generating cell:
  - `Zero` — a 0-cell (a point), no boundaries;
  - `Boundary { boundary_in, boundary_out }` — an $n$-cell ($n>0$) whose input
    and output are themselves $(n-1)$-`Diagram`s. This is exactly an [[atom]]'s
    globular data. See [[boundary]].
- **`Sign`** — `Input` | `Output`, the polarity selecting a [[boundary]]. The same
  two sides as `ogposet::Sign` minus its `Both`; diagram operations always act on
  exactly one side, and `Sign::as_ogposet_sign` converts.
- **`Diagram`** itself — constructed via `Diagram::cell`, pasted via
  `Diagram::paste`, sliced via `Diagram::boundary` / `Diagram::boundary_normal`,
  canonicalised via `Diagram::normal`.

## Construction: cells

`Diagram::cell(tag, &CellData)` is the only way to mint an [[atom]]; it dispatches
on the `CellData`:

- `CellData::Zero` → `Diagram::cell0` *(internal)*: a one-point
  shape (`Ogposet::point`) labelled `tag`.
- `CellData::Boundary { boundary_in, boundary_out }` → `Diagram::cell_n`
  *(internal)*, a thin wrapper over `Diagram::cell_with_input_embedding`
  *(pub(super))*. That checks the two boundaries are **parallel**
  (`Diagram::parallelism`), takes their `pushout` to glue the shared boundary
  sphere, then `build_cell_shape` bolts one new top cell on at dimension $d+1$ —
  its input faces are the `inl` image, its output faces the `inr` image.
  `build_cell_paste_history` makes the new cell's top `paste_history` a
  `Leaf(tag)` — the invariant `is_cell` checks.

`Diagram::parallelism` *(internal)* is the cell-construction gate: it requires the
input and output to (1) have equal dimension, (2) each be **round** in *shape*
(`Diagram::is_round`, delegating to `Ogposet::is_round` — the directed-sphere test
that the shape is `is_pure` with input and output interiors disjoint at every
dimension; labels are irrelevant), and (3) share an equal boundary sphere in both
shape and labels (computed with `ogposet::boundary_traverse(Both, …)`). This is
the **only** place roundness is enforced — pasting does not check it.

## Pasting: $\#_k$

`Diagram::paste(k, u, v)` glues two diagrams along their shared $k$-[[boundary]].
This is **pasting, not composition**: it builds a *larger* diagram and never
reduces the pair to a single cell — composition is a higher-algebraic op plain
alifib types do not have (see [[diagram]]). The juxtaposition `f g` of the surface
syntax is *principal pasting*, $f \#_k g$ at $k = \min(\dim f, \dim g) - 1$
(`interpreter::diagram::interpret_sequence_as_term`).

1. `Diagram::pastability(k, u, v)` *(internal)* checks $\partial^+_k u$ and
   $\partial^-_k v$ agree in both shape and labels (via
   `ogposet::boundary_traverse`). It checks *only* this boundary agreement — it
   does **not** call `is_round`. Any claim that $\#_k$ requires round arguments is
   wrong; the only roundness gate is `parallelism`, at cell construction.
2. `pushout` glues `u.shape` and `v.shape` along that shared boundary.
3. `merge_pushout_labels` routes each side's labels into the merged cell
   positions; `paste_histories` builds the combined `paste_history` (`paste_tree`
   inherits from $u$ below $k$, takes input from $u$ / output from $v$ at $k$, and
   joins into a `Node { dim: k, .. }` above $k$).

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
`Diagram::boundary_correspondence` *(pub(crate))* maps cell positions of an
independently computed boundary diagram back into the parent at dimension $k$
(`find_isomorphism` against the extracted boundary, composed with the boundary
embedding); the interactive web layer uses it to align boundary selections.

## Top dimension and queries

- `dim()` returns the `isize` shape dimension; **negative means the empty
  diagram**. `top_dim()` clamps that to a `usize` (0 for empty).
- `top_label` / `labels_at` / `all_labels` read the label arrays.
- `is_round` is a property of the **shape** — it delegates straight to
  `Ogposet::is_round` ([[core-ogposet]]); labels are irrelevant. `is_normal`
  reports canonical cell order; `is_cell` is `true` iff the top-dimensional input
  paste tree is a single `PasteTree::Leaf` — i.e. the diagram was minted as one
  cell, not assembled by $\#_k$ (false for the empty diagram).
- `equal` is structural (same shape via `Ogposet::equal`, same labels at every
  position); `isomorphic` is equality up to the canonical shape isomorphism
  (tries `equal` first, then `ogposet::find_isomorphism` + `pullback_labels`).
- `normal` reorders cells canonically via `ogposet::normalisation` (identity if
  already normal); `partial_map` uses it to compare diagrams modulo cell order.

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
- **No rewrite constructor lives here.** Diagram-level rewriting is *not* a method
  of `Diagram`. The production rewrite path is
  `matching::construct_parallel_step` → `pushout::multi_pushout` (see
  [[core-matching]], [[rewriting]]), which uses `RulePattern::pattern_to_rewrite`
  (an `ogposet::boundary_traverse` embedding), not anything from this module.
- **`cell_with_input_embedding`'s second return value is currently unconsumed.**
  It returns the embedding of the input boundary into the new cell (the pushout's
  `inl` extended with a `NO_PREIMAGE` row at dim $d+1$ for the new top cell), but
  its only caller today is `cell_n`, which discards it.

## Mathematics

A `Diagram` is a labelled [[molecule]] — see [[diagram]] for the conceptual
account of pasting ($\#_k$), top dimension, and boundaries
$\partial^-_k$/$\partial^+_k$. The substrate is the [[oriented-graded-poset]] whose
shapes are [[regular-directed-complex|regular directed complexes]]; a *type*
built from labelled generators is only a [[directed-complex]]. The boundary
operators are [[boundary]]; `CellData` is the globular data of an [[atom]];
rewriting that builds new diagrams is [[rewriting]] (via
`matching::construct_parallel_step` → `pushout::multi_pushout`, *not* a `Diagram`
method). A `Diagram` is stored and named inside a [[core-complex|Complex]]; its
`paste_history` is a [[core-paste-tree|`PasteTree`]] per dimension, and
`realise_tree` (there) inverts pasting.
