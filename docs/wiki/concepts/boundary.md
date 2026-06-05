---
kind: concept
status: stable
last-touched: 2026-06-05
---

# Boundary

Every [[diagram]] has a *frontier*. A pasting diagram of top dimension $n$ runs
**from** an input shape **to** an output shape: these are its boundaries. More
finely, for each $k \le n$ there is an input boundary $\partial^-_k$ and an output
boundary $\partial^+_k$ — the $k$-skeleton you reach by following faces *against*
the orientation (input) or *with* it (output). Boundaries are the joints along
which diagrams compose ($\#_k$ glues $\partial^+_k U$ to $\partial^-_k V$) and the
silhouettes a rule's input must match during [[rewriting]].

## Definition

Work inside a [[regular-directed-complex]] — an [[oriented-graded-poset]] whose
faces carry an input/output orientation. A diagram $U$ of dimension $n = \dim U$
has, for every $k$ and every sign $\alpha \in \{-,+\}$, a **$k$-boundary**
$\partial^\alpha_k U$.

Following Hadzihasanovic, the boundary is built by *downward closure from the
extremal cells*. Start from the $\alpha$-extremal cells in dimension $k$ — those
not in the $\alpha$-input of any $k$-cell. Then descend: at each lower dimension
$j < k$ take the faces (of either sign) of cells already chosen, together with the
maximal $j$-cells. The result is a sub-complex of dimension $\le k$, the
$(\alpha, k)$-boundary, equipped with its inclusion into $U$.

$$
\partial^-_k U \ \xhookrightarrow{}\ U \ \xhookleftarrow{}\ \partial^+_k U .
$$

Three clamps make this total and well-behaved:

- **Saturation.** If $k \ge n$ the boundary is all of $U$: a diagram is its own
  top boundary. So $\partial^\pm_n U = U$.
- **The empty diagram.** Dimension is allowed to be $-1$ (no cells); its boundary
  is empty.
- **0-cells have no proper boundary.** A point is its own boundary at every $k$;
  there is no $\partial_{-1}$. This is why the *input/output* of an $n$-diagram
  is taken at $k = n-1$, and undefined ($n = 0$) for a point.

**Input and output.** The everyday boundary — what a $1$-cell's two endpoints
are, what a $2$-cell's left and right composite paths are — is the codimension-one
boundary:
$$
\operatorname{in} U = \partial^-_{n-1} U,
\qquad
\operatorname{out} U = \partial^+_{n-1} U,
\qquad n = \dim U \ge 1 .
$$

**Globularity.** Boundaries nest: lower boundaries of $U$ agree with the
boundaries of its boundaries,
$$
\partial^\alpha_j \, \partial^\beta_k U \ = \ \partial^\alpha_j U
\qquad (j < k),
$$
the globular identities that make a diagram a coherent shape rather than a loose
heap of cells.

### Roundness

A diagram is **round** when its input and output boundaries are *disjoint* in
every dimension below the top — they share no cell except where forced — so that
together they close up into a directed sphere. Concretely (after the trivial
cases $\dim \le 0$ and a single top cell) one accumulates the input and output
layers dimension by dimension and checks they stay disjoint at each level.

Roundness is a property of the **shape**, not of the labelling: it is read off
the bare [[oriented-graded-poset]], ignoring which generators sit on the cells.
And it is the precondition for a diagram to be the **input or output boundary of
a cell** — the gate of cell construction, where the two boundary diagrams must be
round and parallel before they can bound a single $(n{+}1)$-cell (see
[[0002-round-boundaries]]). It is *not* a precondition for **pasting**: $\#_k$
glues along a *shared* $k$-boundary and asks only that
$\partial^+_k U = \partial^-_k V$, never re-checking roundness of its arguments.
The round shapes are the [[regular-directed-complex|regular]] ones; the labelling
that bounds a cell may still identify cells, so the [[directed-complex|type]] it
builds need not itself be regular.

## Implementation

Boundaries live in `src/core/diagram.rs` — see [[core-diagram]].

- `Diagram::boundary(sign, k, d)` returns the raw $(\alpha, k)$-boundary as a new
  diagram. It computes the effective dimension `k.min(d.shape.dim)` (clamping the
  saturation case and forcing $0$ for the empty diagram), defers the combinatorics
  to `ogposet::boundary` *(internal)*, pulls back the labels along the inclusion
  embedding, and trims the paste history to match.
- `Diagram::boundary_normal(sign, k, d)` is the same, but the underlying shape is
  re-traversed into canonical form via `ogposet::boundary_traverse` *(internal)*
  — use it when the boundary's cell ordering must be deterministic.
- `boundary_history(histories, sign, k)` *(internal)* clamps the recorded pasting
  history to the boundary dimension. The test
  `boundary_normal_clamps_history_to_top_dim` pins the subtle invariant: asking
  for the $5$-boundary of a point still yields a one-level diagram, never five
  manufactured history levels. Shape, labels, and history lengths must always
  agree.
- `Diagram::boundary_correspondence(sign, k, parent, boundary_diag)`
  *(internal, `pub(crate)`)* recovers how an independently-computed boundary sits
  inside its parent, by finding the isomorphism to the freshly-extracted boundary.

The `Sign` enum is `Input`/`Output`, mapped to the ogposet's input/output sign
by `Sign::as_ogposet_sign`. Roundness is `Diagram::is_round`, delegating to
`ogposet::is_round` (the directed-sphere disjointness condition). It is enforced
at **cell construction**: `Diagram::parallelism` *(internal)* rejects a non-round
input or output boundary before building the cell. `Diagram::paste`'s gate
(`pastability`) checks only that $\partial^+_k U = \partial^-_k V$ in shape and
labels — it does *not* re-check roundness of its arguments.

The codimension-one input/output reading is mirrored in [[output]]:
`normalize::cell_from_diagram` *(internal)* takes `k = top_dim() - 1` (via
`checked_sub(1)`, returning an empty cell for a $0$-cell) and calls
`Diagram::boundary(Sign::Input, k, …)` / `Diagram::boundary(Sign::Output, k, …)`
to render a generator's input and output faces.

A generator's declared boundary is stored directly on its `CellData::Boundary
{ boundary_in, boundary_out }`, or
`CellData::Zero` for a point — see [[core-diagram]] and [[core-complex]].

## Related

[[diagram]] · [[molecule]] · [[oriented-graded-poset]] ·
[[regular-directed-complex]] · [[directed-complex]] · [[rewriting]] · [[output]] ·
[[atom]]

## Notation

House style: $\partial^-_k$ (input), $\partial^+_k$ (output); $\#_k$ for pasting
along the $k$-boundary. See [[CLAUDE]].
