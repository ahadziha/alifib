---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Diagram

A **diagram** is a labelled [[molecule]]: a [[regular-directed-complex|regular
directed]] [[oriented-graded-poset]] shape, every cell of which carries a label —
the tag of the generator ([[atom]]) it instantiates. (A let-binding names a
*whole* diagram in the ambient complex; it never appears as a cell label.) Where
a molecule is a *shape*, a diagram is that shape *decorated*. It is the runtime
value alifib computes with.

A diagram is **not** itself an RDC, and alifib is not "an interpreter for
RDCs". In the book's terms (Hadzihasanovic 2024, 5.3.13–5.3.16), a diagram of
shape $U$ in a strict $\omega$-category $X$ is a strict functor
$d : \mathsf{Mol}/U \to X$ — a *pasting diagram* when $U$ is a molecule. As an
object of $X$ it is an arbitrary colimit of atoms, free to identify cells (the
loop `a : pt -> pt` labels both endpoints of an arrow with the same point);
the glued object is generally not regular, often not even an oriented graded
poset. Only the shape $U$ is an RDC. alifib never stores the functor: it
stores the *combinatorial diagram* $\ell(d) : U \to X$ of 5.3.14 — the
labelling. **Proposition 5.3.15** says $\ell(d)$ determines $d$ uniquely
*because* the shape is regular; on a non-regular shape the `(shape, labels)`
encoding would be ambiguous. The shape's regularity is thus not what alifib
is "about" — it is what makes the value representation faithful.

A **type** is the same phenomenon writ large: assembled by identifying the
boundaries of many cells through labelling, the realised object need only be a
[[directed-complex]] — not necessarily regular (see [[directed-complex]]).
Labels collapse regular shapes into non-regular colimits at both levels; what
every diagram value keeps regular is its shape.

## Definition

A diagram $U$ has an underlying shape with cells stratified by dimension. Its
**top dimension** $\dim U$ is the highest dimension at which a cell occurs (the
empty diagram has $\dim = -1$). For each dimension $k \le \dim U$ and each
polarity, $U$ has a **boundary** (see [[boundary]]):

- the **input** $k$-boundary $\partial^-_k U$,
- the **output** $k$-boundary $\partial^+_k U$,

each itself a $k$-dimensional diagram, obtained by restricting $U$ to the
appropriate side of its $k$-skeleton. The two top boundaries $\partial^\pm_{n-1}U$
of an $n$-diagram are its *input* and *output*; when together they form a directed
sphere — input and output interiors disjoint at every dimension — the diagram is
**round** (see [[boundary]]). Roundness is a property of the *shape* alone (it
ignores labels), and it is the precondition for a diagram to be the input or output
boundary of a single [[atom|cell]] one dimension up — *not* a precondition for
pasting.

### Pasting ($\#_k$)

Two diagrams $U, V$ may be **pasted** along a shared $k$-boundary when the output
$k$-boundary of $U$ matches the input $k$-boundary of $V$ — same shape *and* same
labels:
$$
\partial^+_k U \;=\; \partial^-_k V
\quad\Longrightarrow\quad
U \#_k V .
$$
The result glues $U$ and $V$ along that boundary (a pushout of shapes) and is the
basic way larger diagrams are built from atoms.

**Pasting is not composition.** Pasting combines cells into a *larger* diagram; it
never reduces the pair to a single cell. Reducing to one cell would be
*composition*, a higher-algebraic operation that plain alifib types do not have.
So do not read $\#_k$ as the labelled analogue of categorical composition. Pasting
*is* associative and *is* unital — the boundaries act as units, with no separate
identity cells, since alifib has **no identities** (see [[0001-no-identities]]).

The juxtaposition `f g` written in the surface syntax is **principal pasting**:
shorthand for $f \#_k g$ at $k = \min(\dim f, \dim g) - 1$, the largest $k$ at
which the two can meet. Anything written with an explicit `#n` is the general
$\#_n$.

### Atoms as cells

A single generating cell is a diagram whose shape has a *greatest element* — the
defining property of an [[atom]] (not "one top-dimensional cell": a whiskered
2-cell has one 2-cell yet two maximal cells). An $n$-generator
$a : U \to V$ is determined by its parallel input/output boundaries $U, V$ (each
an $(n-1)$-diagram); pasting that data along the shared lower boundary and adding
$a$ on top yields its **classifier** diagram. See [[atom]].

## Implementation

`Diagram` in `src/core/diagram.rs` — see [[core-diagram]]. Concretely:

- The shape is an `Arc<Ogposet>` ([[oriented-graded-poset]]); labels are
  `Vec<Vec<Tag>>`; a `paste_history` records the $\#_k$ tree that built it.
- **Atoms** are made by `Diagram::cell` from `CellData` (`Zero` for a 0-cell,
  `Boundary { boundary_in, boundary_out }` for the globular data of an $n$-cell).
- **Pasting** $\#_k$ is `Diagram::paste(k, u, v)`, gated by `Diagram::pastability`
  (boundary agreement only — *not* roundness).
- **Boundaries** $\partial^\pm_k$ are `Diagram::boundary(Sign, k, &d)` and
  `Diagram::boundary_normal`, with `Sign::Input` $= \partial^-$ and
  `Sign::Output` $= \partial^+$ — see [[boundary]].
- **Top dimension** is `Diagram::top_dim` (with `dim()` returning $-1$ for the
  empty diagram); roundness of the shape is `Diagram::is_round`.
- **Surface syntax**: juxtaposition parses to `ast::Diagram::PrincipalPaste`,
  explicit `#n` to `ast::Diagram::Paste` (`src/language/parser.rs`); both are
  interpreted in `src/interpreter/diagram.rs`, where the principal dimension is
  `prev.top_dim().min(d_right.top_dim()).checked_sub(1)` — pasting below
  dimension $0$ is an error, not a fallback.

Diagrams are stored in a [[core-complex|Complex]] both as classifiers (for
generators) and as let-bound values. Rewriting builds new diagrams through
`matching::construct_parallel_step` → `pushout::multi_pushout` — see [[rewriting]].

## Related

[[molecule]] · [[atom]] · [[boundary]] · [[regular-directed-complex]] ·
[[directed-complex]] · [[oriented-graded-poset]] · [[rewriting]]
