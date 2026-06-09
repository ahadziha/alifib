---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Oriented graded poset

An **oriented graded poset** (*ogposet*) is the bare combinatorial shape beneath
every alifib value: a finite set of faces stratified by dimension, where each
covering step between dimensions carries an orientation — a $\pm$ sign splitting
a cell's faces into **input** ($-$) and **output** ($+$). It is the substrate
beneath a [[directed-complex]], and in particular beneath the
[[regular-directed-complex|regular]] ones — the shapes in which every
[[molecule]] lives. Strip the labels off a [[diagram]] and what remains is its
ogposet; the [[boundary|boundaries]] $\partial^\pm_k$ are nothing but this
orientation read off the face structure. The ogposet is genuinely the *bare*
layer — shape, no labels, no regularity: a [[regular-directed-complex]] is an
ogposet all of whose cells are regular, while a general [[directed-complex]] —
what a *type* assembles to once labels identify cells — need not be regular at
all (see [[diagram]]).

## Definition

Following Hadzihasanovic, an ogposet $G$ consists of (`docs/interp/interp.pdf`
§3.1 *Oriented Graded Posets*):

- a **dimension** $\dim G \in \mathbb{Z}_{\ge -1}$, with $-1$ the empty ogposet;
- for each $0 \le d \le \dim G$ a finite set of **cells** $G_d$;
- **face maps** $\partial^-_d, \partial^+_d : G_d \to \mathcal{P}(G_{d-1})$
  assigning to each $d$-cell its input and output $(d{-}1)$-faces;
- dually, **coface maps** recording, for each cell, the higher cells in whose
  boundary it appears.

The graded structure is a poset whose order is the transitive closure of "is a
face of"; the *orientation* is the extra datum that each covering relation is
tagged $-$ or $+$. Three derived notions do the real work.

**Extremality.** A $k$-cell is **input-extremal** when it has no output coface —
no $(k{+}1)$-cell has it among its output faces, so nothing *produces* it;
**output-extremal** when it has no input coface — nothing *consumes* it. These
are the cells lying on the input / output frontier of the shape. **Maximal**
cells have no coface at all.

**Boundary.** $\partial^s_k G$ for $s \in \{-,+\}$ is the sub-ogposet on the
downward closure of the $s$-extremal $k$-cells together with the maximal cells of
dimension $< k$. It is again an ogposet of dimension $\le k$, and comes with an
embedding back into $G$ — the full account is [[boundary]].

**Roundness.** $G$ is **round** when it is *pure* (every below-top cell has a
coface) and, at every dimension, the interior touched by the input boundary is
disjoint from that touched by the output boundary. Roundness is a property of
the *bare shape* — orientation alone, no labels — and is the precondition for
$G$ to bound a single **cell**, not for pasting; see [[boundary]] for the full
story.

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
- **Extremality** is `Ogposet::extremal(sign, k)` *(internal)*, defined by
  *missing cofaces* — an `Input`-extremal cell has no output coface, an
  `Output`-extremal one has no input coface. **Maximality** (`Ogposet::maximal`)
  is no coface at all; **purity** (`is_pure`) is every below-top cell having a
  coface. **Roundness** is the public `Ogposet::is_round` (built on `is_pure` and
  `build_layer`); it reads the bare shape, never labels.
- **Boundary extraction** $\partial^s_k$ is `ogposet::boundary` *(internal)*,
  returning the faithful sub-ogposet and its `Embedding`; its normalised cousin
  is `boundary_traverse`. The latter's `Both` branch is special: it returns the
  full boundary *sphere* of an $n$-cell (via `build_stack_cell_n`), ignoring $k$
  entirely — used when forming a cell from two parallel diagrams.
- **Canonical form / isomorphism**: `normalisation` and `find_isomorphism`,
  both driven by the general `traverse`. Shape equality is decided by comparing
  canonical forms; the result is recomputed on every call (no memoisation).
- The membership-only paths `closure` and `signed_k_boundary_of_cell` answer
  "is this cell in the downward closure?" / "what is $\partial^\alpha_k(x)$ of one
  cell?" without building a sub-ogposet.

Note: this is the *shape only*. Labels and paste history live one layer up in
`Diagram` (`src/core/diagram.rs`, see [[core-diagram]]), which holds the shape as
an `Arc<Ogposet>`. Beware a sign subtlety — the [[diagram]] layer has its **own**
two-valued `diagram::Sign` (`Input` / `Output`, no `Both`), since a diagram
operation always acts on exactly one boundary; it converts to the three-valued
ogposet `Sign` via `as_ogposet_sign`. The `Both` variant is internal to the
shape layer.

## Related

[[directed-complex]] · [[regular-directed-complex]] · [[molecule]] · [[diagram]] · [[boundary]] · [[atom]] · [[partial-map]]
