---
kind: decision
status: stable
last-touched: 2026-06-13
---

# 0001 — no identity cells (but composition is unital)

## Context

alifib follows Hadzihasanovic's theory of [[molecule|molecules]] — whose shapes
are [[regular-directed-complex|regular directed complexes]], assembled into types
that are the looser [[directed-complex|directed complexes]]. What the theory
*lacks* is more precise than the slogan "no identities" suggests, and worth
stating carefully — it explains both what the code refuses to build and what it
deliberately permits.

**Why unit-less, more deeply.** The motivation is not merely a guard against
non-termination — though units do loop (a unit witnesses a *do-nothing* process,
so admitting one makes rewriting non-terminating). It reflects a layering:
*units are semantics*. A unit is a **representation of a process that does
nothing**, whereas literally doing nothing is already present — it is the
lower-dimensional cell itself. The raw, unit-less layer of directed-complex data
is the one appropriate for *computation* (rewriting, abstract machines); units,
unitors and the rest belong one storey up, where higher-categorical *meaning*
lives (diagrammatic sets and the 2024–25 papers). Non-termination is then a
*symptom* of conflating the storeys, not the root reason. Where the examples
need unit-like behaviour they **direct** the would-be structural equivalences
rather than adding units — the [[trs-encoding|TRS encoding]] is a *laxified*
cartesian monoidal category whose copy/discard/swap coherences become directed
higher cells.

## Decision

**No cell ever represents the identity on a lower-dimensional diagram.** There is
no $(n{+}1)$-generator standing for the identity on an $n$-diagram: no identity
2-cell on a 1-cell, no identity 1-cell on a 0-cell, no degenerate filler of any
kind. Cells are only genuine generators and the [[diagram|diagrams]] they build.

This is **not** the absence of units. *Pasting has units.* For a [[diagram]] $U$
and any $k < \dim U$,
$$
\partial^-_k U \;\#_k\; U \;\cong\; U \;\cong\; U \;\#_k\; \partial^+_k U ,
$$
so $\partial^-_k U$ is a left unit and $\partial^+_k U$ a right unit for $\#_k$;
composition — and hence [[rewriting]] — is unital. The units are *boundaries*, not
cells, which is exactly why they need no identity-cell to exist.

## Consequences

- **A zero-step rewriting proof is a valid (identity) proof.** It is the unit of
  $\#_n$ at the initial $n$-diagram, represented simply by that $n$-diagram, not
  by a degenerate $(n{+}1)$-cell. `target_reached` is therefore just
  $current \cong target$, true at step $0$ too — an initial diagram already equal
  to the target is already proved.

- **Lower-dimensional structure is represented explicitly.** Combined with
  [[0002-round-boundaries]] (a cell's boundaries must be round and of the
  appropriate dimension), an $(n{+}1)$-cell whose input or output is *morally* of
  dimension $< n$ needs an explicit stand-in — there is no degenerate filler to
  lean on. The worked example is TRS constants (`examples/TRS.ali`): a constant
  is morally nullary, but is modelled **not** as a 2-cell with an empty input
  boundary (which could not be round) but as a 2-cell `node : unit -> cod` whose
  input is an explicit **unit 1-cell** `unit = Unit.wire`. The `Unit` type
  correspondingly introduces explicit **unitor** 2-cells — `lunit : unit wire ->
  wire`, `runit : wire unit -> wire`, and their inverses — to move that unit
  around, precisely because the pasting units are not themselves cells one can
  name or rewrite with.

- **Dimension-lowering maps are legitimate.** Since there is no identity cell to
  send a collapsed cell *to*, a [[partial-map|map]] that collapses a $k$-cell
  sends it to the genuine lower-dimensional image — and collapse inference
  produces such maps on purpose (see [[hole]], [[core-partial-map]]).

## Code refs

- `PartialMap::extend` *(src/core/partial_map.rs)* guards only dimension-*raising*
  (`image.dim() > dim`); there is no lower-bound guard, by design.
- Unitality at the paste-tree level: `remove_units` *(src/core/paste_tree.rs,
  internal)* — a paste $t_1 \#_k t_2$ in which one side has dimension $\le k$ is
  that side acting as a unit, and collapses to the other side.
- The zero-step proof: `RewriteEngine::target_reached` is `current ≅ target` with
  no step-count gate; `RewriteEngine::assemble_proof` returns the initial diagram
  when there are no steps; `stored_expr` renders it for a zero-step session
  (`src/interactive/{engine,session}.rs`; see [[interactive-engine]]).
- Round boundaries are the genuine structural constraint — [[0002-round-boundaries]].
