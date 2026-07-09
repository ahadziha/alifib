---
kind: concept
status: stable
last-touched: 2026-07-10
---

# Diagram

When you write `let d = a (b #0 c)` in alifib, what kind of mathematical
object does `d` name? The quick answer: a **labelled [[molecule]]** — a
shape, every cell of which carries the tag of the generator it instantiates.
(A let-binding names a *whole* diagram; it never appears as a cell label.)
The precise answer is worth spelling out, because it corrects a natural
misconception about what alifib's values are.

## What a diagram is (and is not)

In the book, a *diagram of shape $U$ in a strict $\omega$-category $X$* is a
strict functor
$$
d : \mathsf{Mol}/U \to X
$$
(5.3.13), where $\mathsf{Mol}/U$ is the $\omega$-category of molecules
mapped into $U$; it is a **pasting diagram** when $U$ is itself a molecule
(5.3.16). Read it as: $d$ coherently assigns a cell of $X$ to *every
sub-molecule* of the shape, not merely to every cell.

Two consequences:

- **A diagram is not an RDC.** As an object of $X$ it is an arbitrary
  colimit of atoms, and the labelling may identify cells — the loop
  `a : pt -> pt` sends both endpoints of the walking arrow to the same
  point, and the glued object is not an oriented graded poset at all (its
  one covering edge would need both signs; see [[directed-complex]]). Only
  the *shape* $U$ is a [[regular-directed-complex]].
- **A diagram is more data than a labelling — in general.** A functor on all
  sub-molecules is a lot; a `Tag` per cell is very little. The bridge is the
  **combinatorial diagram** $\ell(d) : U \to X$ (5.3.14), the function
  sending each cell $x$ to $d$ of its closure — i.e. *the labelling* — and
  **Proposition 5.3.15**: when the shape is a regular directed complex,
  $\ell(d)$ determines $d$ uniquely. This is the theorem that licenses
  alifib to store a value as `(shape, labels)` and nothing more. It is
  *false* without regularity, which is why the shapes' regularity is
  load-bearing even though the values themselves are colimits — the full
  argument is in [[regular-directed-complex]].

A **type**, by contrast, is the same identification phenomenon writ large:
many generators glued through their labellings, realising in general only a
[[directed-complex]]. Labels collapse regular shapes into non-regular
colimits at both levels; what every diagram value keeps regular is its
shape.

## Boundaries and roundness

A diagram of top dimension $n$ has input and output $k$-boundaries
$\partial^\pm_k$ for every $k$ — themselves diagrams, computed on the shape
and restricted along the labelling (the algorithm is [[boundary]]; in the
book, $\partial^\alpha_n d := d|_{\partial^\alpha_n U}$, 5.3.17). The
codimension-one pair $\partial^\pm_{n-1}$ is *the* input and output. When
the two together form a directed sphere — interiors disjoint at every level —
the diagram is **round**, the precondition for serving as one side of a
single [[atom|cell]] one dimension up, and *not* a precondition for pasting.
Roundness is read off the bare shape; labels are never consulted.

## Pasting ($\#_k$)

Two diagrams paste along a shared $k$-boundary when output meets input in
both shape and labels:
$$
\partial^+_k U = \partial^-_k V
\quad\Longrightarrow\quad
U \#_k V .
$$
The result glues the two along that boundary (a pushout of shapes, labels
merged) — a *larger* diagram, never a single cell. **Pasting is not
composition**: reducing $U \#_k V$ to one cell would be a higher-algebraic
operation that plain alifib types do not have; do not read $\#_k$ as the
labelled analogue of categorical composition. Pasting is associative and
unital with the boundaries themselves acting as units — there are no
identity cells to do that job ([[0001-no-identities]]).

The surface juxtaposition `f g` is **principal pasting**: shorthand for
$f \#_k g$ at $k = \min(\dim f, \dim g) - 1$. An explicit `#n` is the general
$\#_n$. The principal dimension is *not* the highest $k$ at which these two
diagrams can be pasted — it is the highest $k$ at which the paste is
**non-trivial**. That distinction is easy to get wrong (this page once did),
so the next section works it out.

### Above the principal dimension: units, not errors

Take $f : a \to b$ and a 2-cell $\alpha : f \Rightarrow g$. Their principal
dimension is $\min(1, 2) - 1 = 0$, so what is `f #1 alpha` — out of range for
the 1-dimensional $f$? No. Boundaries clamp: for $k \geq \dim U$ the
$k$-boundary of $U$ is $U$ itself, so the pastability condition
$\partial^+_1 f = \partial^-_1 \alpha$ becomes $f = \partial^-_1 \alpha$. The
paste above the principal dimension is therefore *defined*, but only when the
lower-dimensional diagram literally **is** the other's $k$-input (in shape
*and* labels). And when it is, the pushout glues $\alpha$ to a subdiagram
$\alpha$ already contains, so the result is $\alpha$ unchanged:
$f \#_1 \alpha = \alpha$. The smaller diagram acts as a **boundary-unit** —
this is the unitality above made concrete, and it is exactly why alifib needs
no identity cells: boundaries themselves do the unit's job
([[0001-no-identities]]). Pushing further, at $k \geq$ both dimensions the
condition degenerates to $f = g$ and $f \#_k f = f$.

So the picture across $k$: below the principal dimension, pastes genuinely
combine (two 2-cells at $\#_0$ lie side by side); at it, they stack; at
$\min(\dim f, \dim g)$ and above, a defined paste only ever returns the
larger diagram, absorbing the smaller as a unit.

In code the clamp is one expression: `Diagram::pastability`
(`src/core/diagram.rs`, *internal*) computes both boundaries at
`k.min(top_dim())`, so an over-large `k` is never rejected as out of range —
a paste fails only the usual way, when the clamped boundaries disagree
(*"boundaries do not match"*). The boundary-side clamp is pinned by the test
`boundary_normal_clamps_history_to_top_dim`; the degenerate pastes are the
book's Lemma 3.3.7 cases (see [[molecule]]).

## Atoms as cells

A single generating cell is a diagram whose shape has a *greatest element* —
an [[atom]]. An $n$-generator `a : U -> V` is determined by two round,
parallel $(n{-}1)$-diagrams; gluing them along their shared boundary sphere
and capping with one new top cell yields the generator's **classifier**
diagram. The construction, and the one open soundness question it carries,
are in [[atom]] and [[atom-gluing-sign-invariant]].

## Implementation

`Diagram` in `src/core/diagram.rs` — see [[core-diagram]]. The triple is
literal:

- `shape: Arc<Ogposet>` — the molecule ([[oriented-graded-poset]]);
- `labels: Vec<Vec<Tag>>` — the combinatorial diagram $\ell(d)$, one tag per
  cell, indexed `[dim][pos]`;
- `paste_history` — the derivation certificate ([[core-paste-tree]])
  recording the $\#_k$ tree that built it.

Operations:

- **Atoms** — `Diagram::cell(tag, &CellData)`: `Zero` for a point,
  `Boundary { boundary_in, boundary_out }` for an $n$-cell, gated by
  `Diagram::parallelism` *(internal)*.
- **Pasting** — `Diagram::paste(k, u, v)`, gated by `Diagram::pastability`
  *(internal)*: boundary agreement in shape and labels, no roundness.
- **Boundaries** — `Diagram::boundary(Sign, k, &d)` and `boundary_normal`;
  `Sign::Input` is $\partial^-$, `Sign::Output` is $\partial^+$
  ([[boundary]]).
- **Roundness** — `Diagram::is_round` → `Ogposet::is_round`; **top
  dimension** — `Diagram::top_dim` (`dim()` is $-1$ for the empty diagram).
- **Surface syntax** — juxtaposition parses to
  `ast::Diagram::PrincipalPaste`, explicit `#n` to `ast::Diagram::Paste`
  (`src/language/parser.rs`); both interpret through the same paste in
  `src/interpreter/diagram.rs`, where the principal dimension is
  `prev.top_dim().min(d_right.top_dim()).checked_sub(1)` — pasting below
  dimension 0 is an error, not a fallback.

Diagrams are stored in a [[core-complex|Complex]] as generator classifiers
and let-bound values. Rewriting builds new diagrams through
`matching::construct_parallel_step` → `pushout::multi_pushout` — see
[[rewriting]].

## Related

[[molecule]] — the shape · [[atom]] — the generating cells · [[boundary]] ·
[[regular-directed-complex]] — why `(shape, labels)` is a faithful
representation · [[directed-complex]] — what the labelled colimits assemble
to · [[oriented-graded-poset]] · [[rewriting]] ·
[[atom-gluing-sign-invariant]]
