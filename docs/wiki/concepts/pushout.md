---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Pushout

A **pushout** glues two objects together along a shared piece. Given a span of
maps $B \xleftarrow{f} A \xrightarrow{g} C$ out of a common $A$, the pushout
$B +_A C$ is the object obtained from the disjoint union of $B$ and $C$ by
identifying, for each $a \in A$, its image $f(a)$ with its image $g(a)$. It is
the universal such gluing: the colimit of the span.

In alifib this is the engine of substitution. To rewrite, we match a copy of a
rule's input $U$ inside a target diagram $D$, then **replace** that copy by the
rule's body. The replacement is precisely a pushout: $U$ is the seam, $D$ is one
arm, the rule cell $r$ is the other, and the step is $S = D +_U r$. This gluing
builds a *larger* shape — it is the pasting/substitution that realises
[[rewriting]], not a reduce-to-one-cell composition.

## Definition

Let $A, B, C$ be oriented graded posets (see [[oriented-graded-poset]]) and let
$f \colon A \to B$, $g \colon A \to C$ be embeddings (injective on cells,
respecting faces and orientation). The **pushout** is an ogposet
$P = B +_A C$ with injections

$$ \iota_B \colon B \to P, \qquad \iota_C \colon C \to P $$

making the square commute, $\iota_B \circ f = \iota_C \circ g$, and universal: for
any cocone $(Q, h_B, h_C)$ with $h_B \circ f = h_C \circ g$ there is a unique
$u \colon P \to Q$ factoring both. Concretely the cells of $P$ are

$$ \mathrm{cells}(P) \;=\; \mathrm{cells}(B) \;\sqcup\; \bigl(\mathrm{cells}(C) \setminus g(\mathrm{cells}(A))\bigr), $$

i.e. all of $B$ plus exactly those cells of $C$ that are *not* in the shared
image. A cell of $C$ that came from $A$ is folded onto its partner $f(g^{-1}(c))$
in $B$; a fresh cell of $C$ keeps its faces, but each face is rewritten through
$\iota_C$ so it points into the glued object. Faces, cofaces, and the
input/output orientation $\partial^-,\partial^+$ all transport along the
injections, so $P$ is again a well-formed ogposet.

For the gluing to be a sound diagram operation $A$ must sit inside both arms as
the *same* shape — the seam is a sub-ogposet shared on the nose. Matching is what
manufactures the left leg $f$: it exhibits a copy of the pattern $U$ as a literal
subobject of the target $D$ (see [[rewriting]]).

### Multi-pushout: a genuine multi-way colimit

The binary pushout is the case of **one** extension glued to a base. alifib also
needs to fire several rule matches in a single parallel step, and that is not a
sequence of binary pushouts — it is one colimit of a wide diagram: a base $B$
together with extensions $C_1, \dots, C_n$, each attached along its own seam
$A_i$ via a span $A_i \xrightarrow{f_i} B$, $A_i \xrightarrow{g_i} C_i$.

$$ P \;=\; B \;+_{A_1} C_1 \;+_{A_2} C_2 \;\cdots\; +_{A_n} C_n . $$

The matches of a parallel family are disjoint on top-dimensional cells (their
seams may still share lower boundary cells, which all live in $B$ anyway), so
each extension contributes its fresh cells independently and the colimit is
computed in one pass. The result carries one injection $\iota_B \colon B \to P$
and a family $\iota_{C_i} \colon C_i \to P$. The binary pushout is recovered as
the $n = 1$ singleton — and indeed alifib *defines* it that way, delegating to
the multi-way construction rather than duplicating the gluing logic.

The choice of which arm plays the role of "base" is a pragmatic one: taking the
larger codomain as the base minimises the number of cells that must be freshly
allocated, since base cells are inherited verbatim.

## Implementation

Realised by `pushout` and `multi_pushout` in `src/core/pushout.rs`, the
ogposet-level colimit underneath [[core-matching]].

- `pushout::multi_pushout` *(internal)* — the genuine multi-way colimit. Takes a
  base ogposet and a slice of `pushout::Span` values (each a pair of embeddings
  `into_base`, `into_ext` sharing a domain) and returns a `MultiPushout`: the tip
  $P$, the base injection `inl`, and one injection per extension in `inrs`. Fresh
  cells are exactly the ext-cells with no preimage under $g$ (the `NO_PREIMAGE`
  test); their faces/cofaces are rewritten through the running injection maps.
- `pushout::pushout` *(internal)* — the binary wrapper. It picks the larger
  codomain as base, calls `multi_pushout` with a single `Span`, and un-swaps the
  injections so the result `Pushout { tip, inl, inr }` matches the original
  $f, g$ order.
- The injections are [[partial-map]] / `Embedding` values built by
  `Embedding::make`; the tip is an [[oriented-graded-poset]] assembled by
  `Ogposet::make` (see [[core-ogposet]]).

Used by:

- [[rewriting]] via `matching::construct_parallel_step` *(internal,
  `src/core/matching.rs`)*, the live rewrite path. It assembles one `Span` per
  confirmed match — `into_base` the matched copy $\iota \colon U \to D$ in the
  target, `into_ext` the rule's precomputed `pattern_to_rewrite`
  $j \colon U \to r$ — and calls `multi_pushout` for the whole family at once (a
  single binary pushout when the family is a singleton), then merges labels from
  the target and the rewrite diagrams over the tip.

Matching glues via `multi_pushout` directly. The binary `pushout::pushout`
wrapper is instead the gluing behind the basic diagram operations in
`src/core/diagram.rs`: `Diagram::paste` (gluing two diagrams along their shared
$k$-boundary, the pasting $\#_k$) and `Diagram::cell_with_input_embedding`
(gluing a cell's input and output boundary diagrams into the new cell's boundary
sphere). Both build *larger* shapes — pasting and cell construction, not
reduce-to-one-cell composition.

The colimit is purely combinatorial: it glues *shapes*. Cell **labels** (the
[[atom]] tags decorating positions) are not part of the universal property — they
are reattached afterwards by the callers, base labels taking precedence and
fresh-cell labels pulled back through each `inr` injection's inverse.

## Related

- [[oriented-graded-poset]] — the objects being glued.
- [[partial-map]] — the embeddings forming the span and the resulting injections.
- [[rewriting]] — the operation that supplies the span and consumes the tip.
- [[diagram]] — labelled molecules; pushout glues shapes, labels are merged after.
- [[boundary]] — the seam $U$ is matched and glued along boundaries
  $\partial^\pm_k$.
