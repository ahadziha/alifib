---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Regular directed complex

A **regular directed complex** (RDC) is an [[oriented-graded-poset]] in which
the closure $\mathord{\downarrow} x$ of every cell is an [[atom]]
(Hadzihasanovic, *Combinatorics of higher-categorical diagrams*, 2024, 5.3.1).
Every cell is attached without identifications, by an honest embedded directed
cell — the directed analogue of a *regular* CW complex. RDCs are the shapes of
alifib's *values*: every [[molecule]] is one (Lemma 3.3.12). A *type*, whose
labelling may identify cells, is in general only a [[directed-complex]].

The most consequential fact for a programmer is what RDCs *lack*: **no identity
cells**. A degenerate $(k{+}1)$-cell over a $k$-cell would have coincident
boundary hemispheres — not round, so its closure is no atom. alifib inherits
this wholesale; the consequence for [[partial-map|partial maps]] —
dimension-*lowering* collapse is legitimate, only dimension-*raising* is barred
— is spelled out in [[0001-no-identities]].

## Definition

Start from an [[oriented-graded-poset]]: a poset $P$ graded by $\dim$, in which
each covering relation $x \lessdot y$ (so $\dim y = \dim x + 1$) carries a sign
$-$ (input) or $+$ (output), splitting the faces of $y$ into input and output
faces. For a closed subset $U \subseteq P$ and $\alpha \in \{-,+\}$, the
**$k$-boundary** $\partial^\alpha_k U$ is the closure of the $\alpha$-extremal
$k$-cells, together with any maximal cells of dimension below $k$ — see
[[boundary]].

A cell $x$ of dimension $n$ is **regular** when its closure
$\mathord{\downarrow} x$ is an atom: a greatest cell whose boundaries
$\partial^\pm_{n-1} x$ are **round** $(n{-}1)$-molecules. *Roundness* — input
and output interiors disjoint at every dimension, so the boundary is a directed
sphere split into two hemispheres — is the recursive engine of the definition
(see [[0002-round-boundaries]]).

An oriented graded poset is a **regular directed complex** when every cell is
regular. Atoms are the indivisible regular shapes; [[molecule|molecules]] —
everything alifib pastes with $\#_k$ — are RDCs by construction (Lemma 3.3.12),
so the interpreter never needs a global regularity check.

## Implementation

The substrate is `Ogposet` in `src/core/ogposet.rs` — see [[core-ogposet]]: four
signed adjacency tables (`faces_in`, `faces_out`, `cofaces_in`, `cofaces_out`,
each indexed `[dim][cell]`), `dim: isize` with `-1` the empty shape, and a
`normal` flag recording canonical cell order. The orientation is
`ogposet::Sign` (`Input` / `Output`, plus the query convenience `Both`).

The defining predicates of an RDC live here as methods on `Ogposet`:

- **Roundness** — `Ogposet::is_round` is the directed-sphere condition: it
  requires the shape be `is_pure` *(internal)*, then walks layers via
  `build_layer` *(internal)* checking input and output interiors are disjoint
  at every dimension.
- **Boundaries $\partial^\pm_k$** — `ogposet::boundary` extracts the faithful
  sign-side $k$-boundary sub-ogposet; `ogposet::boundary_traverse` the
  *normalised* one (both `pub(super)`). The frontier of $\alpha$-extremal cells
  is `Ogposet::extremal` *(internal)*, defined by *missing cofaces*.
- **Atoms / closures** — `ogposet::traverse` *(internal)* computes the downward
  closure of a seed and emits it in canonical input-first order;
  `ogposet::signed_k_boundary_of_cell` gives $\partial^\alpha_k$ of a single
  cell; `ogposet::normalisation` puts a shape in canonical form, the key to
  deciding shape equality via `ogposet::find_isomorphism`.

This shape is **carried** by [[core-diagram|Diagram]]: an `Arc<Ogposet>`
(`Diagram::shape`, a field), a label per cell, and a paste history. What is
regular is each value's *shape* — every generator classifier and let-bound
diagram a [[core-complex|Complex]] holds is a molecule, hence an RDC. The
assembled `Complex` — the *type* — is in general the looser
[[directed-complex]], because its labelling may identify cells across those
regular shapes; the `Complex` adds naming, scoping, and identification, no new
shape mathematics.

## Related

- [[directed-complex]] — the looser shape a *type* is; an RDC is the regular case.
- [[oriented-graded-poset]] — the unconstrained substrate an RDC refines.
- [[molecule]] — the pasted shapes; [[atom]] — its cells.
- [[boundary]] — the $\partial^\pm_k$ operators regularity uses.
- [[diagram]] — a labelled molecule; [[partial-map]] — maps between complexes.
- [[0001-no-identities]] · [[0002-round-boundaries]] — the design consequences.
