---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Regular directed complex

A **regular directed complex** (RDC) is an [[oriented-graded-poset]] in
which the closure $\mathord{\downarrow} x$ of every cell is an [[atom]]
(5.3.1). The intuition is borrowed from topology: a *regular* CW complex is
one where every closed cell embeds — attached without identifications — and
an RDC is the directed analogue. Every [[molecule]] is an RDC (Lemma 3.3.12
plus Remark 5.3.2: each cell's closure in a molecule is an atom), but not
conversely — two disjoint points form an RDC and no molecule.

This page exists mainly to correct a tempting misreading, so let us state it
plainly up front.

## alifib does not represent RDCs

It is natural to summarise alifib as "an interpreter whose values are
regular directed complexes". That summary is wrong, and the book says why.

A *value* in alifib — what `let d = ...` binds — is a **diagram**: in the
book's terms a strict functor $d : \mathsf{Mol}/U \to X$ from the molecules
over a shape $U$ into the ambient $\omega$-category $X$ (5.3.13), called a
*pasting diagram* when $U$ is a molecule (5.3.16). Concretely, it is a
molecule each of whose cells is labelled with a generator. The labelling is
free to hit the same generator twice — and as soon as it does, the *glued
object* the diagram describes (an arbitrary colimit of atoms in $X$) stops
being regular, and frequently stops being an oriented graded poset at all:
the loop `a : pt -> pt` labels both endpoints of the walking arrow with
`pt`, and the realised loop would need its single covering edge
$pt \lessdot a$ to carry both signs, which the definition of orientation
forbids ([[directed-complex]] tells this story in full). So the objects
alifib manipulates are not RDCs. **Only their shapes are.**

Why, then, does shape-regularity deserve a page? Because of a theorem.

## Proposition 5.3.15, the licence for `(shape, labels)`

alifib never stores the functor $d$. It stores the *combinatorial diagram*
$\ell(d) : U \to X$ (5.3.14) — the labelling function — as a plain
`Vec<Vec<Tag>>` over the shape. For this to be an honest representation,
the labelling must *determine* the functor: two distinct diagrams must never
share a labelling.

That is exactly what Proposition 5.3.15 gives — **when $U$ is a regular
directed complex**, $\ell(d) = \ell(d')$ implies $d = d'$ — and exactly what
fails without regularity: the book is explicit (introduction to chapter 5)
that for a general oriented graded poset, functors out of $\mathsf{Mol}/U$
are *not* determined by labellings; the restriction to RDCs is what earns
the name "combinatorial diagram". The proof runs through the rigidity of
atoms (Theorem 5.3.7: every morphism between atoms of the same dimension is
an isomorphism), i.e. through regularity.

So the correct statement of alifib's relationship to RDCs is:

> alifib represents arbitrary (colimit-valued) pasting diagrams *by their
> labellings*, and Proposition 5.3.15 makes that representation faithful
> precisely because the shapes carrying the labels are regular.

Shape-regularity is not what alifib is *about*; it is what makes alifib's
data structure mean anything.

## How the code maintains regularity (it never checks it)

Search the codebase for an `is_regular` predicate and you will not find one.
Regularity is an *invariant maintained by construction*: every shape in the
system is born from the three molecule constructors, molecules are RDCs by
Lemma 3.3.12, and nothing else can mint a shape. The entire weight rests on
two gates in `src/core/diagram.rs`:

- **`Diagram::parallelism`** *(internal)* — the gate of the (Atom)
  constructor (`cell_with_input_embedding`): both declared boundaries must
  be round, equal dimension, with equal boundary spheres in shape and
  labels. See [[atom]] for the construction step by step.
- **`Diagram::pastability`** *(internal)* — the gate of (Paste)
  (`Diagram::paste`): output $k$-boundary of the left equals input
  $k$-boundary of the right, shapes and labels.

The honest status of this invariant: the (Paste) gate is sound — the
constructor accepts *any* boundary isomorphism, and exhibiting one via
canonical forms suffices. The (Atom) gate carries an unresolved subtlety:
the book requires the boundary isomorphism to restrict to each sign
($\varphi^\alpha : \partial^\alpha U \cong \partial^\alpha V$), and
`parallelism` checks only positional equality of whole boundary spheres,
relying on the traversal order to keep the hemispheres aligned. That
reliance is proven correct for generators of dimension $\le 3$ and open
above — [[atom-gluing-sign-invariant]]. If it ever failed, the resulting
shape would not be a molecule, and by 5.3.15's contrapositive the value
representation over it would silently lose uniqueness. Until the question is
closed, "every alifib shape is an RDC" is construction discipline plus a
partly-proven lemma, not a theorem about the code.

## No identities

The most consequential everyday fact about RDCs is what they *lack*:
identity cells. A degenerate $(k{+}1)$-cell over a $k$-cell $U$ would need
input and output boundaries both equal to $U$, making the two hemispheres of
its boundary coincide instead of meeting along a rim — not round, so its
closure is no atom. alifib inherits this wholesale; the consequence for
[[partial-map|partial maps]] — dimension-*lowering* collapse is fine, only
dimension-*raising* is barred — is [[0001-no-identities]].

## Implementation

The substrate is `Ogposet` (`src/core/ogposet.rs`, [[core-ogposet]]): four
signed adjacency tables (`faces_in`, `faces_out`, `cofaces_in`,
`cofaces_out`, indexed `[dim][cell]`), `dim: isize` with $-1$ the empty
shape, a `normal` flag for canonical order, and the orientation
`ogposet::Sign` (`Input` / `Output`, plus the query convenience `Both`).
The ingredients of regularity live there as methods:

- **Roundness** — `Ogposet::is_round`: purity, then the layer walk
  (`build_layer` *(internal)*) checking input/output interiors disjoint at
  every dimension; equivalent to the book's 3.2.5 on globular shapes, which
  molecules are — the fine print is in [[boundary]].
- **Boundaries** — `ogposet::boundary` / `boundary_traverse` *(internal)*,
  seeded by `Ogposet::extremal` *(internal)*.
- **Closures and canonical forms** — `ogposet::traverse`,
  `signed_k_boundary_of_cell`, `normalisation`, `find_isomorphism`
  *(all internal)*: downward closure of a cell, its $\partial^\alpha_k$,
  canonical order, and decidable shape isomorphism.

The shape is carried by [[core-diagram|Diagram]] (`Diagram::shape`, an
`Arc<Ogposet>`), alongside the labels that make it a value and the paste
history that certifies it a molecule. Every classifier and let-bound diagram
a [[core-complex|Complex]] holds has a molecule shape, hence an RDC shape;
the assembled `Complex` — the *type* — is the looser [[directed-complex]],
because labellings identify cells across those regular shapes. The `Complex`
adds naming, scoping, and identification; no new shape mathematics.

## Related

- [[diagram]] — what a value actually is (a functor, stored as its labelling).
- [[directed-complex]] — what the labelled colimits assemble to.
- [[molecule]] · [[atom]] — the grammar whose products are RDCs.
- [[boundary]] — $\partial^\pm_k$, globularity, roundness.
- [[oriented-graded-poset]] — the unconstrained substrate.
- [[partial-map]] · [[0001-no-identities]] · [[0002-round-boundaries]].
- [[atom-gluing-sign-invariant]] — the open soundness question at the (Atom) gate.
