---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Oriented graded poset

An **oriented graded poset** (*ogposet*) is the bare combinatorial shape beneath
every alifib value: a finite set of faces stratified by dimension, where each
covering step between dimensions carries an orientation — a $\pm$ sign splitting
a cell's faces into **input** ($-$) and **output** ($+$). It is the substrate of
a [[regular-directed-complex]], hence of every [[molecule]] and [[diagram]];
strip the labels off a diagram and what remains is its ogposet. The
[[boundary|boundaries]] $\partial^\pm_k$ are nothing but this orientation read
off the face structure.

## Definition

Following Hadzihasanovic, an ogposet $G$ consists of (interp.tex §Oriented
Graded Posets):

- a **dimension** $\dim G \in \mathbb{Z}_{\ge -1}$, with $-1$ the empty ogposet;
- for each $0 \le d \le \dim G$ a finite set of **cells** $G_d$;
- **face maps** $\partial^-_d, \partial^+_d : G_d \to \mathcal{P}(G_{d-1})$
  assigning to each $d$-cell its input and output $(d{-}1)$-faces;
- dually, **coface maps** recording, for each cell, the higher cells in whose
  boundary it appears.

The graded structure is a poset whose order is the transitive closure of "is a
face of"; the *orientation* is the extra datum that each covering relation is
tagged $-$ or $+$. Three derived notions do the real work.

**Extremality.** A $k$-cell is **input-extremal** when it has no output coface
(nothing consumes it as an output), **output-extremal** when it has no input
coface. These are the cells lying on the input / output boundary of the shape.
**Maximal** cells have no coface at all.

**Boundary.** The $\partial^s_k G$ for $s \in \{-,+\}$ is the sub-ogposet on the
downward closure of the $s$-extremal $k$-cells: take everything on the $s$-side
of the $k$-skeleton and forget the rest. It is again an ogposet of dimension
$k$, and comes with an embedding back into $G$.

**Roundness.** $G$ is **round** when, at every dimension, the interior touched
by the input boundary is disjoint from that touched by the output boundary.
Roundness is the precondition for $G$ to be the input/output shape of a single
[[atom]] — a globe-like shape with two well-separated poles.

Two ogposets are *isomorphic* exactly when they share a canonical form; the
canonical form is obtained by an input-first **traversal** that walks the
orientation deterministically. This is what makes shape equality decidable, and
underwrites both [[partial-map|embeddings]] and [[rewriting|matching]].

### The sign

The orientation is carried by a three-valued tag. $\mathsf{Input}$ ($\partial^-$)
and $\mathsf{Output}$ ($\partial^+$) are the two genuine polarities; a third
value $\mathsf{Both}$ is a convenience meaning "either side", used when a query
ranges over the whole boundary (e.g. forming the downward closure of a cell, or
the shared boundary of an $n$-cell whose two poles are pasted). $\mathsf{Both}$
is not a third orientation — it is the union $\partial^- \cup \partial^+$.

## Implementation

Realised by `Ogposet` and `Sign` in `src/core/ogposet.rs` — see [[core-ogposet]].

- `Ogposet` stores the four adjacency tables `faces_in` / `faces_out` /
  `cofaces_in` / `cofaces_out`, each indexed `[dim][cell]`, plus `dim`
  ($-1$ = empty) and a `normal` flag for canonical ordering. `Ogposet::empty`
  and `Ogposet::point` are the base shapes.
- `Sign` *(internal `pub(crate)` enum)* is the orientation: variants `Input`,
  `Output`, `Both` — exactly the $\partial^-$ / $\partial^+$ / union split above.
  `Ogposet::faces_of` and `cofaces_of` dispatch on it.
- **Extremality** is `Ogposet::extremal(sign, k)` *(internal)*; **maximality** is
  `Ogposet::maximal`; **roundness** is the public `Ogposet::is_round` (with
  `is_pure` as a helper).
- **Boundary extraction** $\partial^s_k$ is `ogposet::boundary` *(internal)*,
  returning the sub-ogposet and its `Embedding`; its normalised cousin is
  `boundary_traverse`.
- **Canonical form / isomorphism**: `normalisation` and `find_isomorphism`,
  both driven by the general `traverse`.

Note: this is the *shape only*. Labels and paste history live one layer up in
`Diagram` (`src/core/diagram.rs`, see [[core-diagram]]), which holds the shape as
an `Arc<Ogposet>`. Beware a sign subtlety — the [[diagram]] layer has its **own**
two-valued `diagram::Sign` (`Input` / `Output`, no `Both`), since a diagram
operation always acts on exactly one boundary; it converts to the three-valued
ogposet `Sign` via `as_ogposet_sign`. The `Both` variant is internal to the
shape layer.

## Related

[[regular-directed-complex]] · [[molecule]] · [[diagram]] · [[boundary]] · [[atom]] · [[partial-map]]
