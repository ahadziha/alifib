---
kind: concept
status: stable
last-touched: 2026-06-05
---

# Directed complex

A **directed complex** is the shape of an alifib *type*: a finite
[[oriented-graded-poset]], realised as a directed cell complex, every cell of
which carries a partition of its boundary into an *input* half and an *output*
half. It is the most general shape the language builds with — and, crucially, it
need **not** be *regular*. The shapes of *values* — the [[atom|atoms]] and
[[molecule|molecules]] a type's diagrams are pasted from — are the stricter
[[regular-directed-complex|regular directed complexes]]; the *type* that holds
them is a directed complex, regular only by accident.

## Definition

Following Hadzihasanovic (*Combinatorics of higher-categorical diagrams*, 2024),
a directed complex is an [[oriented-graded-poset]] that presents an honest
*directed cell complex*: a space assembled from directed cells of every
dimension, each glued to those below it along its oriented boundary. What
separates this from the older notion of *polygraph* is **topological soundness**
— the combinatorics never drift away from the geometry they describe. But
soundness is secured by a condition on *shapes*, not on the complex as realised.

The condition is **roundness**. Each cell is attached along a [[diagram]] whose
**shape** is a directed sphere — its [[boundary|input and output]] halves each a
ball (see [[0002-round-boundaries]]). That is a property of the *attaching shape*
alone. The *attachment* — the labelling that places the cell in a type — is then
free to **identify parts of that shape**. A single point `pt` with one arrow
`a : pt -> pt` on it is a perfectly good cell: the arrow is attached along the
round $0$-sphere of its two endpoints, and the attachment sends both endpoints to
the same point.

This is exactly where *regular* and merely *directed* part company.

- A **regular directed complex** is one in which no such identification occurs:
  every cell's closure is an [[atom]], realised by an honest CW-cell that embeds.
  [[atom|Atoms]] and [[molecule|molecules]] are regular by construction.
- A **directed complex** permits the identifications. The type `Letters` above —
  a point with a directed loop — is realised by a cell complex that is *not*
  regular (the closed $1$-cell does not embed; its two endpoints coincide), yet
  it is a perfectly good directed complex, and a perfectly good alifib type.

So a type is a directed complex; its values are diagrams whose *shapes* are
regular molecules. The two notions are not in tension — they sit at different
layers. **Roundness is checked on the shape; the realisation may identify.** This
is one fact seen from two sides: from the cell's, in [[0002-round-boundaries]];
from the boundary's, in [[boundary]].

### Why the distinction earns its keep

Insisting on regularity everywhere would forbid the loop `a : pt -> pt`, and with
it most of what makes alifib expressive: a one-object monoid, a term-rewriting
system over a single sort, an automaton with one state. These are the
bread-and-butter types of the language, and each is a non-regular directed
complex. By taking the *shapes* of values to be regular while letting *types* be
merely directed, alifib keeps the combinatorial soundness of the theory where it
pays — in [[rewriting|deciding when one diagram sits inside another]] — without
paying for it where it would only get in the way.

## Implementation

A type is a [[core-complex|`Complex`]]: a scoped table of generators, each stored
with a classifier [[diagram]]. Two layers realise the shape-vs-realisation
distinction.

- The **shape** of every diagram is an `Arc<Ogposet>` ([[oriented-graded-poset]],
  `src/core/ogposet.rs`); roundness is the bare-shape predicate
  `Ogposet::is_round`, which never inspects labels. It is enforced **only** at
  cell construction, by `Diagram::parallelism` (`src/core/diagram.rs`), the gate
  of the $(n{+}1)$-cell constructor `cell_with_input_embedding` — see
  [[0002-round-boundaries]]. Pasting (`Diagram::pastability`) does *not* re-check
  it.
- The **realisation** is the labelling: a `Vec<Vec<Tag>>` laid over that shape.
  Two distinct shape-cells bearing the same `Tag` are exactly the identification
  that takes a regular shape to a non-regular type. In the classifier of
  `a : pt -> pt` the shape is the walking arrow — two distinct $0$-cells and a
  $1$-cell — and both $0$-cells carry `pt`'s tag.

The interpreter never needs a global "is this whole type regular?" pass:
roundness of the *shapes* it pastes is maintained cell-by-cell at construction,
and the assembled type is allowed to be a general directed complex. See
[[core-complex]] for the generator/label storage and [[interpreter]] for how a
type's body is elaborated into one.

## Related

[[regular-directed-complex]] — the stricter notion the shapes satisfy ·
[[oriented-graded-poset]] — the bare substrate · [[atom]] · [[molecule]] ·
[[diagram]] · [[boundary]] · [[0002-round-boundaries]] · [[0001-no-identities]] ·
[[module-system]]
