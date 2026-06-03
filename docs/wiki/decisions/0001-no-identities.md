---
kind: decision
status: stable
last-touched: 2026-06-03
---

# 0001 — alifib has no identity cells

## Context

alifib follows Hadzihasanovic's theory of [[regular-directed-complex|regular
directed complexes]] / [[molecule|molecules]], which has **no identity cells**:
there is no degenerate "identity at $p$" for a 0-cell $p$, no identity 2-cell on a
1-cell, and so on. A [[diagram]] is built only from genuine generators. This is a
real divergence from strict $\omega$-categories (which have identities), and
alifib inherits it wholesale.

## Decision

No identity cells anywhere. Identities are simply not part of the data of a
diagram.

## What this does — and does not — entail

The one true consequence concerns *fillers*: you cannot use "the identity on $p$"
as the image of a [[partial-map|map]] or the filler of a [[hole]], because no such
cell exists. A refinement onto a coarse target must therefore spell out honest
"internal step" cells rather than absorbing detail into identities.

It is **not** true that "a $k$-cell cannot map to a lower-dimensional image."
That claim filled an earlier draft of this page; it does not follow from the
absence of identities, and it is wrong. The absence of identities constrains what
*diagrams contain*, not what *maps may do*. A 1-cell $s \to t$ whose endpoints
both map to a 0-cell $p$ collapses to $p$ itself — a genuine 0-dimensional
[[molecule]], not "the identity on $p$." Dimension-*lowering* maps are perfectly
legitimate, and **collapse inference** (`assign_cell` / `collapsed_boundary_image`,
see [[hole]] and [[core-partial-map]]) produces them deliberately.

## Code refs

The only dimension guard in `PartialMap::extend` *(src/core/partial_map.rs)* is
**no-*raising***: `if image.dim() > dim` ⇒ error *"image dimension exceeds source
dimension"* — a $k$-generator's image may not exceed dimension $k$. There is
deliberately **no** no-*lowering* guard; lowering is allowed (see above), so this
is correct, not a gap. Boundary compatibility is checked separately by
`check_boundary_match` *(internal)*: the map applied to each $\partial^\pm$ of the
source equals the corresponding `Diagram::boundary_normal` of the image.

The genuine *structural* constraint the theory enforces on cells — that a cell's
input and output boundaries are **round** — is a separate matter, recorded in
[[0002-round-boundaries]]; it has nothing to do with identities.
