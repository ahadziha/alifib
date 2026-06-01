---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Atom

An **atom** is a [[molecule]] with exactly one top-dimensional cell. It is the
indecomposable unit of directed rewriting: every molecule is built by pasting
atoms with $\#_k$, and an atom is precisely a molecule that admits no nontrivial
decomposition. In Hadzihasanovic's combinatorics an atom of dimension $n$ is the
*representable* molecule on a single $n$-cell — the directed analogue of an
$n$-globe, but with arbitrary (not merely globular) input and output boundaries.

In the alifib language an atom is what a **generator** declares: `a : U -> V`
names a single top cell whose input is the diagram $U$ and whose output is the
diagram $V$.

## Definition

Fix a dimension $n > 0$. An atom $a$ is determined by a pair of $(n-1)$-diagrams,
its input and output boundary,
$$ \partial^-_{n-1} a = U, \qquad \partial^+_{n-1} a = V, $$
subject to the **parallelism** condition: $U$ and $V$ must be *round* and share a
common boundary,
$$ \partial^-_{k} U = \partial^-_{k} V \quad\text{and}\quad \partial^+_{k} U = \partial^+_{k} V \qquad (k < n-1). $$
Equivalently, $\partial^\pm_k a$ is well-defined for every $k < n$: the two
faces of the boundary sphere agree below the top, so the atom has a genuine
$k$-boundary at each lower dimension. Roundness is what makes the pair $U, V$
glue into the boundary of a single cell rather than a generic diagram.

The atom $a$ is then the molecule whose underlying [[oriented-graded-poset]] is
$U$ and $V$ amalgamated along their shared boundary, with one new top element
$a$ whose input face is (the copy of) $U$ and whose output face is $V$:
$$ \dim a = n, \qquad \partial^-_{n-1} a = U, \qquad \partial^+_{n-1} a = V. $$

The degenerate case $n = 0$ is the **point**: a $0$-atom has no boundary at all
(there is no dimension below $0$), so it is given by no data. There are no
identity atoms — alifib follows the [[regular-directed-complex]] discipline in
which cells never degenerate (see [[0001-no-identities]]).

Atoms are the generators of the ambient complex; a [[molecule]] is any
diagram reachable from them under pasting, and a [[diagram]] is a molecule with
each cell labelled by a generator.

## Implementation

An atom is realised by **`Diagram::cell`** in `src/core/diagram.rs`
([[core-diagram]]), which dispatches on its argument

- `CellData::Zero` — the point; built by `Diagram::cell0` *(internal)*, a single
  $0$-cell with no boundary.
- `CellData::Boundary { boundary_in, boundary_out }` — the two boundary diagrams
  (`Arc<Diagram>`); built by `Diagram::cell_n` *(internal)* via
  `Diagram::cell_with_input_embedding`.

The parallelism condition above is enforced operationally by
`Diagram::parallelism` *(internal)*: it rejects the pair unless the two diagrams
have equal dimension, are each *round* (`Diagram::is_round`), and have
equal boundary shape and labels under `boundary_traverse(Both, …)`. The new top
cell's shape is the pushout of the two boundaries glued along their shared
sphere, with one element appended above.

Atoms are stored as **generators** in the [[core-complex|Complex]] via
`Complex::add_generator`, which keeps a classifier [[diagram]] per generator and
asserts the invariant that the classifier's top label *is* the generator's tag
(`classifier.top_label() == Some(&tag)`) — i.e. the stored diagram really is the
single-top-cell atom for that name. Boundaries are recovered with
`Diagram::boundary` / `Diagram::boundary_normal` (see [[boundary]]).

## Related

[[molecule]] · [[diagram]] · [[boundary]] · [[regular-directed-complex]] ·
[[oriented-graded-poset]] · [[0001-no-identities]]
