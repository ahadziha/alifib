---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Directed complex

A **directed complex** is the shape of an alifib *type*: a directed cell
complex — cells of every dimension, each attached along its oriented
boundary — that need **not** be regular. The shapes of *values* are
[[regular-directed-complex|regular directed complexes]]:
identification-free, the directed analogue of regular CW complexes. A type
relaxes exactly one thing: each cell is still attached *along* a round
regular shape, but the attachment — the labelling — may **identify** cells.
This is the directed analogue of passing from regular CW complexes to
arbitrary ones, where attaching maps stop being embeddings.

## Why identification breaks the poset

It is worth seeing concretely why a type cannot, in general, be stored the
way a value is — as one [[oriented-graded-poset]].

Declare a point and a loop on it:

```
pt : *
a  : pt -> pt
```

The classifier of `a` has a perfectly regular shape: the walking arrow, two
distinct 0-cells and one 1-cell. The *labelling* sends both 0-cells to
`pt`. Now try to realise the type itself as a single poset: it has two
elements, $pt < a$, hence exactly one covering edge $pt \lessdot a$ — and
that edge would have to be labelled $-$ (because `pt` is the input face of
`a`) *and* $+$ (because it is also the output face). But an orientation
assigns each Hasse edge **one** sign (2.1.1). The realised loop is not a
non-regular oriented graded poset; it is not an oriented graded poset at
all.

So a non-regular directed complex is kept not as a global poset but as a
**presentation**: a family of generators, each an [[atom]] attached along a
[[diagram]] whose *shape* is a round directed sphere
([[0002-round-boundaries]]) and whose *labelling* may send distinct
shape-cells to the same cell of the complex. The arrow `a` is attached along
the round 0-sphere of its two endpoints; the labelling folds both onto
`pt`. Shapes stay regular; only their images identify. This is the theory's
own device for diagrams with identifications, and it is the same mechanism
at every scale — a single value with a repeated label is a small colimit,
a type is a large one ([[diagram]]).

The two notions part company exactly here:

- A **regular directed complex** admits no identification: every cell's
  closure is an [[atom]], every closed cell embeds, and the whole thing *is*
  one oriented graded poset.
- A **directed complex** permits identification, exists in general only as a
  presentation, and is a perfectly good alifib type.

### Why the distinction earns its keep

Insisting on regularity everywhere would forbid the loop `a : pt -> pt` —
and with it a one-object monoid, a term-rewriting system over a single sort,
an automaton with one state: the bread-and-butter types of the language.
Keeping the *shapes* regular while letting *types* be merely directed buys
the combinatorial soundness where it pays — decidable shape equality,
faithful `(shape, labels)` values via Proposition 5.3.15
([[regular-directed-complex]]), [[rewriting|matching]] — without paying for
it where it would only get in the way.

## Implementation

A type is a [[core-complex|`Complex`]]: a scoped table of generators, each
stored with its classifier [[diagram]]. The presentation-not-poset point is
visible in the data layout — there is no "type ogposet" anywhere; the
`Complex` holds one regular shape *per generator* and lets the labels do the
identifying.

- The **shape** of every classifier is an `Arc<Ogposet>`
  ([[oriented-graded-poset]], `src/core/ogposet.rs`); roundness of declared
  boundaries is the bare-shape predicate `Ogposet::is_round`, enforced only
  at cell construction by `Diagram::parallelism` *(internal)* — the gate of
  `Diagram::cell_with_input_embedding` (`src/core/diagram.rs`); see
  [[atom]]. Pasting (`Diagram::pastability`) never re-checks it.
- The **attachment** is the labelling, a `Vec<Vec<Tag>>` over that shape.
  Two distinct shape-cells bearing the same `Tag` is exactly the
  identification that takes a regular shape to a non-regular type. In the
  classifier of `a : pt -> pt`, the shape is the walking arrow and both
  0-cells carry `pt`'s tag.

The interpreter never runs a global "is this type regular?" pass — there is
nothing it would run it *on*, since the type is never realised as one
poset. Regularity is maintained shape-by-shape at the two construction
gates ([[regular-directed-complex]]), and the assembled type is allowed to
be a general directed complex. See [[core-complex]] for generator and label
storage, [[interpreter]] for how a type's body elaborates into one.

## Related

[[regular-directed-complex]] — the regular case, and why shape-regularity
still matters · [[diagram]] — the labelling mechanism, one value at a time ·
[[atom]] · [[molecule]] · [[boundary]] · [[oriented-graded-poset]] ·
[[0002-round-boundaries]] · [[0001-no-identities]] · [[module-system]]
