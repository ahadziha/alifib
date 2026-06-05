---
kind: concept
status: stable
last-touched: 2026-06-05
---

# Partial map

A **partial map** $f : U \rightharpoonup V$ assigns, to each generating cell of a
source [[diagram]] $U$, an image [[diagram]] in a target $V$ — but only on a
*subset* of $U$'s generators. It is the language's notion of a structure-
preserving morphism between [[directed-complex|complexes]] (a type is a
[[directed-complex]], not necessarily regular; its [[atom|atoms]] and
[[molecule|molecules]] are the [[regular-directed-complex|regular]] shapes): a
way to say "this generator of $U$ *is* that piece of $V$". Where the assignment
is defined on every generator it is **total**; the partial case is what makes
incremental construction and refinement possible.

The defining discipline is **boundary compatibility**. A map is not free to send
cells anywhere: if $a$ is in the domain, then so is every cell of its
[[boundary]], and the images must agree on the overlap. Formally, for a cell $a$
of dimension $\dim a = k+1$,
$$
\partial^-_k\,f(a) \;=\; f(\partial^-_k a),
\qquad
\partial^+_k\,f(a) \;=\; f(\partial^+_k a).
$$
The image of a cell's input/output boundary is the input/output boundary of its
image. This is exactly the *cellular map* condition: a map closed under taking
boundaries, commuting with $\partial^\pm$.

## Definition

Let $U, V$ be diagrams. A partial map $f : U \rightharpoonup V$ consists of a
**domain of definition** $\mathrm{dom}\,f$ — a downward-closed set of generators
of $U$ (closed under boundary: $a \in \mathrm{dom}\,f$ and $b \in \partial a$
imply $b \in \mathrm{dom}\,f$) — together with an image diagram $f(a)$ for each
$a \in \mathrm{dom}\,f$, subject to the boundary law above.

**Dimension constraint.** The implementation enforces exactly one inequality:
$\dim f(a) \le \dim a$ — an image may not *raise* dimension. *Lowering is
allowed*: a $k$-cell may map to a diagram of dimension $< k$, collapsing to it
(collapse inference does exactly this). The only thing alifib's lack of
[[0001-no-identities|identity cells]] rules out is using "the identity on $p$" as
a filler — there is no such cell — so a collapse maps to the genuine
lower-dimensional cell rather than to a degenerate identity. A refinement that
wants to mark an "internal step" must therefore name an honest cell for it.
(`collapsed_boundary_infers_image` and `dimension_lowering_case1_is_sound` are
the tests that pin this down — a 1-cell whose endpoints both map to a $0$-cell
maps to that $0$-cell.)

**Action on composites.** A diagram is built by pasting atoms along their
boundaries, $U = U_1 \#_k U_2 \#_{k'} \cdots$. A partial map is determined on
generators alone; its value on a composite is computed by *following the paste
structure*:
$$
f(U_1 \#_k U_2) \;=\; f(U_1) \;\#_k\; f(U_2).
$$
The map is applied leaf-by-leaf to the paste tree and the images are recomposed
with the same $\#_k$ operations. (When every image is a single generator, this
collapses to a relabelling — no re-pasting needed.) Applying $f$ to a composite
fails exactly when some leaf lies outside $\mathrm{dom}\,f$.

**Composition.** Partial maps compose: $g \circ f$ is defined on the part of
$\mathrm{dom}\,f$ whose $f$-image lands entirely inside $\mathrm{dom}\,g$,
matching the usual partial-function composite. This makes complexes-and-partial-
maps a category, and refinements compose as one expects.

### `attach ... along`

The language surfaces partial maps through two faces:

- **Naming a map.** `let total Name :: Source = [ gen => diagram, … ]` (or the
  block form `[ prefix? clause* ]`) names a partial map out of the complex
  `Source`. The `total` keyword asserts that the map is defined on *every*
  generator of the domain — totality is checked, and a gap is an error.

- **`attach T :: S along [ … ]`.** This imports a fresh copy of type `S` under
  the name `T`, and the `along` clause is a partial map *identifying* generators
  of the imported copy with diagrams already present in the host complex. It is
  the construction that glues a sub-structure into a larger one along a shared
  boundary — the categorical pushout/amalgamation read operationally. Omitting
  `along` attaches a disjoint copy (the empty map).

A clause `gen => ?` leaves the image a [[hole]] — a *pending assignment* the map
records rather than commits. A map may carry holes and still be well-formed
(even `total`, since a hole counts as covering its generator — `total_map_accepts_holes`);
the open holes are then filled later, either by the local inferences that fire as
the map is built, or interactively (`fill`). The variant `<map> => ?` holes a
whole sub-map pointwise (`map_to_hole_holes_each_cell`). See [[hole]] for the full
account.

## Implementation

Realised by [[core-partial-map]]. The data structure and its laws live in
`src/core/partial_map.rs` as `PartialMap`:

- the boundary law is enforced in `PartialMap::extend` via
  `partial_map::check_boundary_match` *(internal)*, which applies the map to a
  cell's stored boundary and compares it (after `Diagram::normal`) against
  `Diagram::boundary_normal` of the proposed image;
- the dimension constraint is the early `image.dim() > dim` rejection in
  `extend` — this blocks *raising* only; lowering is allowed (collapse inference
  relies on it), so there is no lower-bound guard;
- action on composites is `PartialMap::apply`, walking the diagram's
  `PasteTree` via `partial_map::apply_tree` *(internal)*; the relabelling fast
  path fires when the `cellular` flag holds (`partial_map::remap_tag`);
- composition is `PartialMap::compose`, dropping entries whose image escapes
  $g$'s domain exactly as the partial composite demands.

The language front-end is `src/interpreter/partial_map.rs`: `interpret_partial_map`
and `interpret_pmap_def` evaluate the surface syntax. Construction runs through a
`MapBuild` *(internal)* that carries the committed `PartialMap` alongside pending
[[hole|holes]]; `assign_cell` *(internal)* commits a determined image or records a
hole, forcing every undefined boundary face (`ensure_defined`) before the cell
itself, and `cascade` commits each conditional whose dependencies have closed.
`check_map_totality` *(internal)* implements the `total` keyword, counting a holed
generator as covered. The `attach ... along` statement is wired in
`src/interpreter/include.rs::interpret_attach_instr`, which calls
`interpret_pmap_def` on the `along` clause (`resolve_attach` *(internal)*).

## Related

[[diagram]] · [[boundary]] · [[rewriting]] · [[module-system]] ·
[[directed-complex]] · [[regular-directed-complex]] · [[0001-no-identities]] ·
[[0002-round-boundaries]]
