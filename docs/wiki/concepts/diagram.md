---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Diagram

A **diagram** is a labelled [[molecule]]: a [[regular-directed-complex|regular
directed]] [[oriented-graded-poset]] shape, every cell of which carries a label —
a generator ([[atom]]) or, via a let-binding, the name of another diagram. Where a
molecule is a *shape*, a diagram is that shape *decorated*. It is the runtime
value alifib computes with.

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
**round** (see [[boundary]]), the precondition for it to be a single [[atom]]'s
boundary.

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
basic way larger diagrams are built from atoms. Pasting is associative and the
labelled analogue of composition in a higher category — but note alifib has **no
identities** (see [[0001-no-identities]]), so there are no degenerate units.

### Atoms as cells

A single generating cell is a diagram with one top cell. An $n$-generator
$a : U \to V$ is determined by its parallel input/output boundaries $U, V$ (each
an $(n-1)$-diagram); pasting that data along the shared lower boundary and adding
$a$ on top yields its **classifier** diagram. See [[atom]].

## Implementation

`Diagram` in `src/core/diagram.rs` — see [[core-diagram]]. Concretely:

- The shape is an `Arc<Ogposet>` ([[oriented-graded-poset]]); labels are
  `Vec<Vec<Tag>>`; a `paste_history` records the $\#_k$ tree that built it.
- **Atoms** are made by `Diagram::cell` from `CellData` (`Zero` for a 0-cell,
  `Boundary { boundary_in, boundary_out }` for the globular data of an $n$-cell).
- **Pasting** $\#_k$ is `Diagram::paste(k, u, v)`, gated by `pastability`.
- **Boundaries** $\partial^\pm_k$ are `Diagram::boundary(Sign, k, &d)` and
  `Diagram::boundary_normal`, with `Sign::Input` $= \partial^-$ and
  `Sign::Output` $= \partial^+$ — see [[boundary]].
- **Top dimension** is `Diagram::top_dim` (with `dim()` returning $-1$ for the
  empty diagram); roundness is `is_round`.

Diagrams are stored in a [[core-complex|Complex]] both as classifiers (for
generators) and as let-bound values. Rewriting builds new diagrams via
`Diagram::whisker_rewrite` — see [[rewriting]].

## Related

[[molecule]] · [[atom]] · [[boundary]] · [[oriented-graded-poset]] · [[rewriting]]
