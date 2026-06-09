---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Reconstruction

A [[diagram]] is more than its shape: it carries a *paste history*, the record
of how it was built from [[atom|atoms]] by composition $\#_k$. **Reconstruction**
is the inverse problem. Given only the bare geometry — an
[[oriented-graded-poset]] $U$ together with a labelling of its cells by
generators — recover *a* paste history that realises $U$. We are handed the
shadow and asked to reinvent the gesture that cast it.

The need is acute after a rewrite. Matching produces the *shape* of the result
diagram (a pushout, an ogposet) and reads off *which generator sits on which
cell* (the labels), but the algebraic decomposition is lost in the pasting. To
turn that bare pre-diagram back into a first-class [[diagram]] with composable
boundaries, we must synthesise a layering and check it actually realises the
given shape.

## Definition

A **pre-diagram** is a pair $(U, \ell)$ of an ogposet $U$ and a labelling
$\ell$ assigning to each cell a generator tag. A **reconstruction** of $(U,\ell)$
against a [[core-complex|complex]] $\mathcal{C}$ is a paste tree $T$ — a binary
tree of leaves (generators) and $\#_k$-nodes — whose realisation
$\llbracket T \rrbracket_{\mathcal{C}}$ is a diagram isomorphic to $(U,\ell)$.
Reconstruction *succeeds* when such a $T$ exists; the witness is verified, not
trusted, by realising $T$ and comparing cell counts (the *size* invariant) and
ultimately isomorphism.

The synthesis is a **layering by topological sort of a [[flow-graph|flow graph]]**.
The geometry already tells us, at each level $k$, which cell's output boundary
feeds which cell's input boundary: that is exactly the edge relation
$\partial^+_k(x) \cap \partial^-_k(y) \neq \varnothing$ of the maximal $k$-flow
graph $\mathbf{M}_k(U)$. A composite $U = a_1 \#_k a_2 \#_k \cdots \#_k a_m$ can
only have been pasted in an order *compatible* with that flow — a topological
sort $x_1, \dots, x_m$ of $\mathbf{M}_k(U)$. Each admissible order proposes a
layering. (Layerings and their correspondence with topological sorts of
$\mathbf{M}_k$ are studied in Kessler 2025, *Computational Aspects of Rewriting
in Higher-Dimensional Diagrams*, `docs/papers/` — there as analysis of an
existing diagram; reconstruction runs the correspondence backwards, as
synthesis.)

### Choosing the decomposition dimension

At which level $k$ do we cut? The choice is governed by the
**layering dimension** of $U$: the least $k \ge -1$ such that $U$ has at most one
maximal cell of dimension $> k+1$. When the layering dimension is $-1$ — at most
one maximal cell of positive dimension — the ogposet is already irreducible and
reconstruction bottoms out in a leaf. Otherwise:

- For $\dim U > 3$, cut at $k = $ the layering dimension: the lowest level at
  which $U$ genuinely decomposes, keeping each layer as large as possible and
  the recursion shallow.
- For $\dim U \le 3$ (and $\dim U > 1$) prefer the *frame dimension* — the
  greatest $k$ whose maximal flow graph has at least one edge, descending from
  $\dim U - 1$ and falling back to $0$. In low dimension a finer cut is cheap
  and the candidate layering is always valid, so no search is needed.
- For $\dim U = 1$ always cut at $k = 0$.

### Layering

Fix the cut dimension $k$ and a topological sort $x_1, \dots, x_m$ of the chosen
flow graph. The layers are defined inductively as ogposet restrictions:

$$
L_1 = \partial^-_k(U) \,\cup\, \mathrm{cl}(x_1),
\qquad
L_i = \partial^+_k(L_{i-1}) \,\cup\, \mathrm{cl}(x_i) \quad (i > 1).
$$

Each $L_i$ glues the running output $k$-boundary of everything composed so far to
the down-closure of the next maximal cell $x_i$; the labelling is pulled back
along the inclusion $L_i \hookrightarrow U$. Recursing on each $L_i$ yields a
sub-tree $T_i$, and the layers are left-associated:

$$
T = (\cdots((T_1 \#_k T_2)\#_k T_3)\cdots)\#_k T_m.
$$

This is *one* candidate. In dimension $> 3$ a given topological sort need not
realise to the right shape — boundary algebra can fail to line up — so candidates
are tried lazily, the first that realises and matches sizes wins, and the search
is bounded (a cycle in the flow graph, or exhausting the bound, is failure). In
dimension $\le 3$ a single sort suffices: the candidate is always valid by
construction, so no realise-and-recheck round trip is needed.

## Implementation

Reconstruction lives in `src/core/reconstruct.rs`, the inverse counterpart of
[[core-paste-tree|paste-tree realisation]] and the supporting half of
[[core-matching|matching]].

- The entry point is `reconstruct::reconstruct` — it builds a candidate tree,
  realises it with `paste_tree::realise_tree` ([[core-paste-tree]]), and verifies
  via the size check `reconstruct::check_sizes` (internal).
- The recursive synthesiser is `reconstruct::build_paste_tree` (internal): it
  reads `Ogposet::layering_dimension`, picks the cut dimension, builds the
  maximal flow graph with `flow::maximal_flow_graph`, and either takes a single
  `graph::topological_sort` (dim $\le 3$) or enumerates lazily via
  `graph::try_topological_sorts` (dim $> 3$, bound $10\,000$).
- `reconstruct::build_layers` (internal) realises the inductive layer formula
  above — `ogposet::boundary` for the input/output $k$-boundaries,
  `reconstruct::restrict_ogposet` for the restriction, `pullback_labels` for the
  labelling — and `reconstruct::try_sort` (internal) left-associates the
  recursively built sub-trees into a `PasteTree`.

The **tip-dim $\le 3$ fast path** in [[core-matching|matching]]
(`matching::assemble_low_dim_step`, internal) does *not* call
`reconstruct::reconstruct` at all: it calls `reconstruct::build_tree` to get the
candidate tree and assembles the step's history directly, skipping the
`realise_tree` + `check_sizes` round trip. The switch is on the *step's*
dimension (`mp.tip.dim`), not the target's. The doc-comment on
`assemble_low_dim_step` records why: in low dimension the candidate paste tree is
always valid, so realisation is unnecessary. Only the tip-dim $> 3$ branch of
`matching` routes through full `reconstruct`.

Behavioural evidence: the round-trip tests `reconstruct_all_idem`,
`reconstruct_all_assoc`, `reconstruct_all_magma`, `reconstruct_all_category`,
`reconstruct_all_semigroup` and `reconstruct_all_total` reconstruct every
classifier and named diagram of a theory and assert isomorphism to the original
(`assert_reconstruct`, internal), pinning the inverse-of-realisation guarantee.

## Related

- [[flow-graph]] — the digraph whose topological sorts furnish the candidate
  layerings.
- [[core-paste-tree]] — the `PasteTree` reconstruction produces; `realise_tree` is
  the forward map it inverts.
- [[diagram]] — what a realised tree becomes.
- [[oriented-graded-poset]] — the bare substrate $(U,\ell)$ that reconstruction
  consumes.
- [[boundary]] — the $\partial^\pm_k$ used to assemble each layer.
- [[rewriting]] — the caller: matching hands reconstruction the pushout shape and
  labels of a rewrite result.
