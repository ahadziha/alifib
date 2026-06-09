---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Molecule

A **molecule** is a well-formed pasting of [[atom|atoms]]: everything you can
build from single cells by gluing along matching $k$-[[boundary|boundaries]]
with $\#_k$. Molecules are the shapes of alifib's values — every [[diagram]] is
a labelled molecule — and the objects [[rewriting]] computes with: a rule's
input molecule is matched inside a larger one and its output substituted.

## Definition

Molecules are the [[oriented-graded-poset|oriented graded posets]] generated
inductively by three constructors (Hadzihasanovic, *Combinatorics of
higher-categorical diagrams*, 2024, §3.3):

- **(Point).** The point is a molecule.
- **(Paste).** If $U, V$ are molecules, $k < \min(\dim U, \dim V)$, and the
  output $k$-boundary of $U$ agrees with the input $k$-boundary of $V$,
  $$ \partial^+_k U \;\cong\; \partial^-_k V, $$
  then the pushout $U \#_k V$ of the two along that shared boundary is a
  molecule. For $k \ge \min(\dim U, \dim V)$ the paste degenerates — one side
  is absorbed (Lemma 3.3.7).
- **(Atom).** If $U, V$ are *round* molecules of equal dimension with
  $\partial U \cong \partial V$, then gluing them along that shared boundary
  and adding one greatest element — input face $U$, output face $V$ — yields a
  molecule. See [[atom]].

The boundary agreement in (Paste) is the whole of well-formedness: $\#_k$ is a
*partial* operation, defined exactly when the boundaries agree as oriented
shapes (and, in the labelled setting, as labellings). It does **not** require
roundness — roundness gates (Atom) only. Pasting builds a *larger* shape; it is
**not composition**, which would reduce a diagram to a single cell — a
higher-algebraic operation plain alifib types do not have (see [[diagram]]).
The surface juxtaposition `U V` is *principal pasting*, $U \#_k V$ at
$k = \min(\dim U, \dim V) - 1$.

Derived notions:

- An **atom** is a molecule with a greatest element (3.3.9) — equivalently, one
  whose final constructor was (Point) or (Atom), not a nontrivial (Paste)
  (Lemma 3.3.10). A single top-*dimensional* cell is not enough: a whiskered
  $2$-cell has one $2$-cell but two maximal cells, and is no atom.
- A molecule is **round** when $\partial^- U$ and $\partial^+ U$ meet exactly
  in their common boundary, so $\partial U$ is a directed sphere. A property of
  the shape alone; precondition for (Atom), never for (Paste). Every atom is
  round.
- $\dim U$ is the largest dimension of any cell. Every molecule is a
  [[regular-directed-complex]] (Lemma 3.3.12); the converse fails — two
  disjoint points form an RDC but no molecule.

## Implementation

A molecule is realised at runtime by **`Diagram`** (`src/core/diagram.rs`,
[[core-diagram]]): an [[oriented-graded-poset]] shape, a label per cell, and a
paste history re-deriving it from its atoms ([[core-paste-tree]]). The three
constructors map one-to-one:

- **(Point)/(Atom)** — `Diagram::cell(tag, &CellData)`: `CellData::Zero` for
  the point, `CellData::Boundary { boundary_in, boundary_out }` for the
  globular data of an $n$-cell. Roundness and parallelism are enforced here by
  `Diagram::parallelism` *(internal)* — see [[atom]].
- **(Paste)** — `Diagram::paste(k, u, v)`. The agreement
  $\partial^+_k U = \partial^-_k V$ is checked by `Diagram::pastability`
  *(internal)* — shape and labels, nothing else; no roundness. Its clamping of
  `k` to `top_dim` mirrors Lemma 3.3.7's degenerate cases.
- **Boundaries** $\partial^\pm_k$ — `Diagram::boundary` /
  `Diagram::boundary_normal` ([[boundary]]).
- **Roundness** — `Diagram::is_round` → `Ogposet::is_round`, shape only.
  **Atomicity** — `Diagram::is_cell`: the top-dimensional input paste history
  is a single `PasteTree::Leaf`, the operational reading of Lemma 3.3.10.

Principal pasting `U V` is elaborated by `interpret_sequence_as_term`
(`src/interpreter/diagram.rs`) at $k = \min(\dim U, \dim V) - 1$; an explicit
`#n` goes through the same `Diagram::paste`. A molecule whose labels identify
cells can realise a non-regular *type* — that distinction belongs to
[[directed-complex]].

## Related

[[atom]] · [[diagram]] · [[boundary]] · [[regular-directed-complex]] ·
[[directed-complex]] · [[oriented-graded-poset]] · [[rewriting]] ·
[[core-diagram]] · [[core-paste-tree]]
