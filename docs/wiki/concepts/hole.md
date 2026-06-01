---
kind: concept
status: stable
last-touched: 2026-06-01
code: [src/interpreter/inference.rs, src/interpreter/diagram.rs, src/interpreter/partial_map.rs, src/interpreter/load.rs]
---

# Hole

A **hole**, written `?` in the surface language, is a [[diagram]] whose identity
is *not given* but *to be inferred*. It is the placeholder for a cell that the
author declines to name, deferring its dimension and boundaries to whatever the
surrounding context forces them to be. Where a concrete diagram says "this is
$f$", a hole says "this is *whichever* cell makes the surrounding equation hold"
— and alifib's job is to reconstruct that cell, or to report that the
constraints leave it underdetermined or contradictory.

A hole is therefore not a value but an *unknown* in the algebraic sense. It
carries no data of its own beyond a process-unique identity; everything one can
say about it is a statement *relating* it to known diagrams: it must be parallel
to its companion in a paste, it must equal a given diagram under an assertion, it
must match the image of a source cell under a [[partial-map]]. Inference is the
business of collecting those statements and grinding them to a fixpoint.

## Definition

Fix an ambient [[regular-directed-complex|complex]] (the *scope*). A hole $h$ is
an unknown [[diagram]] in that scope. Its determination is sought via three kinds
of atomic fact, the **constraints**:

- **Dimension equality** $\dim h = n$. The hole is an $n$-cell.
- **Boundary equality** $\partial^s_k(h) = D$ for a sign $s \in \{-,+\}$, a
  dimension $k$, and a known diagram $D$. The hole's $k$-dimensional input
  ($s=-$) or output ($s=+$) boundary is exactly $D$.
- **Value** $h = D$: the hole is pinned to a specific diagram $D$ outright.

These are not independent. A value $h = D$ entails $\dim h = \dim D$ together with
boundary equalities at the two **principal** slots $(\,-, n-1)$ and $(\,+, n-1)$
where $n = \dim D$; and any boundary equality at slot $(s, k)$ entails the lower
**globular** boundary equalities $\partial^{s'}_j(D)$ for all $j < k$ and both
signs $s'$ — because the boundaries of a boundary are themselves determined. The
inference engine treats this entailment as a closure operator: it seeds the
atomic facts and propagates the implied ones until nothing new appears.

### Where the constraints come from

A hole acquires constraints from the syntactic site where it occurs. Each site
knows which fact a `?` there must satisfy:

- **Boundary declaration** `? -> t` or `s -> ?`. The hole sits opposite a known
  diagram, hence must be *parallel* to it: same dimension, same principal
  boundaries. This emits $\dim h = \dim(\text{companion})$ and a boundary equality
  at each principal slot.
- **Paste / juxtaposition** `… *k h *k …`. A hole composed at dimension $k$ with
  concrete neighbours inherits its dimension $n = k+1$ from the neighbour, and —
  for the first hole of a trailing block — its input boundary from the left
  neighbour's output.
- **Assertion** `assert ? = D` (or `assert D = ?`), when the *entire* other side
  is a bare hole: a value constraint $h = D$. An assertion `f ? g = h` with the
  hole *embedded* in a composite emits no value (that would be wrong); the paste
  context constrains it instead.
- **Partial-map clause** `gen => ?` (see below): the hole is the image of a source
  cell, so the map applied to that cell's boundaries gives the hole's boundaries.

### Two-phase, root-only solving

Inference is deliberately split in two:

1. **Collection.** During interpretation each site emits its atomic constraints
   into the running result. No hole is enriched in place; the interpreter only
   *accumulates* facts. Composite information (parallelism, equality) is
   decomposed into atomic dimension- and boundary-equalities *at the emission
   site*, so that the solver need only understand three constraint variants.
2. **Solving.** After the *entire file* is interpreted, a work-queue fixpoint runs
   over the collected constraints. Each constraint is applied to its hole; when it
   yields new information — a slot newly filled, or a dimension/value set for the
   first time — the implied constraints (the principal-slot decomposition of a
   value, the globular sub-boundaries of a boundary) are computed and enqueued.
   The loop ends when the queue drains.

Termination is structural rather than heuristic: each hole's per-slot state moves
monotonically from *empty* to *known* and never back, and dimension and value are
each set at most once. The number of state changes is thus bounded, so the
fixpoint is reached in finite time.

Conflicts are recorded, not fatal. If two constraints disagree on a slot, the
*first* wins (preserving its origin for diagnostics) and an inconsistency message
is logged against the hole. This keeps inference total: it always returns a
solved state per hole, possibly carrying a list of contradictions to report.

Solving is **root-only**. Constraints are collected across the whole program, but
the solver runs only on the holes of the *root* module; a hole appearing inside an
included dependency is flagged with a warning rather than inferred. This is a
deliberate scoping decision: the root file is the unit of interactive editing, and
a dependency's holes belong to its own authoring session.

## Implementation

The whole machinery lives in [[interpreter]], split across four files.

Identity and the constraint algebra are in `src/interpreter/inference.rs`. A hole
is named by `HoleId` *(internal)* — a `Copy` wrapper over a process-wide atomic
counter, allocated by `HoleId::fresh`, so two `?` tokens always get distinct ids
(the same pattern as `aux::GlobalId`). The three atomic facts are the variants of
the `Constraint` enum: `DimEq`, `BoundaryEq` (keyed by a `BdSlot`, a
sign-and-dimension pair), and `Value`. Each carries a `ConstraintOrigin` so
diagnostics can name the site (`Paste`, `Declaration`, `Assertion`, `PartialMap`)
that produced it.

The solver is the free function `solve(entries, constraints) -> Vec<SolvedHole>`.
It seeds a `VecDeque` with every constraint and drains it, dispatching through
`inference::process_constraint` *(internal)*: a `Value` derives a `DimEq` plus
principal `BoundaryEq`s via `Diagram::boundary_normal`; a `BoundaryEq` derives its
globular sub-boundaries via `inference::globular_sub_boundaries` *(internal)*; a
`DimEq` derives nothing. Per-hole accumulation and conflict detection are the
`SolvedHole::set_dim` / `set_value` / `set_boundary` *(internal)* methods, each
returning whether it set *new* information (the signal to enqueue derived facts).
The first-wins-and-record-inconsistency policy is pinned by the tests
`solve_conflicting_dim_eq`, `solve_boundary_eq_conflict_records_inconsistency`,
and `solve_value_conflict_records_inconsistency`; the entailment closure by
`solve_value_higher_dim` (value → principal boundaries) and
`solve_globular_cascade` (boundary → sub-boundaries).

The per-hole record carried *through* interpretation is `HoleInfo` in
`src/interpreter/types.rs`: an id, a source span, and partial-map bookkeeping
(`source_tag`, `direct_in_partial_map`, and renderer-only `partial_hints`). Each
`?` encountered becomes a `HoleInfo::new` added to the `InterpResult`.

Constraint emission for syntactic sites is in `src/interpreter/diagram.rs`. The
boundary-declaration and paste cases route through
`diagram::push_parallel_constraints` *(internal)*, which emits the `DimEq` and the
two principal `BoundaryEq`s for a hole parallel to a companion diagram;
`interpret_boundaries` calls it for `? -> t` / `s -> ?`. The trailing-hole logic in
the principal-paste interpreter emits an input `BoundaryEq` from the left
neighbour's output plus `DimEq`s for the block. The assertion interpreter emits
`Constraint::Value` only when the opposite side is a bare hole, gated by
`diagram::is_pure_hole_diagram` *(internal)*.

The `gen => ?` partial-map path is in `src/interpreter/partial_map.rs::enrich_holes`
*(internal)*, called after a map is built. For a **direct** hole (the `?` is the
entire RHS of a clause) it looks up the source cell's `CellData` and emits the
hole's dimension and, for each boundary the map fully covers, a `BoundaryEq`; where
the map covers a boundary only partially it stores a `PartialHint` for the renderer
instead of a constraint. **Embedded** holes (`gen => ? g`) emit no solver
constraint here — the surrounding paste already constrains them — only display
hints.

The two phases are wired together in `src/interpreter/load.rs`. After
`interpret_program` over the root module returns its `InterpResult`, the holes are
turned into `HoleEntry`s and handed to `solve`; the resulting `SolvedHole`s land in
`InterpretedFile::solved_holes`, with `partial_hints` copied across for rendering.
The root-only discipline is visible there too: dependency modules are interpreted
first, and a dependency hole triggers a *"boundary inference is only performed for
the root file"* warning rather than a solve. The end-to-end behaviour is exercised
by the `solved_holes` assertions in `tests/interpreter.rs`.

## Related

- [[partial-map]] — the `gen => ?` clauses whose image holes feed `enrich_holes`.
- [[boundary]] — the $\partial^s_k$ operators the constraints equate.
- [[diagram]] — what a hole is ultimately solved *to*.
- [[interpreter]] — the module that owns collection and solving.
- [[reconstruction]] — rendering a solved hole back into surface syntax.
