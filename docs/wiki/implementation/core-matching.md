---
kind: impl
status: stable
last-touched: 2026-06-05
code: [src/core/matching.rs, src/core/embeddings.rs, src/core/pushout.rs, src/core/flow.rs, src/core/reconstruct.rs]
---

# core-matching — the rewriting engine

> Five modules conspire to turn a rule and a target into a step. `matching`
> finds where a rule fits, `embeddings` records the fit, `flow` is the lens that
> makes the search tractable, `pushout` glues the rewrite in, and `reconstruct`
> recovers a well-formed [[diagram]] from raw ogposet-and-labels.

This is the heart of [[rewriting]]. A rewrite rule is a generator of dimension
$n+1$ whose input $n$-boundary is the pattern $U$ and whose output is the
replacement. To *apply* it inside a target diagram $V$ (also of top dimension
$n$) is to: locate a copy of $U$ in $V$, glue the rule's $(n+1)$-cell on top, and
read off the resulting $(n+1)$-step. The new diagram's output boundary is $V$
with $U$ replaced.

## The five modules

| Module | Responsibility |
|---|---|
| `embeddings.rs` | `Embedding` — an injective, dimension-preserving map of ogposets, stored with its partial inverse |
| `flow.rs` | the $k$-flow graph $\mathbf{F}_k(U)$ — reduces subdiagram matching to labelled subgraph matching |
| `matching.rs` | the orchestrator: find candidates, confirm them, assemble (parallel) steps |
| `pushout.rs` | `multi_pushout` — the ogposet-level colimit that glues rule(s) onto the target |
| `reconstruct.rs` | recover a `Diagram` (paste history) from a bare ogposet + labels |

## Data flow of one rewrite step

```
target V ──flow_graph──▶ TargetFlowData (built once, reused per rule)
rule (n+1)-cell ──RulePattern::new──▶ pattern U + pattern_to_rewrite

           find_path_induced_matches          ← labelled subgraph match
                     │  vertex matches
                     ▼
           check_match_isomorphism            ← closure, restrict, find_isomorphism
                     │  iso_emb : U ↪ V        (an Embedding)
                     ▼  CandidateMatch
           construct_parallel_step
                     │
        ┌────────────┴─────────────┐
   multi_pushout(V.shape, spans)   merge labels (target ∪ rewrites)
                     │  tip ogposet + labels
                     ▼
        dim ≤ 3 ? assemble_low_dim_step : reconstruct::reconstruct
                     │
                     ▼
              MatchResult { step : Diagram, ... }
```

### 1. Precompute the pattern — `RulePattern`

`RulePattern::new(rewrite, backward)` slices the rule's classifier once and keeps
the result for the lifetime of a session (rebuilding boundaries every step was a
hot spot). It records:

- `pattern` — the normalised $n$-boundary: $\partial^-_n$ of the rule when
  forward, $\partial^+_n$ when `backward` (a [[boundary]] taken via
  `Diagram::boundary_normal`).
- `pattern_to_rewrite` — the [[partial-map|embedding]] of the pattern's shape
  into the rule's full $(n+1)$ shape, via `ogposet::boundary_traverse`. This is
  the *right injection* of the pushout that builds the step.
- `backward` — direction flag.

Patterns for every rule of the right dimension are built up front by
`build_rule_patterns`.

### 2. Find candidates — flow + subgraph matching

The expensive insight: a subdiagram of $V$ is determined by which top-cells it
covers, and adjacency of top-cells is captured by the **flow graph**
($\mathbf{F}_{n-1}$). `flow::flow_graph(shape, k)` (Definition 61 of
Hadzihasanovic–Kessler) builds a `DiGraph` whose nodes are all cells of
dimension $> k$ and whose edges record $\Delta^+_k(x) \cap \Delta^-_k(y) \neq
\varnothing$ — i.e. the output $k$-boundary of $x$ meets the input $k$-boundary
of $y$. So matching $U$ inside $V$ becomes a **labelled, path-induced subgraph
isomorphism** of $\mathbf{F}(U)$ into $\mathbf{F}(V)$.

`TargetFlowData::new(target)` builds $\mathbf{F}(V)$ once and is reused across
*all* rules. The production entry point is `for_each_rule_candidate`, which builds
the target flow data once and then drives `for_each_candidate_in_rule` over every
rule of the right dimension; that inner loop calls `find_path_induced_matches` to
do the backtracking subgraph search: label-filtered candidate sets,
most-constrained-variable ordering, and the path-induced constraint (edge in
pattern iff edge in target). Each surviving vertex match yields a tentative set
of `image_positions` (the top-cells it covers). dim-0 targets are special-cased
in `for_each_candidate_dim0`. (`find_matches` is a `#[cfg(test)]`-only helper that
walks this same path for a single rule; production never calls it.)

### 3. Confirm the match — `check_match_isomorphism`

A flow match is only a necessary condition. `check_match_isomorphism` takes the
matched cells, computes their `ogposet::closure` in $V$, checks cell-count
equality against the pattern, restricts to the sub-ogposet
(`reconstruct::restrict_ogposet`), runs `ogposet::find_isomorphism`, and
composes the maps while checking that **labels** agree at every cell. Success
yields the confirmed $\text{iso\_emb} : U \hookrightarrow V$ as an `Embedding`,
wrapped in a `CandidateMatch`.

### 4. Build the step — `construct_parallel_step` + `multi_pushout`

For a (possibly parallel) family of confirmed matches, each match becomes a
`pushout::Span { into_base: iso_emb, into_ext: pattern_to_rewrite }`: a span
$V \xleftarrow{} U \xrightarrow{} \text{rule}$. `pushout::multi_pushout`
computes the colimit — gluing every rule's $(n+1)$-cell onto $V$ along its
matched copy of $U$ simultaneously — returning the `tip` ogposet, the base
injection `inl: V ↪ tip`, and one `inr_i` per rule. Labels are then merged:
base cells keep $V$'s tags; fresh cells pull their tag from the rule that
introduced them (via `inr.inv`).

`multi_pushout` is the genuine workhorse of `pushout.rs`; the binary `pushout`
is just a wrapper that picks the larger codomain as base to minimise new cells.
Shared cells (those with a preimage under `into_ext`) are identified with their
image in the base; un-shared cells become fresh, and their faces/cofaces are
remapped through the running `inr_map`.

### 5. Recover a diagram — `reconstruct`

`multi_pushout` yields an ogposet and labels, but a [[diagram]] also carries a
**paste history** (`BoundaryHistory` / `PasteTree`). Two paths:

- **dim ≤ 3** (`assemble_low_dim_step`): the candidate paste tree is provably
  valid here, so realisation is skipped. `reconstruct::build_tree` builds the
  tree; the new $k$-boundary is derived by substituting each rule's top cell with
  its output (or input, if backward) boundary tree. This is the ~4.5× speedup
  noted in the commit log.
- **dim > 3** (`reconstruct::reconstruct`): build a paste tree, `realise_tree`
  it, and `check_sizes` against the pre-diagram.

`reconstruct` itself is a recursive layering algorithm. It picks a
*decomposition dimension* $k$ (the frame dimension for $\dim \le 3$, else the
layering dimension), builds the **maximal** flow graph $\mathbf{M}_k$
(`flow::maximal_flow_graph` — $\mathbf{F}_k$ restricted to cells with no
cofaces), topologically sorts it, and slices the diagram into layers:
$\text{layer}_1 = \partial^-_k(U) \cup \mathord\downarrow x_1$, then each layer
chains off the previous layer's output $k$-boundary. Each layer recurses. For
$\dim > 3$ it tries topological sorts lazily until one realises correctly
(`graph::try_topological_sorts`, capped at 10 000).

## Key public types

- `Embedding { dom, cod, map, inv }` (`embeddings.rs`) — injective ogposet map,
  `inv[d][j] = NO_PREIMAGE` (`usize::MAX`) when cell $j$ is not in the image.
  `id`, `empty`, `make` constructors.
- `RulePattern { pattern, pattern_to_rewrite, backward }` — precomputed rule side.
- `CandidateMatch { rule_name, image_positions, iso_emb }` — a confirmed
  isomorphism, no step yet (crate-private).
- `MatchResult { step, members, image_positions }` — a confirmed rewrite; `step`
  is the $(n+1)$-diagram. `members: Vec<FamilyMember>` — one per constituent
  match (singleton for an ordinary rewrite).
- `pushout::{Span, MultiPushout, Pushout}` — all `pub(super)`; the colimit data.

## Parallel rewriting

A *compatible family* is a set of matches with pairwise-disjoint
`image_positions` (an independent set in the conflict graph).

The **live** strategy is **greedy**: `greedy_parallel_auto_step` →
`build_greedy_family` does one pass per rule, accreting disjoint matches into an
`occupied` set; `try_or_shrink` verifies the family and peels back one member at
a time on failure. This is the only path production reaches — the interactive
engine calls it from `RewriteEngine::auto` when `parallel` is set
(`src/interactive/engine.rs`) and so does the interpreter's auto-normalisation
(`src/interpreter/diagram.rs`).

> **Deterministic exhaustive alternative (retained, not dead):**
> `find_compatible_families` (with helpers `max_independent_set_size`,
> `max_is_dfs`, `enumerate_independent_sets_of_size`) solves a *different*
> problem from the greedy live path: it enumerates **all** maximal compatible
> families — maximal independent sets in the conflict graph, in size-descending
> order — verifying each by constructing the step and pruning dominated
> sub-families. Family enumeration is worst-case exponential in the number of
> matches, so it is deliberately kept out of the interactive engine's hot path
> and lives here as a backend capability. It is `pub(crate)` behind
> `#[allow(dead_code)]` with a documented retention rationale on the function
> itself; its only callers today are the tests `idem_parallel_in_four_chain` and
> `idem_no_parallel_in_three_chain`. The `#[allow(dead_code)]` stays until a tool
> (not the auto-step loop) wires it. This is intentional retention, not rot.

Every confirmation — singleton or family — funnels through `try_family` →
`construct_parallel_step`, so individual and parallel rewrites share one code
path (`confirm_candidate` is just the singleton case, via `try_family` on a
one-element slice).

## Non-obvious invariants and gotchas

- **`TargetFlowData` once, rules many.** The flow graph of the *target* is the
  per-step cost; it is built once and shared. The pattern's flow graph is small
  and rebuilt per rule inside `for_each_candidate_in_rule`.
- **Path-induced, not just monotone.** Subgraph matching requires edges to agree
  in *both* directions — an edge exists in the pattern iff it exists in the
  target image. A merely injective label-respecting map is not enough.
- **Flow match ⇒ candidate, not match.** The isomorphism + label check in
  `check_match_isomorphism` is the real gate. The flow stage only prunes.
- **Backward rewriting** swaps which boundary is the pattern (`Sign::Output`
  instead of `Sign::Input`) and which boundary tree is substituted in the low-dim
  path.
- **dim ≤ 3 trusts the tree.** `assemble_low_dim_step` deliberately skips
  `realise_tree` + `check_sizes`. If that fast path ever produces a malformed
  diagram, suspect this shortcut first.
- **`NO_PREIMAGE` is `usize::MAX`** and is used as an in-band sentinel across
  `embeddings`, `pushout`, and `reconstruct` — never index with it.

## Mathematics

This cluster realises [[rewriting]] inside a type — a [[directed-complex]] of
generators — whose shapes (atoms, molecules, diagrams) are
[[regular-directed-complex|regular]]. The matching map and pushout injections are
[[partial-map|partial/total maps]] of [[oriented-graded-poset|oriented graded
posets]]. A rule's pattern is the $n$-[[boundary]] of an [[atom]]; the target is
a [[molecule]]/[[diagram]]. The pushout glues (pastes) the rule's cell onto the
target — it builds a *larger* diagram, not a composite reduced to one cell. The
flow graph $\mathbf{F}_k$ and the layering used in reconstruction come from
Hadzihasanovic–Kessler (Definition 61). See
[[core-diagram]] for `Diagram`/`BoundaryHistory`, [[core-paste-tree]] for
`PasteTree`/`realise_tree`, [[core-complex]] for `classifier` and generator
lookup, and [[interactive-engine]] for how steps are driven in a session.
