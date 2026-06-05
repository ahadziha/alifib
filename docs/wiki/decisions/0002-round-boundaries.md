---
kind: decision
status: stable
last-touched: 2026-06-05
---

# 0002 — a cell is attached along a round shape

## Context

In Hadzihasanovic's theory of [[molecule|molecules]], an atom of dimension $n+1$
is the filler of a **round pair**: two parallel $n$-[[molecule|molecules]] — its
input and output boundaries — whose *shapes* together bound a directed sphere. A
[[diagram]] $U$ is *round* when its input and output boundaries $\partial^- U$ and
$\partial^+ U$ meet exactly in their common boundary $\partial\partial U$, so that
$U$ is a directed sphere; see [[boundary]]. Roundness is what lets a pair of
$n$-diagrams be the boundary of a single $(n{+}1)$-cell.

The subtlety this record exists to pin: **roundness is a condition on the
attaching *shape*, not on the realised, labelled boundary.**

## Decision

A cell `name : in -> out` exists only when the *shapes* of `in` and `out` are each
**round** and **parallel** (same dimension, agreeing on their common boundary).
This is a theory-mandated structural constraint, enforced when the cell is
constructed — and it looks only at the bare [[oriented-graded-poset]] shape, never
at the labels.

Because it is a shape condition, the **attachment may still identify cells**. The
canonical example is a point `pt` with a single arrow `a : pt -> pt`: the arrow is
attached along the round $0$-sphere of its two endpoints — a perfectly round
shape — while its labelling sends *both* endpoints to `pt`. The cell is well
formed; the resulting [[directed-complex|type]] (a point with a directed loop) is
not a regular complex, but it is a perfectly good directed complex. See
[[directed-complex]].

## Consequences

You cannot declare a generator whose input or output *shape* is non-round — a
1-diagram that is not a single directed path between two endpoints cannot be the
boundary of a 2-cell. Boundaries must be globular/round shapes, all the way up.
But you *can* declare a cell whose realised boundary identifies cells: loops,
endomorphisms, single-object structures are all legitimate, because roundness
never inspects the labelling. Pasting is unaffected either way — $\#_k$ checks
only boundary *agreement*, not roundness (see [[boundary]],
[[0001-no-identities]]).

## Code refs

Enforced in `Diagram::parallelism` *(src/core/diagram.rs)*, the gate of
`Diagram::cell_with_input_embedding` — the $(n{+}1)$-cell constructor. It rejects
a non-round input (*"first argument is not round"*) or output (*"second argument
is not round"*), then checks the two share a boundary shape and labels via
`boundary_traverse(Both, …)` (*"shapes of boundaries do not match"* /
*"boundaries do not match"*) before building the cell via the boundary `pushout`.
Roundness itself is `Diagram::is_round` → `Ogposet::is_round`
(`src/core/ogposet.rs`), the directed-sphere disjointness test — which operates on
the bare shape and ignores labels entirely. By contrast `Diagram::pastability`
(the $\#_k$ gate) does **not** call `is_round`. See [[core-diagram]] for the cell
constructor, [[boundary]] for $\partial^\pm$, [[directed-complex]] for the
shape-vs-realisation point, and [[pushout]] for the boundary gluing.
