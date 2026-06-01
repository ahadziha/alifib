---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Rewriting

A **rewrite step** takes a rule and a [[diagram]] and produces a new diagram by
*matching* the rule's input somewhere inside the current state and *substituting*
its output. A rule is an [[atom|generator]] of dimension $n+1$ — read as a
directed cell $r : U \Rightarrow V$ whose input boundary $\partial^-_n r = U$ is
the **input** pattern and whose output boundary $\partial^+_n r = V$ is the
**output**. Rewriting an $n$-dimensional diagram $D$ means: find a copy of $U$
inside $D$, carve it out, and paste $V$ in its place. The step itself is the
$(n{+}1)$-cell that *witnesses* this replacement — directed rewriting records not
just the result but the derivation.

alifib is, in its own words, "an interpreter for directed higher-categorical
rewriting": the whole machine exists to enumerate, validate, and compose these
steps.

## Definition

Fix a [[regular-directed-complex]] of generators and let $D$ be a diagram with
$\dim D = n$. A rule $r$ of dimension $n+1$ has input $U = \partial^-_n r$ and
output $V = \partial^+_n r$. A **match** of $r$ in $D$ is an embedding
$\iota : U \hookrightarrow D$ — an isomorphism onto a sub-diagram of $D$ that
respects orientation *and labels*. Two obligations must be met:

1. **Shape.** The image of $\iota$ must be a closed, well-oriented sub-poset of
   $D$ isomorphic to $U$ as an [[oriented-graded-poset]].
2. **Labels.** Every cell of $U$ must carry the same generator-tag as its image
   in $D$.

Finding such an $\iota$ naively is subgraph isomorphism — expensive. alifib
splits it into a **necessary prune** followed by a **sufficient check**:

- **Necessary: flow-graph filtering.** The $(n{-}1)$-[[flow-graph]] $\mathbf{F}_{n-1}$
  records, for each top cell, which cells feed into which: an edge $x \to y$
  whenever $\partial^+_{n-1}(x)$ and $\partial^-_{n-1}(y)$ overlap. Any genuine
  match must send the pattern's flow graph onto an *induced, label-preserving*
  subgraph of $D$'s. This is a cheap combinatorial filter that discards the vast
  majority of position sets before any geometry is checked.
- **Sufficient: isomorphism on the closure.** For each surviving candidate, take
  the order-closure of the matched cells in $D$ and test it for an actual ogposet
  isomorphism with $U$, checking labels along the way. Only this confirms a match.

Given a confirmed match $\iota : U \hookrightarrow D$, the new diagram is built by
**gluing** (a [[pushout]]). The rule $r$ also embeds its input,
$j : U \hookrightarrow r$. The step diagram is the colimit

$$ S \;=\; D +_U r, $$

the pushout of $\iota$ and $j$ over the shared input $U$. Its output boundary
$\partial^+_n S$ is the rewritten diagram — $D$ with $U$ replaced by $V$ — and $S$
itself is the $(n{+}1)$-dimensional cell recording the rewrite. Several
non-overlapping matches can be glued at once via an iterated pushout, giving a
single **parallel** step that applies them simultaneously.

The pushout yields a bare oriented graded poset with labels but *no* compositional
history. To turn it back into a usable diagram — one that knows how it pastes
together — alifib runs [[reconstruction]]: it searches for a paste tree over the
generators that realises the glued shape. This recovers the derivation that the
raw colimit forgets.

**Directedness.** Because input and output are *distinct* boundaries of the rule
($\partial^-$ vs. $\partial^+$, with alifib having [[partial-map|no identity
cells]]), rewriting is inherently oriented: $U \Rightarrow V$ is not the same as
$V \Rightarrow U$. Running a rule in reverse is a deliberate *backward* mode that
swaps which boundary is treated as the pattern.

## Implementation

The mathematics above is realised by [[core-matching]] and driven by
[[interactive-engine]].

- **Match = prune + check.** [[core-matching]] computes the target's
  $(n{-}1)$-[[flow-graph]] once per diagram (`matching::TargetFlowData`,
  `core::flow::flow_graph`), enumerates label-preserving induced subgraph
  embeddings (`matching::find_path_induced_matches` (internal)), then confirms each
  survivor with the closure isomorphism (`matching::check_match_isomorphism`
  (internal)). A precomputed `matching::RulePattern` holds the normalised input
  $U$ and its embedding $j$ into $r$.
- **Glue via [[pushout]].** Confirmed matches are turned into a step by
  `matching::construct_parallel_step` (internal), which calls
  `core::pushout::multi_pushout` to form $D +_U r$ (or the iterated colimit for a
  parallel family) and then merges labels from $D$ and the rules.
- **Recover history via [[reconstruction]].** The glued shape is handed to
  `core::reconstruct::reconstruct` to recover a paste tree; in dimensions $\le 3$
  the fast path `matching::assemble_low_dim_step` (internal) skips re-realisation
  and assembles the history directly (`reconstruct::build_tree`).
- **Driven by [[interactive-engine]].** `RewriteEngine` holds the session state
  and exposes `RewriteEngine::step` / `step_multi`; it enumerates candidates with
  `matching::for_each_rule_candidate` and confirms via
  `matching::confirm_candidate`, with parallel auto-stepping through
  `matching::greedy_parallel_auto_step`.

Behavioural evidence: `idem_whole_match` (two matches of `id id` in `id id id`),
`idem_chain_reaches_rhs` (two steps normalise to the target), `assoc_dim2_matches`
(2-dimensional matching), and `idem_parallel_in_four_chain` /
`greedy_parallel_in_four_chain` (parallel gluing of disjoint matches).

## Related

[[diagram]] · [[atom]] · [[boundary]] · [[partial-map]] · [[flow-graph]] ·
[[pushout]] · [[reconstruction]] · [[core-matching]] · [[interactive-engine]]
