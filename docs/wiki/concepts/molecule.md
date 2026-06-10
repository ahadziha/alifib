---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Molecule

Not every [[oriented-graded-poset]] is the shape of a sensible pasting
diagram — most are noise. Rather than characterise the sensible ones by a
property you could test, the book defines them by a *grammar*: a
**molecule** is anything you can build with three constructors (3.3.2), and
nothing else.

- **(Point).** The single point is a molecule.
- **(Paste).** If $U, V$ are molecules and the output $k$-boundary of $U$ is
  isomorphic to the input $k$-boundary of $V$ —
  $\partial^+_k U \cong \partial^-_k V$ — then gluing them along it (a
  pushout) gives the molecule $U \#_k V$. Boundary agreement is the *only*
  requirement; for $k \ge \min(\dim U, \dim V)$ the paste degenerates and
  one side absorbs the other (Lemma 3.3.7).
- **(Atom).** If $U, V$ are **round** molecules of the same dimension $n$
  with isomorphic boundaries — an isomorphism
  $\varphi : \partial U \cong \partial V$ that restricts to each sign,
  $\varphi^\alpha : \partial^\alpha U \cong \partial^\alpha V$ — then gluing
  $U$ to $V$ along $\varphi$ and adjoining one new $(n{+}1)$-cell on top,
  with input face $U$ and output face $V$, gives a molecule (the *rewrite
  construction* $U \Rightarrow V$, 3.2.1). This is the constructor that
  climbs dimensions; see [[atom]] for it in full.

So a molecule is an ogposet *with a derivation*: a certificate showing how it
was assembled. alifib takes this literally — a value carries its derivation
around at runtime (the paste history below).

## What the grammar buys

Each property below is a theorem proved by induction on the derivation, and
each is silently relied on somewhere in the code:

- **Globularity** (3.3.8): boundaries of boundaries are lower boundaries.
  Relied on by `Ogposet::is_round`, whose disjoint-interiors check is only
  equivalent to the book's roundness (3.2.5) on globular shapes — see
  [[boundary]].
- **Boundaries are molecules** (3.3.8): $\partial^\alpha_k U$ of a molecule
  is a molecule of dimension $\min(k, \dim U)$. This is why
  `Diagram::boundary` can return an ordinary diagram.
- **Connectedness** (3.3.13): a molecule is non-empty and connected. (Two
  disjoint points form a perfectly good [[regular-directed-complex]] but no
  molecule.)
- **Every cell's closure is an atom** (3.3.12): so every molecule is a
  [[regular-directed-complex]] (Remark 5.3.2). This is the bridge to
  Proposition 5.3.15, the theorem that makes alifib's `(shape, labels)`
  value representation faithful — the full story is in
  [[regular-directed-complex]].
- **Atoms are exactly the underivable-by-(Paste) molecules** (3.3.10): a
  molecule has a greatest element iff its final constructor was (Point) or
  (Atom).

Note what the grammar does *not* require: roundness gates (Atom) only,
never (Paste) — pasting two round things side by side usually destroys
roundness (the book's Example 3.2.10), and that is fine. And pasting is
**not composition**: $\#_k$ builds a *larger* shape; collapsing a pasting to
a single cell would be a higher-algebraic operation plain alifib types do
not have ([[diagram]]).

## Implementation

A molecule never appears bare at runtime; it appears as the shape of a
**`Diagram`** (`src/core/diagram.rs`, [[core-diagram]]): an `Arc<Ogposet>`
shape, a label per cell, and a **paste history** ([[core-paste-tree]]) — the
derivation certificate, kept as a tree of `PasteTree` nodes. The three
constructors are the only ways a `Diagram` is ever minted, and they map
one-to-one:

- **(Point)** — `Diagram::cell(tag, &CellData::Zero)`, via `cell0`
  *(internal)*.
- **(Atom)** — `Diagram::cell(tag, &CellData::Boundary { boundary_in,
  boundary_out })`, via `cell_with_input_embedding` *(internal)*. The gate is
  `Diagram::parallelism` *(internal)*: equal dimensions, both boundaries
  round in shape, boundaries equal in shape and labels. Whether this gate
  fully enforces the sign-restriction $\varphi^\alpha$ of (Atom) is the open
  question [[atom-gluing-sign-invariant]].
- **(Paste)** — `Diagram::paste(k, u, v)`, gated by `Diagram::pastability`
  *(internal)*: $\partial^+_k U = \partial^-_k V$ as canonical shapes with
  equal labels, nothing more. Its clamping of `k` by `top_dim` is exactly
  Lemma 3.3.7's degenerate cases.

There is no `is_molecule` check anywhere, for the same reason there is no
`is_regular` ([[regular-directed-complex]]): molecule-hood is maintained by
construction, not tested. The observable shadow of Lemma 3.3.10 is
`Diagram::is_cell` — the top input history is a single `PasteTree::Leaf`
exactly when the value was minted by `cell`, not assembled by `paste`.

Surface syntax: juxtaposition `U V` is *principal pasting*, elaborated by
`interpret_sequence_as_term` (`src/interpreter/diagram.rs`) as $\#_k$ at
$k = \min(\dim U, \dim V) - 1$, the highest dimension at which the two can
meet; explicit `#n` goes through the same `Diagram::paste`.

## Related

[[atom]] — the dimension-raising constructor · [[boundary]] — the gluing
joints and the globularity theorem · [[diagram]] — a molecule with labels,
i.e. a value · [[regular-directed-complex]] — what Lemma 3.3.12 makes every
molecule · [[directed-complex]] · [[oriented-graded-poset]] · [[rewriting]] ·
[[core-diagram]] · [[core-paste-tree]] · [[atom-gluing-sign-invariant]]
