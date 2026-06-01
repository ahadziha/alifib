---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Flow graph

The **flow graph** turns a directed diagram into an ordinary labelled digraph,
so that "does this rewrite rule fire here?" becomes "does the pattern's graph
sit inside the target's graph as an induced labelled subgraph?". It is the
device that makes subdiagram matching tractable: the geometry of an
$(n{+}1)$-dimensional pasting is recast as combinatorics on the $k$-cells one
dimension down.

The intuition: fix a level $k$. Each cell $x$ of dimension $> k$ has an *input*
and an *output* $k$-boundary, $\partial^-_k(x)$ and $\partial^+_k(x)$, both sets
of $k$-cells. Draw an edge $x \to y$ whenever the output of $x$ feeds the input
of $y$ — whenever $\partial^+_k(x) \cap \partial^-_k(y) \neq \varnothing$. The
$k$-cells are the *wires*; the higher cells are *gates*; the flow graph records
which gate's output flows into which gate's input.

## Definition

Let $U$ be a [[regular-directed-complex|regular directed complex]] (an
[[oriented-graded-poset]]) and fix $0 \le k < \dim U$. Write
$\Delta^-_k(x)$ and $\Delta^+_k(x)$ for the input and output $k$-boundary of a
cell $x$ — the $k$-dimensional cells of $\partial^-_k(\mathrm{cl}\,x)$ and
$\partial^+_k(\mathrm{cl}\,x)$ respectively.

The **$k$-flow graph** $F_k(U)$ (Definition 61, Hadzihasanovic–Kessler) is the
directed graph whose

- **vertices** are all cells of $U$ of dimension strictly greater than $k$, and
- **edges** are $x \to y$ exactly when
  $$ \Delta^+_k(x) \cap \Delta^-_k(y) \neq \varnothing, \qquad x \neq y. $$

The **maximal $k$-flow graph** $M_k(U)$ is the induced subgraph of $F_k(U)$ on
the *maximal* cells — those with no cofaces in either direction. For a pure
molecule the top cells are exactly the maximal ones, so at the top level the two
agree: $M_{n-1}(U) = F_{n-1}(U)$ when $\dim U = n$.

In matching we work at $k = n-1$ for an $n$-dimensional [[diagram]]. Then the
vertices are the $n$-cells (the atoms being rewritten) together with any maximal
lower cells, and an edge $a \to b$ says: an output $(n{-}1)$-face of $a$ is an
input $(n{-}1)$-face of $b$, i.e. $a$ must be composed *before* $b$ along that
shared face. The flow graph is thus a directed picture of the **pasting order**
$\#_{n-1}$ inside the diagram.

### Matching as path-induced subgraph isomorphism

A rewrite rule presents a pattern molecule $P$ (the input $n$-boundary of the
rule). A subdiagram of the target $T$ matching $P$ is, up to the labelling,
a copy of $F_{n-1}(P)$ sitting inside $F_{n-1}(T)$ in a way that respects both
the cell labels and the flow edges. Concretely, an injection
$f : V(F_{n-1}(P)) \hookrightarrow V(F_{n-1}(T))$ qualifies when it is

- **label-preserving**: each matched cell carries the same generator label as
  its image (the [[atom|atomic]] label, an [[oriented-graded-poset|ogposet]] tag); and
- **induced** (the "path-induced" condition): for all $u, v$,
  $$ u \to v \text{ in } F_{n-1}(P) \iff f(u) \to f(v) \text{ in } F_{n-1}(T). $$

The biconditional — edge *iff* edge, in both directions — is what makes this an
*induced* (not merely monotone) subgraph embedding: the candidate may neither
invent nor drop an adjacency relative to the pattern. Confirming the geometric
match (a genuine [[partial-map]] / [[boundary]] isomorphism of subdiagrams) is a
second, more expensive step; the flow graph is the cheap combinatorial filter
that proposes candidates.

## Implementation

Realised in `src/core/flow.rs` and consumed by the matcher in
`src/core/matching.rs`, both documented on [[core-matching]].

- `flow::flow_graph` builds $F_k(U)$: vertices are cells of dimension
  $k{+}1,\dots,\dim U$, edges added when the output and input $k$-boundaries
  intersect (`intset::is_disjoint` negated). The per-cell boundaries come from
  `ogposet::signed_k_boundary_of_cell` (see [[core-ogposet]]). It returns the
  `DiGraph` together with a `node_map: Vec<(dim, pos)>` recovering each vertex's
  original cell. Edge cases ($k \ge \dim U$, empty complex) yield an empty graph.
- `flow::maximal_flow_graph` builds $M_k(U)$ the same way but iterates only over
  `Ogposet::maximal(dim)`.
- `matching::TargetFlowData` *(internal)* caches the target's flow graph,
  `node_map`, and the label slice once per diagram — building the flow graph is
  the dominant per-step cost — so it is reused across every rule in a session.
- `matching::find_path_induced_matches` *(internal)* performs the labelled
  induced-subgraph search by backtracking with most-constrained-variable
  ordering; `backtrack_subgraph` enforces the edge-iff-edge biconditional in both
  directions. Its doc comment states exactly the two conditions above.

The behavioural anchor is the test `flow::tests::flow_graph_two_arrow_paste`:
pasting two arrows $f \#_0 g$ at a shared midpoint $m$ yields the edge
$f \to g$ and *no* edge $g \to f$, because the disjoint endpoints $a, b$ share
nothing — the flow graph recovers the composition order from the geometry.

## Related

- [[oriented-graded-poset]] — the substrate whose signed $k$-boundaries define the edges.
- [[boundary]] — $\partial^\pm_k$, the input/output faces the flow graph reads.
- [[diagram]] — flow graphs are computed on the shape of a labelled diagram.
- [[rewriting]] — matching is the entry point that drives flow-graph construction.
- [[partial-map]] — the geometric isomorphism that *confirms* a flow-graph candidate.
