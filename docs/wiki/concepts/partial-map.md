---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Partial map

A **partial map** $f : U \rightharpoonup V$ assigns, to each generating cell of a
source [[diagram]] $U$, an image [[diagram]] in a target $V$ — but only on a
*subset* of $U$'s generators. It is the language's notion of a structure-
preserving morphism between [[regular-directed-complex|complexes]]: a way to say
"this generator of $U$ *is* that piece of $V$". Where the assignment is defined
on every generator it is **total**; the partial case is what makes incremental
construction and refinement possible.

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
$\dim f(a) \le \dim a$ — an image may not *raise* dimension. Lowering is not
forbidden by a dimension test, but because alifib has [[0001-no-identities|no
identity cells]] there is nothing for a genuine $k$-cell to collapse *to*: no
identity exists to absorb the slack, so a refinement target must spell out
explicit "internal step" cells rather than quietly identifying detail with an
identity. The dimension test itself is the engine's rejection of an image whose
dimension exceeds the source's.

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

A clause `gen => ?` leaves the image a [[hole]]; the elaborator then *infers*
the missing image from boundary constraints induced by the map on `gen`'s
boundary — this is the refinement-by-inference workflow.

## Implementation

Realised by [[core-partial-map]]. The data structure and its laws live in
`src/core/partial_map.rs` as `PartialMap`:

- the boundary law is enforced in `PartialMap::extend` via
  `partial_map::check_boundary_match` *(internal)*, which applies the map to a
  cell's stored boundary and compares it (after `Diagram::normal`) against
  `Diagram::boundary_normal` of the proposed image;
- the dimension constraint is the early `image.dim() > dim` rejection in
  `extend` — note this blocks *raising* only; the no-*lowering* half of
  [[0001-no-identities]] is **not** enforced (see that page's warning);
- action on composites is `PartialMap::apply`, walking the diagram's
  `PasteTree` via `partial_map::apply_tree` *(internal)*; the relabelling fast
  path fires when the `cellular` flag holds (`partial_map::remap_tag`);
- composition is `PartialMap::compose`, dropping entries whose image escapes
  $g$'s domain exactly as the partial composite demands.

The language front-end is `src/interpreter/partial_map.rs`: `interpret_partial_map`
and `interpret_pmap_def` evaluate the surface syntax; `extend_map_for_cell`
performs the *smart* extension that recursively maps a cell's boundary
dependencies before the cell itself; `check_map_totality` *(internal)* implements
the `total` keyword; and `enrich_holes` *(internal)* turns a `gen => ?` clause
into boundary constraints for inference. The `attach ... along` statement is
wired in `src/interpreter/include.rs::interpret_attach_instr`, which calls
`interpret_pmap_def` on the `along` clause (`resolve_attach` *(internal)*).

## Related

[[diagram]] · [[boundary]] · [[rewriting]] · [[module-system]] ·
[[regular-directed-complex]] · [[0001-no-identities]]
