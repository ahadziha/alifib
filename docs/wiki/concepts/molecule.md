---
kind: concept
status: stable
last-touched: 2026-06-05
---

# Molecule

A **molecule** is a [[regular-directed-complex]] presented as a pasting of
[[atom|atoms]] — the *well-formed shapes* of higher-categorical [[diagram|diagrams]].
Where an [[atom]] $a$ is a single top-dimensional cell with its globular boundary,
a molecule $U$ is everything you can build by gluing atoms together along matching
$k$-[[boundary|boundaries]]. Molecules are the objects alifib computes with:
[[rewriting]] matches a rule's input molecule inside a larger molecule and
substitutes its output.

## Definition

Following Hadzihasanovic (*Combinatorics of higher-categorical diagrams*, 2024),
molecules are the subclass of regular directed complexes generated *inductively* by
two clauses, closed under one operation:

1. **Atoms.** Every [[atom]] is a molecule. An atom of dimension $n$ has a single
   top cell whose input boundary $\partial^-_{n-1}$ and output boundary
   $\partial^+_{n-1}$ are themselves $(n-1)$-molecules — its globular data.
   The point ($n=0$) is the base case: an atom with no boundary.

2. **Pasting.** If $U$ and $V$ are molecules and the output $k$-boundary of $U$
   *agrees* with the input $k$-boundary of $V$,
   $$ \partial^+_k U \;=\; \partial^-_k V, $$
   then their **paste** $U \#_k V$ is a molecule. Geometrically $U$ and $V$ are
   glued along that shared $k$-dimensional face; the colimit (pushout) of the two
   along the common boundary is again a regular directed complex, and the
   inductive closure guarantees it is again a molecule.

The matching condition in clause 2 is the heart of well-formedness: $\#_k$ is a
*partial* operation, defined exactly when the boundaries are equal as oriented
shapes (and, in the labelled setting, as labellings). It does **not** require the
arguments to be round — roundness gates *cell construction*, not pasting (see the
round bullet below, and correction in [[diagram]]). Not every regular directed
complex arises this way — molecules are precisely the **pasting-decomposable**
ones. A molecule is an [[atom]] iff it has a single top-dimensional cell (it is
then *indecomposable* under $\#_k$ at the top dimension).

Pasting builds a *larger* shape; it is **not composition**, which would reduce a
diagram to a single cell — a higher-algebraic operation plain alifib types do not
have. The surface juxtaposition `U V` is *principal pasting*, $U \#_k V$ at
$k = \min(\dim U, \dim V) - 1$.

Two derived notions recur:

- A molecule is **round** when its input and output boundaries are spheres —
  $\partial^-_{n-1}U$ and $\partial^+_{n-1}U$ share a boundary and together close
  up. Roundness is a property of the *shape* alone (it ignores any labelling), and
  it is the precondition for a molecule to be the input/output boundary of a *cell*
  one dimension up — not a precondition for pasting. Every atom is round.
- The **dimension** $\dim U$ is the largest dimension of any cell; the empty
  molecule has dimension $-1$.

## Why alifib needs it

Molecules are the well-typed values of the language. A generator declares an atom;
a `let`-binding names a molecule pasted from generators; a rewrite rule is a pair of
parallel molecules. Because pasting is partial, the type system is really a
*shape-checking* discipline: every $\#_k$ must verify a boundary agreement, and the
recorded paste structure is what lets a molecule be re-derived from its atoms.

## Implementation

A molecule is realised at runtime by the **`Diagram`** type
(`src/core/diagram.rs`) — a labelled molecule. See [[core-diagram]]. The
substrate is an [[oriented-graded-poset]] (`src/core/ogposet.rs`); the labelling
names each cell with a generator, so a `Diagram` is a molecule *over* a
[[core-complex|Complex]] of generators.

The *shape* of a molecule (and so of every `Diagram` value) is a
[[regular-directed-complex|regular directed complex]]. A **type**, assembled from
generators whose boundaries the labelling identifies, is in general only a
[[directed-complex]] — it need not be regular. (Canonical witness: a point with one
arrow `a : pt -> pt`; the arrow's shape is the round 0-sphere with two distinct
endpoints, but the labels send both to `pt`, realising a directed loop that is a
fine directed complex yet not a regular CW complex. See [[diagram]].)

- **Atoms** are minted by `Diagram::cell(tag, &CellData)`, where `CellData::Zero`
  is the point and `CellData::Boundary { boundary_in, boundary_out }` carries an
  [[atom]]'s globular input/output as two $(n-1)$-`Diagram`s.
- **Pasting** $\#_k$ is `Diagram::paste(k, u, v)`. The boundary-agreement
  precondition $\partial^+_k U = \partial^-_k V$ is checked by `Diagram::pastability`
  (internal) before the pushout glues the two shapes; this is *all* `pastability`
  enforces — it does **not** check roundness.
- **Boundaries** $\partial^\pm_k$ are `Diagram::boundary` / `Diagram::boundary_normal`.
- **Roundness** is `Diagram::is_round`, which delegates to `Ogposet::is_round`: it
  inspects the bare shape and ignores labels. Roundness is enforced only by
  `Diagram::parallelism` at cell construction, never by `paste`. Atomicity is
  `Diagram::is_cell`, which tests that the top-dimensional input paste history is a
  single `Leaf` (i.e. the diagram was minted as one cell, not assembled by $\#_k$)
  — see [[core-diagram]].

The full bridge — construction, pasting, boundary clamping, the three-arrays
invariant — lives in [[core-diagram]]; the conceptual gloss of a `Diagram` as a
labelled molecule is [[diagram]].

## Related

[[atom]] · [[diagram]] · [[boundary]] · [[regular-directed-complex]] ·
[[directed-complex]] · [[oriented-graded-poset]] · [[rewriting]]
