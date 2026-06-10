---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Boundary

A pasting diagram of dimension $n$ runs **from** something **to** something:
a path of arrows runs from its first point to its last; a sheet of 2-cells
runs from one composite path to another. The "from" and "to" are its
boundaries, and more finely there is one pair per dimension: the **input
$k$-boundary** $\partial^-_k U$ and **output $k$-boundary** $\partial^+_k U$,
the $k$-dimensional shapes you would see entering and leaving if you
flattened $U$ down to dimension $k$. Boundaries are the joints along which
diagrams paste ($\#_k$ glues $\partial^+_k U$ to $\partial^-_k V$) and the
data compared whenever alifib asks "do these two diagrams meet?"

## How a boundary is computed

The definition is an algorithm, and it is worth internalising because the
code is a transcription of it. To compute $\partial^\alpha_k U$:

1. **Seed** with the $\alpha$-extremal $k$-cells
   ([[oriented-graded-poset|extremality]]): for $\alpha = -$, the $k$-cells
   that no higher cell *produces* — the ones the diagram needs handed to it;
   for $\alpha = +$, the ones nothing *consumes*.
2. **Close downward**: add every face (of either sign) of everything chosen,
   recursively.
3. **Adopt strays**: add the maximal cells of dimension $< k$, with their
   closures — parts of the diagram too low-dimensional to be seen by the
   extremality scan but still on the frontier.

The result is a closed sub-shape of dimension $\le k$ together with its
inclusion into $U$. Three conventions make it total: for $k \ge \dim U$ the
boundary is all of $U$; the empty diagram ($\dim = -1$) bounds nothing; and
the everyday *input/output* of an $n$-diagram means the codimension-one case
$\operatorname{in} U = \partial^-_{n-1} U$,
$\operatorname{out} U = \partial^+_{n-1} U$.

In the running example $\alpha : f \Rightarrow g \#_0 h$: the input
1-boundary seeds at $f$ and closes to $\{x, y, f\}$; the output 1-boundary
seeds at $\{g, h\}$ and closes to $\{x, m, y, g, h\}$. Both are
1-dimensional diagrams from $x$ to $y$, as they should be.

## Globularity — a theorem, not an axiom

For diagrams to behave like cells of a higher category, boundaries must
nest: the boundary of a boundary is the lower boundary,
$$
\partial^\alpha_j(\partial^\beta_k U) = \partial^\alpha_j U \qquad (j < k).
$$
On an *arbitrary* [[oriented-graded-poset]] this can fail — the seed-and-close
procedure above is defined regardless, and nothing forces coherence. The
book's Lemma 3.3.8 proves it holds for every [[molecule]]: globularity is a
*reward* for building shapes with the molecule constructors, not a property
of the substrate. This matters below, because the code's roundness check
silently assumes it.

## Roundness

A molecule can be the *input or output of a single higher cell* only if its
boundary closes up into a directed sphere — input hemisphere and output
hemisphere meeting exactly along their common rim. The book's definition
(3.2.5): $U$ is **round** if it is globular and, for every $n < \dim U$,
$$
\partial^-_n U \,\cap\, \partial^+_n U \;=\; \partial_{n-1} U .
$$
The two hemispheres at each level intersect in nothing more than the level
below. A path is round (two endpoint hemispheres meeting in nothing); the
2-diagram $g \#_0 h$ is round; but $O^2 \#_0 O^2$ — two 2-cells side by side
on a shared middle vertex — is not (the book's Example 3.2.10), which is why
roundness gates *cell construction* and not *pasting*: pastings do not in
general preserve it, the rewrite construction does (3.2.9).

Two consequences worth knowing: round implies pure (3.2.6), and the
boundaries of a round shape are round (3.2.7) — roundness propagates down,
which is what makes the recursive definition of [[atom|atoms]] bottom out.

**What the code checks instead.** `Ogposet::is_round` does not test the
equation above. It tests purity, then walks dimension by dimension checking
the *interiors* of the input and output boundaries are disjoint at every
level. For a *globular* shape these are equivalent: globularity gives
$\partial^\alpha_n U = \operatorname{int} \partial^\alpha_n U \sqcup
\partial_{n-1} U$, so the intersection equals $\partial_{n-1}U$ exactly when
the interiors are disjoint. The code never checks globularity — every shape
it feeds to `is_round` is a molecule, and molecules are globular by Lemma
3.3.8. On a non-molecule ogposet, `is_round` may answer wrongly; it is
correct precisely on the domain the construction discipline guarantees it is
called on. The same caveat covers its fast path: "pure with a single top
cell ⟹ round" is Corollary 3.3.11 (every atom is round), a fact about
molecules, not ogposets.

## Implementation

The shape-level algorithms are in `src/core/ogposet.rs`
([[core-ogposet]]); the diagram-level wrappers in `src/core/diagram.rs`
([[core-diagram]]).

- The seed-and-close algorithm is `ogposet::boundary` *(internal)*,
  literally steps 1–3: `extremal` for the seeds, a downward face walk, and
  the `maximal` adoption of strays. It returns the sub-ogposet plus its
  `Embedding` into the parent.
- `ogposet::boundary_traverse` *(internal)* returns the same boundary in
  canonical traversal order — use when the answer's cell ordering must be
  deterministic, e.g. for comparing boundaries by table equality. Its
  `Sign::Both` branch is special: it assembles the *whole boundary sphere*
  of a prospective cell (both hemispheres, input-first), the comparison
  object of [[atom]] construction.
- `Diagram::boundary(sign, k, d)` / `Diagram::boundary_normal` wrap these:
  clamp `k`, pull labels back along the embedding, trim the paste history
  (`boundary_history`, pinned by the test
  `boundary_normal_clamps_history_to_top_dim`).
  `Diagram::boundary_correspondence` *(internal)* re-locates an
  independently-computed boundary inside its parent.
- **Roundness** is `Ogposet::is_round`: `is_pure`, the single-top-cell fast
  path, then the layer walk (`build_layer` *(internal)*) accumulating input
  and output interiors level by level and testing disjointness — the
  globularity-conditional equivalent of Definition 3.2.5 explained above.
  `Diagram::is_round` delegates to it; labels are never consulted.
- Roundness is enforced in exactly one place: `Diagram::parallelism`
  *(internal)*, the gate of cell construction ([[atom]],
  [[0002-round-boundaries]]). Pasting's gate `Diagram::pastability` checks
  only $\partial^+_k U = \partial^-_k V$ in shape and labels.

The codimension-one reading is mirrored where generators are rendered:
`normalize::cell_from_diagram` *(internal)* takes `k = top_dim() - 1` and
extracts both sides — see [[output]].

## Related

[[oriented-graded-poset]] — where the signs live · [[molecule]] — whose
globularity (3.3.8) the roundness check assumes · [[atom]] — what roundness
gates · [[diagram]] · [[regular-directed-complex]] · [[directed-complex]] ·
[[rewriting]] · [[output]] · [[0002-round-boundaries]] ·
[[atom-gluing-sign-invariant]]
