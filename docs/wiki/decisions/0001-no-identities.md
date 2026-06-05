---
kind: decision
status: stable
last-touched: 2026-06-05
---

# 0001 — no identity cells (but composition is unital)

## Context

alifib follows Hadzihasanovic's theory of [[molecule|molecules]] — whose shapes
are [[regular-directed-complex|regular directed complexes]], assembled into types
that are the looser [[directed-complex|directed complexes]]. What the theory
*lacks* is more precise than the slogan "no identities" suggests, and worth
stating carefully.

## What is true

There is **no representation of an $n$-cell as an $(n{+}1)$-dimensional cell**.
A type has no degenerate "identity cell": no $(n{+}1)$-generator standing for the
identity on an $n$-diagram, no identity 2-cell on a 1-cell, no identity 1-cell on
a 0-cell. Cells are only genuine generators and the [[diagram|diagrams]] they
build.

## Composition is still unital

This is **not** the absence of units. *Pasting has units.* For a [[diagram]] $U$
and any $k < \dim U$, pasting $U$ at dimension $k$ with its own $k$-dimensional
input or output [[boundary]] returns a diagram isomorphic to $U$:
$$
\partial^-_k U \;\#_k\; U \;\cong\; U \;\cong\; U \;\#_k\; \partial^+_k U .
$$
So $\partial^-_k U$ is a left unit and $\partial^+_k U$ a right unit for $\#_k$;
composition — and hence [[rewriting]] — is unital. The units are *boundaries*, not
cells, which is exactly why they need no identity-cell to exist.

A direct consequence: **a zero-step rewriting proof is a valid (identity) proof.**
It is the unit of $\#_n$ at the initial $n$-diagram, represented simply by that
$n$-diagram (what `proof` / `stored_expr` render), not by a degenerate
$(n{+}1)$-cell. `target_reached` is therefore just $current \cong target$, true at
step $0$ too — an initial diagram already equal to the target is already proved.

## Consequence: represent lower-dimensional structure explicitly

Combined with [[0002-round-boundaries]] (a cell's boundaries must be round and of
the appropriate dimension), the absence of identity cells means that to model an
$(n{+}1)$-cell whose input or output is *morally* of dimension $< n$ you must
introduce an **explicit representation** — there is no degenerate filler to lean
on.

The worked example is TRS constants (`examples/TRS.ali`). A constant is morally a
nullary operation, with no input. It is modelled **not** as a 2-cell with an empty
input boundary (which could not be round), but as a 2-cell `node : unit -> cod`
whose input is an explicit **unit 1-cell** `unit = Unit.wire`. The `Unit` type
correspondingly introduces explicit **unitor** 2-cells — `lunit : unit wire ->
wire`, `runit : wire unit -> wire`, and their inverses — to move that unit around,
precisely because the pasting units are not themselves cells one can name or
rewrite with.

## Maps and dimension

Since there is no identity cell to send a collapsed cell *to*, a
[[partial-map|map]] that collapses a $k$-cell sends it to the genuine
lower-dimensional image. Dimension-*lowering* maps are thus legitimate, and
collapse inference produces them on purpose (see [[hole]], [[core-partial-map]]).

## Code refs

- `PartialMap::extend` *(src/core/partial_map.rs)* guards only dimension-*raising*
  (`image.dim() > dim`); there is no lower-bound guard, by design.
- The zero-step proof: `RewriteEngine::target_reached` is `current ≅ target` with
  no step-count gate; `stored_expr` renders the initial diagram for a zero-step
  session (`src/interactive/{engine,session}.rs`; see [[interactive-engine]]).
- Round boundaries are the genuine structural constraint — [[0002-round-boundaries]].
