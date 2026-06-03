---
kind: decision
status: stable
last-touched: 2026-06-03
---

# 0002 — a cell's boundaries must be round

## Context

In Hadzihasanovic's theory of [[regular-directed-complex|regular directed
complexes]] / [[molecule|molecules]], an atom of dimension $n+1$ is the filler of
a **round pair**: two parallel $n$-[[molecule|molecules]] — its input and output
boundaries — that together bound a directed sphere. A [[diagram]] $U$ is *round*
when its input and output boundaries $\partial^- U$ and $\partial^+ U$ meet
exactly in their common boundary $\partial\partial U$, so that $U$ is a directed
sphere; see [[boundary]]. Roundness is precisely what allows a pair of
$n$-diagrams to be the boundary of a single $(n{+}1)$-cell.

## Decision

A cell `name : src -> tgt` exists only when `src` and `tgt` are each **round** and
**parallel** (same dimension, agreeing on their common boundary). This is a
genuine, theory-mandated structural constraint — and, unlike the fabricated
dimension-lowering rule once attributed to [[0001-no-identities]], it really is
enforced.

## Consequences

You cannot declare a generator whose source or target is a non-round diagram. For
instance, a 1-diagram that is not a single directed path between two endpoints is
not round and cannot be the boundary of a 2-cell. Boundaries must be
globular/round, all the way up.

## Code refs

Enforced in `Diagram::parallelism` *(src/core/diagram.rs)*, called by
`Diagram::cell_with_input_embedding` — the $(n{+}1)$-cell constructor. It rejects
a non-round input (*"first argument is not round"*) or output (*"second argument
is not round"*), then checks the two share a boundary shape and labels
(*"shapes of boundaries do not match"* / *"boundaries do not match"*) before
building the cell via the boundary `pushout`. Roundness itself is
`Diagram::is_round` → `Ogposet::is_round`, the directed-sphere disjointness test
(`src/core/ogposet.rs`). See [[core-diagram]] for the cell constructor,
[[boundary]] for $\partial^\pm$, and [[pushout]] for the boundary gluing.
