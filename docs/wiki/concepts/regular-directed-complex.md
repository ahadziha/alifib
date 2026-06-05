---
kind: concept
status: stable
last-touched: 2026-06-05
---

# Regular directed complex

A **regular directed complex** (RDC) is the class of
[[oriented-graded-poset|oriented graded posets]] whose every cell is *regular* —
its input and output boundaries each a well-shaped sphere, realised by an honest
CW-cell that embeds. RDCs are the shapes of *values*: every [[molecule]] is one,
and the [[atom|atoms]] it is pasted from are precisely the cells of an RDC. An
alifib *type*, by contrast, is the more general [[directed-complex|directed
complex]] — regularity is a property of the *shapes* a type holds, not of the
type itself, which may freely identify cells (a point with a directed loop is the
standard witness). The theory is Hadzihasanovic's (*Combinatorics of
higher-categorical diagrams*, 2024).

The single most consequential fact about RDCs, for a programmer, is what they
*lack*: **no identity cells**. There is no degenerate $(k{+}1)$-cell sitting over
a $k$-cell. Everything is honest, directed geometry. alifib inherits this
wholesale — see [[0001-no-identities]].

## Definition

Start from an [[oriented-graded-poset]]: a poset $P$ graded by $\dim$, in which
the covering relation $x \lessdot y$ (so $\dim y = \dim x + 1$) is *oriented*,
each such cover tagged $-$ (input) or $+$ (output). This splits the faces of a
cell $y$ into its input faces $\partial^-_{\dim y - 1} y$ and output faces
$\partial^+_{\dim y - 1} y$.

For a closed subset $U \subseteq P$ (a sub-ogposet, downward-closed) and a sign
$\alpha \in \{-,+\}$, the **$k$-boundary** $\partial^\alpha_k U$ is read off the
orientation: the closure of the $\alpha$-extremal $k$-cells together with the
faces forced beneath them. We write $\partial^-_k$, $\partial^+_k$, and
$\partial_k$ for the input, output, and full $k$-boundary.

A cell $x$ of dimension $n$ is **regular** when, recursively, the closure
$\mathord{\downarrow} x$ of $x$ is an *atom*: an oriented graded poset with a
single top cell whose boundaries $\partial^-_{n-1} x$ and $\partial^+_{n-1} x$
are themselves **round** $(n{-}1)$-dimensional shapes — globular spheres in the
directed sense. *Roundness* is the condition that, layer by layer down from the
top, the input interior and the output interior stay disjoint, so the boundary
genuinely looks like a sphere split into an input hemisphere and an output
hemisphere. (This is exactly what `Ogposet::is_round` checks; see below.)

An **oriented graded poset is a regular directed complex** when every one of its
cells is regular in this sense. The condition is local and recursive: an
$n$-cell is regular precisely when its boundary spheres are round and their own
cells are, in turn, regular. Atoms are the indivisible regular shapes;
[[molecule|molecules]] are the shapes obtained by *pasting* atoms along shared
round boundaries with $\#_k$.

### Why no identities

In a strict $\omega$-category every $k$-cell $f$ carries an identity
$(k{+}1)$-cell $\mathrm{id}_f$ with $\partial^-_k \mathrm{id}_f =
\partial^+_k \mathrm{id}_f = f$. Such a cell is *degenerate*: its two boundary
hemispheres coincide, so it is not round, so it is **not a regular cell**. The
RDC framework therefore has nowhere to put an identity. Composition is recovered
not by degeneracy but by genuine pasting of distinct atoms. The consequence for
[[partial-map|partial maps]] is *not* that they preserve dimension: a map may
collapse a $1$-cell, sending it to the genuine $0$-cell its endpoints fall onto.
There is no identity cell to send it to, so it goes to the lower-dimensional
image itself — dimension-*lowering* maps are legitimate, and collapse inference
relies on them; the only guard is against dimension-*raising*. Fully spelled out
in [[0001-no-identities]].

## Implementation

The RDC substrate is **realised** by `Ogposet` in `src/core/ogposet.rs` — see
[[core-ogposet]]. An `Ogposet` is exactly a finite oriented graded poset: four
signed adjacency tables (`faces_in`, `faces_out`, `cofaces_in`, `cofaces_out`,
each indexed `[dim][cell]`) plus `dim: isize`, where `dim = -1` is the empty
shape. The $\pm$ orientation is the `Sign` enum (`Input` / `Output` / `Both`).

The defining predicates of an RDC live here as methods on `Ogposet`:

- **Roundness** — `Ogposet::is_round` (pub) is the directed-sphere condition
  above: it requires the shape be `is_pure` (internal), then walks layers via
  `build_layer` (internal) checking the input and output interiors are disjoint
  at every dimension.
- **Boundaries $\partial^\pm_k$** — `ogposet::boundary` extracts the faithful
  sign-side $k$-boundary sub-ogposet; `ogposet::boundary_traverse` returns the
  *normalised* one (both `pub(super)`). The frontier of $\alpha$-extremal cells
  is `Ogposet::extremal` (internal), defined by *missing cofaces*.
- **Atoms / closures** — `ogposet::traverse` (internal) computes the downward
  closure $\mathord{\downarrow} x$ of a seed and emits it in canonical
  input-first order; `ogposet::signed_k_boundary_of_cell` gives $\partial^\alpha_k(x)$
  of a single cell; `ogposet::normalisation` puts a shape in canonical form, the
  key to deciding shape equality via `ogposet::find_isomorphism`.

This shape is **carried** by [[core-complex]]: a `Complex` stores
[[diagram|diagrams]], and a `Diagram` is exactly an `Arc<Ogposet>` shape
(`Diagram::shape`, `src/core/diagram.rs`) plus a label at each cell. What is
regular is each generator's classifier *shape*: the combinatorial RDC lives
inside every named generator and let-binding a `Complex` holds. The assembled
`Complex` — the *type* — is in general the looser
[[directed-complex|directed complex]], because its labelling may identify cells
across those regular shapes; the `Complex` adds the naming, scoping, and
identification, no new shape mathematics.

## Related

- [[directed-complex]] — the looser shape a *type* is; an RDC is the regular case.
- [[oriented-graded-poset]] — the unconstrained substrate an RDC refines.
- [[molecule]] — the pasted shapes that live in an RDC; [[atom]] — its cells.
- [[boundary]] — the $\partial^\pm_k$ operators the regularity condition uses.
- [[diagram]] — a labelled molecule; [[partial-map]] — maps between complexes.
- [[0001-no-identities]] — the design consequence of regularity.
