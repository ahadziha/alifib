---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/core/paste_tree.rs]
---

# core-paste-tree — assembly histories and their canonical form

> A `PasteTree` records *one* way to build a [[diagram]] from generators by
> iterated pasting: leaves are generating cells, nodes paste their children at a
> recorded dimension. This module owns the operations on those trees that stand
> apart from a diagram's own bookkeeping — realising a tree back into a diagram,
> rewriting its leaves, and canonicalising it into *pseudo-normal* form.

`src/core/paste_tree.rs` was split out of `diagram.rs` when `resume` was added
([[interactive-engine]]): the engine needs to take a finished proof diagram apart
into the rewrite steps that built it, which is an operation on the paste tree, not
on the labelled shape. A [[diagram|`Diagram`]] carries one `PasteTree` per
dimension as its `paste_history` (a `BoundaryHistory`, see [[core-diagram]]); this
module is everything you do *to* such a tree once you hold it.

## Key public types and functions

All `pub(crate)`.

| Symbol | Role |
|---|---|
| `PasteTree` | `Leaf(Tag)` — a generating cell by tag; `Node { dim, left, right }` (`Arc` children) — pasting `left` and `right` at dimension `dim` |
| `PasteTree::substitute(f)` | replace every leaf whose tag satisfies `f` with the tree `f` returns; other leaves unchanged |
| `realise_tree(tree, complex)` | rebuild the diagram a tree describes: `Leaf` → the generator's `classifier`, `Node` → `Diagram::paste(dim, …)` |
| `flatten_at(tree, k)` | flatten the outermost chain of `$\#_k$` pastes into its maximal subtrees, left to right |
| `top_generators(tree, complex)` | the tags of every top-dimensional leaf, left-to-right with multiplicity |
| `pseudo_normalise(t, complex)` | rewrite into canonical *pseudo-normal* form (see below) |
| `is_pseudo_normal(t)` | predicate: is a unit-free tree already pseudo-normal? |
| `remove_units(t, complex)` | collapse unit pastes (a side of dimension `≤` the pasting dimension) |

Internal helpers: `leaf_dimension` / `tree_dimension` (look a generator's /
tree's dimension up in the [[core-complex|complex]]), `pasting_dimension`,
`output_boundary_tree`, `paste_node`, and `split_node`.

## Realisation — the inverse of assembly

`realise_tree` is the round-trip partner of pasting: where `Diagram::paste`
*records* a `$\#_k$` as a `Node { dim: k, .. }`, `realise_tree` *replays* it.
It recurses to the leaves, looks each leaf's classifier up by tag
(`Complex::find_generator_by_tag` → `Complex::classifier`), and folds back up with
`Diagram::paste`. So a diagram's paste history is a faithful recipe: realising it
reproduces the diagram up to isomorphism. This is the guarantee [[reconstruction]]
verifies and that `Diagram::paste`'s history-tracking exists to support.

## Pseudo-normalisation — picking a canonical tree

Many trees realise the *same* diagram: the **interchange law** lets a
higher-dimensional paste slide above or below a lower-dimensional one. To recover
the rewrite steps of a proof, `resume` needs a canonical representative.

`pseudo_normalise` produces one in two moves:

1. `remove_units` strips *unit* pastes — a `Node` one of whose sides has dimension
   `≤` the pasting dimension contributes no top cell at that level, so it is
   absorbed.
2. The highest-dimensional paste is repeatedly lifted to the root by interchange,
   so the outermost `Node` is always at the top dimension occurring in the tree.

The result is **pseudo-normal**: every node pastes at the highest dimension
beneath it, and the realised diagram is unchanged up to isomorphism.
`is_pseudo_normal` is the predicate that pins this — `pseudo_normalise` does not
self-check, but its one caller `RewriteEngine::resume` `debug_assert!`s it on the
output ([[interactive-engine]]).

Once a proof tree is pseudo-normal, `flatten_at(tree, n)` (with `n` one below the
top) splits its outermost `$\#_n$` chain into the maximal subtrees pasted at that
dimension — and each subtree, realised, is one rewrite step. `top_generators`
labels that step by the `(n+1)`-generators it applies. This is exactly the
decomposition `RewriteEngine::resume` performs (see [[interactive-engine]]).

## Non-obvious invariants & gotchas

- **`realise_tree` is the inverse of *assembly*, not of `reconstruct`.**
  `reconstruct::build_tree` ([[core-matching]]) *synthesises* a tree from a bare
  ogposet + labels; `realise_tree` turns *any* tree back into a diagram. Both meet
  in `reconstruct::reconstruct`, which builds a tree and then realises it to check.
- **A `Node`'s `dim` is the pasting dimension, not a cell dimension.** Leaves carry
  no dimension of their own; a leaf's dimension is looked up from the complex
  (`leaf_dimension`), and a tree's top dimension from its leaves (`tree_dimension`).
- **`flatten_at` only descends through nodes at the requested `k`.** A `Node` at a
  different dimension is returned whole as one part — it is *not* recursively
  flattened. This is what makes each flattened part a single `$\#_k$`-step.
- **Pseudo-normal ≠ normal.** Pseudo-normalisation canonicalises the *paste
  structure* (interchange + units); it does not touch the diagram's cell ordering
  (that is `Diagram::normal` / `Ogposet::normalisation`, see [[core-ogposet]]).
- **Behavioural evidence.** `realise_*` tests (`realise_generator_classifier`,
  `realise_composite_diagram_dim1`/`dim2`, `realise_idem_classifier`,
  `realise_beta_classifier`, …) pin the diagram → tree → diagram round trip;
  `interchange_left_nested`, `interchange_right_nested`,
  `already_pseudo_normal_is_stable`, and `lambda_sigma_examples_roundtrip` pin
  pseudo-normalisation (idempotent on canonical input, realisation-preserving).

## Mathematics

A `PasteTree` is the syntactic record of how a [[molecule]] was composed from
[[atom|atoms]] by pasting `$\#_k$` — the term, in the free-higher-category sense,
that the [[diagram]] is the value of. `realise_tree` is the evaluation map from
terms to diagrams; [[reconstruction]] inverts it, synthesising a term for a given
shape, and verifies the synthesis by realising it. `pseudo_normalise` is the
interchange-law normal form that lets [[rewriting]]'s `resume` read a proof
diagram back as the sequence of rewrite steps that produced it. See
[[core-diagram]] for the `Diagram`/`BoundaryHistory` a tree lives inside,
[[core-matching]] for the synthesis direction (`build_tree`/`reconstruct`), and
[[interactive-engine]] for `resume`, the operation this module was extracted to
serve.
