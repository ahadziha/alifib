---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Atom

An **atom** is a [[molecule]] with a greatest element: one cell whose closure is
the whole shape (Hadzihasanovic, *Combinatorics of higher-categorical diagrams*,
2024, 3.3.9). Atoms are the indecomposable units of directed rewriting — a
molecule is an atom precisely when it was not produced by a nontrivial pasting
(Lemma 3.3.10) — and they are what a **generator** declares: `a : U -> V` names
one new top cell with input diagram $U$ and output diagram $V$.

The greatest element must dominate *every* cell; a single top-*dimensional* cell
is not enough. Whiskering a $2$-cell with a $1$-cell yields a molecule with one
$2$-cell but two maximal cells — not an atom.

## Definition

Fix $n > 0$. An atom $a$ of dimension $n$ is determined by a pair of round
$(n{-}1)$-molecules, its boundaries
$$ \partial^-_{n-1} a = U, \qquad \partial^+_{n-1} a = V, $$
subject to **parallelism**: $U$ and $V$ share a common boundary,
$$ \partial^\alpha_k U = \partial^\alpha_k V \qquad (\alpha \in \{-,+\},\ k < n-1). $$
Roundness is what lets the pair close up into the boundary sphere of a single
cell rather than a generic diagram (see [[0002-round-boundaries]]). The atom is
then $U$ and $V$ amalgamated along that shared boundary, with one new top
element whose input face is (the copy of) $U$ and whose output face is $V$.

The base case $n = 0$ is the **point**, given by no data. There are no identity
atoms — cells never degenerate ([[0001-no-identities]]).

An atom's *shape* is a [[regular-directed-complex|regular directed complex]];
the *type* a generator realises may identify boundary cells (the loop
`a : pt -> pt`) and need not be regular — the distinction is
[[directed-complex]]'s; the labelling story is [[diagram]]'s.

## Implementation

An atom is minted by **`Diagram::cell`** (`src/core/diagram.rs`,
[[core-diagram]]), dispatching on `CellData`:

- `CellData::Zero` — the point, via `Diagram::cell0` *(internal)*.
- `CellData::Boundary { boundary_in, boundary_out }` — two `Arc<Diagram>`
  boundaries, via `Diagram::cell_n` *(internal)* →
  `Diagram::cell_with_input_embedding`.

Parallelism is enforced by `Diagram::parallelism` *(internal)*: equal
dimensions; each argument round *in shape* (`Diagram::is_round` →
`Ogposet::is_round`, labels ignored); equal shared boundary sphere in both
shape and labels. The sphere is extracted by `ogposet::boundary_traverse(Both, …)`,
whose `Both` branch assembles input-extremal cells below the top plus
output-extremal cells at the top (`build_stack_cell_n`, internal), and compared
by `Ogposet::equal` plus a label check. The cell's shape is then the pushout of
the two boundaries glued along that sphere, with one element appended above
(`build_cell_shape`, internal). This is the *only* place roundness is checked;
pasting (`Diagram::pastability`) never requires it — see [[core-diagram]].

Atomicity is observable as `Diagram::is_cell`: the top-dimensional input paste
history is a single `PasteTree::Leaf`, i.e. the diagram was minted by `cell`,
not assembled by $\#_k$ — the operational reading of Lemma 3.3.10 ("final
constructor is (Point) or (Atom)").

Generators are stored in the [[core-complex|Complex]] by
`Complex::add_generator`, which keeps one classifier [[diagram]] per generator
and debug-asserts that the classifier's top label *is* the generator's tag
(`classifier.top_label() == Some(&tag)`). Boundaries are recovered with
`Diagram::boundary` / `Diagram::boundary_normal` ([[boundary]]).

## Related

[[molecule]] · [[diagram]] · [[boundary]] · [[regular-directed-complex]] ·
[[directed-complex]] · [[oriented-graded-poset]] · [[0001-no-identities]] ·
[[0002-round-boundaries]]
