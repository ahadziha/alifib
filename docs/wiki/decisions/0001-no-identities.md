---
kind: decision
status: stable
last-touched: 2026-06-03
---

# 0001 — alifib has no identity cells

## Context

alifib follows Hadzihasanovic's theory of [[regular-directed-complex|regular
directed complexes]] / [[molecule|molecules]], which does **not** include
identity cells. This is a deliberate divergence from strict $\omega$-categories
(which have identities).

## Decision

No identity cells anywhere. There is no "identity at $p$" for a 0-cell $p$, no
identity 2-cell on a 1-cell, and so on.

## Consequences

For [[partial-map|partial maps]] (`let total Name :: Source = [ … ]`,
`attach … along [ … ]`):

- A 1-cell in the source **cannot** map to a 0-cell of the target — there is no
  identity to map it to. Likewise a 2-cell can't map to a 1-cell, and so on up.
  (This is the *intended* discipline; the current implementation enforces it only
  partially — see the warning under [Code refs](#code-refs).)
- A refinement map from a richly structured complex to a coarse `Spec` needs an
  honest 1-cell-or-longer-path target for **every** source 1-cell. So `Spec` must
  include explicit "internal step" cells if you want to absorb implementation
  detail — you cannot collapse internal protocol to identities the way you would
  in a strict $\omega$-category.

## Code refs

The map-level discipline lives in `PartialMap::extend`
*(src/core/partial_map.rs)*, which gates every clause of a `let total` /
`attach … along`. It does two things relevant here:

- **Rejects dimension-*raising*.** `if image.dim() > dim` ⇒ error
  *"image dimension exceeds source dimension"*. A $k$-cell cannot map to anything
  of dimension $> k$.
- **Checks boundary compatibility** via `check_boundary_match` *(internal)*: the
  map applied to each $\partial^\pm$ of the source cell must equal the
  corresponding `Diagram::boundary_normal` of the image (compared under
  `Diagram::normal` / `Diagram::equal`).

> [!warning] The no-dimension-*lowering* half is **not** enforced today.
> The decision says a 1-cell must not collapse to a 0-cell, but `extend` does not
> reject it. The dimension guard only blocks *raising* (`image.dim() > dim`); for
> a 1-cell mapped to a 0-cell `image.dim() (0) > dim (1)` is false, so the guard
> passes. Then `check_boundary_match` runs at $k = 0$: `boundary_normal(·, 0, p)`
> of a 0-cell `p` is just `p` (it clamps $k$ to the image's top dimension), so the
> match succeeds whenever both endpoints of the source 1-cell map to the *same*
> point. A degenerate 1-cell (`s, t` both sent to `K.o`, `arr => K.o`) therefore
> loads **cleanly** — verified by interpreting a `let total F :: Edge` whose
> resulting `F` carries an entry of source dim 1 with an image of dim 0. The rule
> is only *accidentally* enforced when the two endpoint images differ (then the
> input/output boundaries genuinely disagree and the match fails). This is a
> source-side gap (`PartialMap::extend` has no lower-dimension guard) — one to
> triage into `docs/wiki/source-drift.md`.

> [!note] Collapse inference now *deliberately* lowers dimension.
> Since the maps-with-holes rewrite (`3d20e03`), `assign_cell`'s **collapse
> inference** infers a cell's image as a lower-dimensional diagram when its
> boundary maps below dimension $n-1$ (`collapsed_boundary_image`). So
> dimension-lowering is not merely an unguarded accident at the core `extend`
> layer — the interpreter *intends* it in this case. Whether that is compatible
> with the no-identities discipline (a collapsed cell is arguably an identity in
> disguise) is the open edge of this decision; see [[hole]] and
> [[source-drift]].

The interpreter-level wiring (`let total`, `attach … along`, clause evaluation)
lives in `src/interpreter/partial_map.rs` — `assign_cell` infers the boundary
entries (or records them as [[hole|holes]]) then submits each committed entry to
`PartialMap::extend`; see [[core-partial-map]] for the full module and
[[core-matching]] for the related matching machinery.

A concrete downstream consequence: `Engine::target_reached`
*(src/interactive/engine.rs)* only succeeds when `active_len > 0`, since with no
identities an initial-equals-target diagram at step zero is never a valid proof.
