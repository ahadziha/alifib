---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Oriented graded poset

Start with the encoding problem. A pasting diagram — points, arrows between
points, 2-cells between parallel paths of arrows, and so on upwards — has to
be handed to a computer somehow. The insight alifib inherits from
Hadzihasanovic (*Combinatorics of higher-categorical diagrams*, 2024; the
running citation for every concept page) is that a diagram's shape is
captured by two pieces of data:

1. **which cells are faces of which** — a poset, graded by dimension, where
   $x < y$ means "$x$ appears somewhere in the boundary of $y$"; and
2. **which side each face is on** — every covering relation $x \lessdot y$
   (a face one dimension down) is labelled $-$ (*input*: $y$ consumes $x$) or
   $+$ (*output*: $y$ produces $x$).

That is the whole definition. An **orientation** on a graded poset is an
edge-labelling of its Hasse diagram with values in $\{+,-\}$ (2.1.1); an
**oriented graded poset** — *ogposet* from here on — is a graded poset with
an orientation (2.1.3). Each covering edge carries *exactly one* sign; this
small clause does real work later (see [[directed-complex]] for what breaks
when a face would need both signs).

A worked example. The 2-cell $\alpha : f \Rightarrow g \#_0 h$ — "rewrite the
arrow $f$ into the path $g$ then $h$" — has as its shape:

- dimension 0: three cells $x, y, m$ (the endpoints and the midpoint of the
  output path);
- dimension 1: three cells $f, g, h$, with $f: x \to y$ recorded as
  $\Delta^- f = \{x\}$, $\Delta^+ f = \{y\}$, and similarly $g: x \to m$,
  $h: m \to y$;
- dimension 2: one cell $\alpha$ with $\Delta^- \alpha = \{f\}$ (it consumes
  $f$) and $\Delta^+ \alpha = \{g, h\}$ (it produces $g$ and $h$).

Nothing else. The geometry — that $g$ and $h$ join end-to-end, that the two
sides of $\alpha$ share endpoints — is all *derivable* from the signed face
data. That derivation is what the rest of this wiki's concept pages are
about.

## The derived vocabulary

Three notions defined directly from the signs do most of the work downstream.

**Extremality.** Read the orientation as a flow: a $(k{+}1)$-cell consumes
its input faces and produces its output faces. A $k$-cell that *no* higher
cell produces (it has no output coface) lies on the **input frontier** of the
shape — it must be supplied from outside. Dually, a cell no higher cell
consumes lies on the **output frontier**. These extremal cells are the seeds
from which [[boundary|boundaries]] are computed. In the example: $f$ is
input-extremal (nothing outputs it), $g$ and $h$ are output-extremal.

**Maximality and purity.** A cell with no cofaces at all is **maximal**. A
shape is **pure** when every maximal cell has top dimension — no stray
lower-dimensional cell left dangling outside everything's boundary. Purity is
a cheap necessary condition for the *roundness* property that gates cell
construction ([[boundary]]).

**Canonical form.** Two ogposets are isomorphic exactly when a deterministic
*input-first traversal* — walk the shape from its input frontier, in an order
fixed by the face structure — relabels them identically. This makes shape
equality decidable by computing canonical forms and comparing tables, which
is how alifib decides every "do these boundaries agree?" question.

An ogposet by itself is **unconstrained** — almost no ogposet is the shape of
a meaningful diagram (the book opens §3.3 with examples that are not). The
tower above this page exists to carve out the meaningful ones: a
[[molecule]] is an ogposet built by an explicit grammar of pastings; an
[[atom]] is an indecomposable molecule; a [[regular-directed-complex]] is an
ogposet locally made of atoms. The ogposet is the substrate they all refine.

## Implementation

`Ogposet` in `src/core/ogposet.rs` — see [[core-ogposet]] — is the definition
above stored as adjacency tables:

- Four tables `faces_in`, `faces_out`, `cofaces_in`, `cofaces_out`, each
  indexed `[dim][cell]`: the sets $\Delta^- y$, $\Delta^+ y$ and their duals
  $\nabla^- x$, $\nabla^+ x$ (which higher cells have $x$ as an input/output
  face). Storing cofaces too makes extremality a constant-time lookup.
- `dim: isize`, with $-1$ for the empty shape; `Ogposet::empty` and
  `Ogposet::point` are the base cases.
- A `normal` flag recording whether the cells are already in canonical
  traversal order.

The sign is `ogposet::Sign` *(internal)*: `Input`, `Output`, and a third
variant `Both` that is **not** a third orientation but a query convenience —
"union over both signs" — used e.g. when collecting a whole boundary sphere.
One layer up, `diagram::Sign` is two-valued (`Input`/`Output`, converting via
`as_ogposet_sign`), because an operation on a [[diagram]] always addresses
one side.

The derived vocabulary maps one-to-one:

- **Extremality** — `Ogposet::extremal(sign, k)` *(internal)*: input-extremal
  = empty `cofaces_out` row, output-extremal = empty `cofaces_in` row.
  **Maximality** — `Ogposet::maximal`; **purity** — `Ogposet::is_pure`
  *(internal)*.
- **Canonical form** — `ogposet::traverse` *(internal)* is the input-first
  traversal; `ogposet::normalisation` applies it to a whole shape;
  `ogposet::find_isomorphism` decides isomorphism by comparing the resulting
  tables (recomputed per call; nothing is memoised).
- **Boundary extraction** — `ogposet::boundary` and `boundary_traverse`
  *(both internal)*; the full account, including what the `Both` branch of
  `boundary_traverse` is for, is in [[boundary]] and [[atom]].

Everything here is shape only. Labels — which generator each cell
instantiates — and the pasting history live one layer up in `Diagram`
(`src/core/diagram.rs`, [[core-diagram]]), which holds its shape as an
`Arc<Ogposet>`.

## Related

[[boundary]] — the $\partial^\pm_k$ operators read off the orientation ·
[[molecule]] — the constructively well-formed ogposets · [[atom]] ·
[[regular-directed-complex]] · [[directed-complex]] · [[diagram]] ·
[[partial-map]]
