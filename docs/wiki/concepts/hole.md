---
kind: concept
status: stable
last-touched: 2026-06-03
code: [src/core/map_hole.rs, src/interpreter/partial_map.rs, src/interactive/fill.rs, src/output/normalize.rs]
---

# Hole

A **hole**, written `?` in the surface language, is a *pending assignment* in a
[[partial-map|partial map]]: an entry the map cannot yet commit because part of
its image is unknown. Holes only ever live inside a map. Where a clause
`arr => a` names the image of a generator outright, a clause `arr => ?` declines
to name it, leaving the image *open* — to be supplied later, interactively, or
inferred from the structure the rest of the map already fixes.

This is a deliberate narrowing from the older, free-floating notion of a hole as
"any unknown the surrounding equations must pin down". A hole is now anchored to
exactly one domain generator of one map, and its meaning is operational: a map
with holes is a map that is not yet total, and *filling* a hole is extending the
map by one more clause until, eventually, no hole remains.

## Two flavours

A hole records the image of a generator $x$ of the map's domain. It comes in two
kinds, distinguished by whether the image diagram is yet known:

- **Pure hole**, written `arr => ?`: the image of `arr` is wholly unknown. The
  hole carries only `arr`'s dimension and, once those are forced, its boundaries.
- **Conditional assignment**, written `x => a` where some boundary face of `x` is
  *itself* still unmapped: the image is known to be the diagram `a`, but `x`
  cannot enter the real map until those faces (its **dependency holes**) are
  filled. When they are, `x` commits automatically.

The conditional flavour is what makes a single named assignment generate further
holes. Writing `arr => ?` for a 1-cell `arr : s -> t` forces holes for the images
of `s` and `t` as well (a cell's image determines its boundaries' images by the
cellular-map law); writing `r => m` for a cell whose source `f g` is not yet
mapped leaves `r` pending and turns `f`, `g` into the unknowns, subject to the
**constraint** that $F(f) \#_0 F(g) = \partial^- m$.

## Boundaries as paste trees, not diagrams

The crux of the design: a hole's boundaries are stored as **paste trees** whose
leaves may be *metavariables*, never as realised [[diagram|diagrams]]. Each hole
owns a process-unique metavariable `HoleId` (rendered `?name`, after the
generator it images), and a dependent hole refers to another by carrying that
metavariable as a `Tag::Hole(id)` leaf in its boundary tree.

Keeping boundaries unrealised is what lets a hole be filled by a *non-round* or
even lower-dimensional diagram: there is no premature commitment to a directed
sphere that a degenerate filler would violate. A hole's outstanding **dependencies**
are read straight off its boundary trees — the set of `Tag::Hole` leaves still
present — so a conditional with no remaining metavariables is, by definition,
ready to commit.

## Filling — construction time and interactively

Holes are resolved in two quite different settings.

**At construction.** While a map is built clause by clause, three local
inferences fire, none of them a global solver:

- **Forced faces.** Assigning any cell forces every undefined boundary face to
  be assigned too — recursively, through the same path — so a cell's faces are
  always holes or commitments, never silently absent.
- **Sound (case-1) inference.** When a cell's image is known and one of its
  boundaries is a *single* cell, that cell's image is forced by reading it off
  the known image's boundary — no choice is involved, so it is committed
  directly rather than left a hole.
- **Collapse inference.** When a cell's boundary maps to a diagram of dimension
  *below* $n-1$, the cell itself is inferred to be that collapsed diagram rather
  than made a hole. This is the implementation deliberately lowering dimension —
  a legitimate move (see [[0001-no-identities]]), not a degeneracy.

When a filled hole's image becomes known, its paste tree is **substituted** for
its metavariable in every other hole's boundary trees, and any conditional whose
dependencies have all closed is **cascaded** into the real map. The cascade
repeats until no ready conditional remains.

**Interactively.** A hole that survives interpretation is *open* — a normal,
non-error state of the map. The interactive front-ends ([[interactive-session]])
list open holes and let the author fill them one at a time:

- a hole on an $m$-cell with $m \ge 1$ is a [[rewriting|rewrite]] from
  $F(x.\mathtt{in})$ to $F(x.\mathtt{out})$, built with the ordinary
  [[interactive-engine|rewrite engine]];
- a hole on a $0$-cell is just the choice of one of the target's $0$-cells.

Finalising a fill appends `x => <proof>` to the map's definition and
re-evaluates the file. The new clause sits after the original `arr => ?`; by the
idempotence $[\,x \Rightarrow ?,\ x \Rightarrow a\,] \equiv [\,x \Rightarrow a\,]$
it commits $x$ with the hole gone. So the source file is always the durable
record — there is no separate hole store.

## Implementation

The hole datum is `MapHole` in `src/core/map_hole.rs`: a metavariable `meta:
HoleId`, the domain generator `source: Tag` and its `dim`, the optional known
`image: Option<Diagram>` (the pure/conditional distinction), and `boundary:
Option<(PasteTree, PasteTree)>` — the input/output trees, both present or both
absent (a $0$-cell has neither). `MapHole::deps` derives the outstanding
dependencies by collecting the `Tag::Hole` leaves of those trees
(`collect_hole_deps`); `collects_only_hole_leaves` pins it.

The build machinery is in `src/interpreter/partial_map.rs`. A `MapBuild`
*(internal)* carries the hole-free `PartialMap` alongside the pending `holes`.
`assign_cell` *(internal)* is the workhorse: it commits a fully-determined image,
or records a hole via `upsert_entry`, applying case-1 and collapse inference
(`collapsed_boundary_image`) along the way; `ensure_defined` forces undefined
boundary faces through the same path. `commit_one` adds one entry to the real map
and substitutes the filled metavariable into the remaining holes; `cascade` then
commits every conditional whose `deps` have closed. `hole_map_image` implements
the pointwise `<map> => ?`. A pending assignment that cannot be committed because
a *dependency* failed is blamed via `blame_pending` (the fix in `a151779`). The
evaluated map carries its leftover holes out as `EvalMap::holes`
(`src/interpreter/types.rs`); `check_map_totality` counts a holed generator as
*covered*, so `let total F :: D = [ arr => ? ]` is accepted (a hole is a
deliberate placeholder, not an omission).

Rendering is in `src/output/normalize.rs`: `render_hole_line`,
`render_hole_boundary`, and `render_hole_constraints` turn a `MapHole` into its
`?name : in → out` display and the equations a conditional imposes, sharing
`hole_names` to name each metavariable after the generator it images.

Interactive filling is `src/interactive/fill.rs`: `list_open_holes` /
`list_constraints` enumerate a module's holes in a deterministic `(type, map,
dim, source-name)` order; `start_fill` opens a `FillSession` (`Rewrite` for
$m \ge 1$, `ZeroCell` for a $0$-cell), checking via `blocking_holes` that a
hole's dependencies are filled first; `edit_for_fill` splices the proof back into
the map definition — pinning onto an explicit `=> ?` when present
(`pins_a_dotted_explicit_hole_in_place`), otherwise appending
(`appends_when_no_matching_explicit_hole`).

## Related

- [[partial-map]] — holes live inside a partial map; `arr => ?` and `<map> => ?`
  are its surface.
- [[core-partial-map]] — the `MapBuild` / `cascade` / `commit_one` machinery.
- [[boundary]] — the $\partial^s_k$ the cellular-map law equates; a hole's
  boundary trees are these, deferred.
- [[diagram]] — what a hole is ultimately filled *with* (possibly non-round).
- [[interactive-session]] — the `holes` / `fill` / `done` workflow.
- [[0001-no-identities]] — why dimension-lowering (hence collapse inference) is
  legitimate, not a degeneracy.
