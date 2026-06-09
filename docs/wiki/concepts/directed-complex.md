---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Directed complex

A **directed complex** is the shape of an alifib *type*: a directed cell
complex — cells of every dimension, each attached along its oriented boundary —
that need **not** be *regular*. The shapes of values, [[atom|atoms]] and
[[molecule|molecules]], are [[regular-directed-complex|regular directed
complexes]]: identification-free, the directed analogue of regular CW complexes
(Hadzihasanovic, *Combinatorics of higher-categorical diagrams*, 2024, 5.3.1).
A type relaxes exactly one thing: each cell is still *attached along* a round
regular shape, but the attachment may **identify** cells — the directed
analogue of passing from regular CW complexes to arbitrary CW complexes.

## Definition

A regular directed complex is a single [[oriented-graded-poset]]; an
identification breaks that. The canonical witness is a point `pt` with one
arrow `a : pt -> pt`: the realised directed loop is not an oriented graded
poset at all — the covering edge $pt \lessdot a$ would have to carry both
signs. So a non-regular directed complex is given not as a global poset but as
a **presentation**: a family of generators, each an [[atom]] attached along a
[[diagram]] whose **shape** is a round directed sphere
([[0002-round-boundaries]]) and whose **labelling** — the attachment — may send
distinct shape-cells to the same cell of the complex. The arrow `a` is attached
along the round $0$-sphere of its two endpoints; the labelling sends both to
`pt`. This is the theory's own device for diagrams with identifications:
shapes stay regular, only their images identify.

The two notions part company exactly here:

- A **regular directed complex** admits no identification: every cell's closure
  is an [[atom]], every closed cell embeds. Atoms and molecules are regular by
  construction.
- A **directed complex** permits it. The point with a directed loop is realised
  by a cell complex that is not regular — the closed $1$-cell does not embed —
  yet it is a perfectly good alifib type.

**Roundness is checked on the shape; the attachment may identify.** One fact,
two sides: the cell's, in [[0002-round-boundaries]]; the boundary's, in
[[boundary]].

### Why the distinction earns its keep

Insisting on regularity everywhere would forbid the loop `a : pt -> pt`, and
with it a one-object monoid, a term-rewriting system over a single sort, an
automaton with one state — the bread-and-butter types of the language. By
keeping the *shapes* of values regular while letting *types* be merely
directed, alifib keeps the combinatorial soundness of the theory where it pays
— [[rewriting|deciding when one diagram sits inside another]] — without paying
for it where it would only get in the way.

## Implementation

A type is a [[core-complex|`Complex`]]: a scoped table of generators, each
stored with a classifier [[diagram]]. Two layers realise the
shape-vs-attachment distinction.

- The **shape** of every diagram is an `Arc<Ogposet>`
  ([[oriented-graded-poset]], `src/core/ogposet.rs`); roundness is the
  bare-shape predicate `Ogposet::is_round`, which never inspects labels. It is
  enforced **only** at cell construction, by `Diagram::parallelism`
  (`src/core/diagram.rs`), the gate of the $(n{+}1)$-cell constructor
  `Diagram::cell_with_input_embedding` — see [[0002-round-boundaries]]. Pasting
  (`Diagram::pastability`) does *not* re-check it.
- The **attachment** is the labelling: a `Vec<Vec<Tag>>` laid over that shape.
  Two distinct shape-cells bearing the same `Tag` are exactly the
  identification that takes a regular shape to a non-regular type. In the
  classifier of `a : pt -> pt` the shape is the walking arrow — two distinct
  $0$-cells and a $1$-cell — and both $0$-cells carry `pt`'s tag.

The interpreter never needs a global "is this whole type regular?" pass:
roundness of the shapes it pastes is maintained cell-by-cell at construction,
and the assembled type is allowed to be a general directed complex. See
[[core-complex]] for generator/label storage and [[interpreter]] for how a
type's body is elaborated into one.

## Related

[[regular-directed-complex]] — the stricter notion the shapes satisfy ·
[[oriented-graded-poset]] — the bare substrate · [[atom]] · [[molecule]] ·
[[diagram]] · [[boundary]] · [[0002-round-boundaries]] · [[0001-no-identities]] ·
[[module-system]]
