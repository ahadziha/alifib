---
kind: concept
status: stable
last-touched: 2026-06-10
---

# Atom

An **atom** is a [[molecule]] with a greatest element: one cell whose closure
is the whole shape (3.3.9). Atoms are what a generator declaration mints —
`a : U -> V` names one new top cell whose input is the diagram $U$ and whose
output is $V$ — and they are the indecomposable units everything else is
pasted from: a molecule is an atom precisely when its derivation did not end
in a nontrivial (Paste) (Lemma 3.3.10).

Beware the obvious-looking shortcut: "has one top-dimensional cell" is *not*
atomicity. Whisker a 2-cell with an arrow and you get a molecule with a
single 2-cell but two maximal cells — the 2-cell and the whisker — and no
greatest element. The greatest element must dominate *everything*.

## The rewrite construction, step by step

How do you build the shape of a new $(n{+}1)$-cell from its declared input
and output? The book's answer is the *rewrite construction*
$U \Rightarrow V$ (3.2.1), and the code follows it exactly, so here it is
slowly. You are given two $n$-dimensional molecules $U$ and $V$.

1. **Both must be round** ([[boundary]]): each one's input and output
   hemispheres meet only along their common rim, so each has a well-defined
   boundary sphere $\partial U$, $\partial V$. Roundness is what lets a
   diagram act as *one side of a single cell*; a non-round diagram (say,
   two 2-cells side by side) has a frontier too ragged to cap with one cell.
2. **The boundary spheres must agree, sign by sign**: an isomorphism
   $\varphi : \partial U \cong \partial V$ that restricts to
   $\varphi^- : \partial^- U \cong \partial^- V$ and
   $\varphi^+ : \partial^+ U \cong \partial^+ V$. The sign condition is what
   "parallel" means: $U$ and $V$ share the same input *and* the same output.
3. **Glue** $U$ and $V$ along $\varphi$ — a pushout, after which
   $U \cap V = \partial U = \partial V$ — and **adjoin one new top element**
   $\top$ with $\Delta^- \top = U_n$ (all of $U$'s top cells) and
   $\Delta^+ \top = V_n$.

The result is an atom of dimension $n+1$ whose input boundary is $U$ and
output boundary is $V$ (Lemma 3.2.3); it is itself round (3.2.9), which is
what lets the construction iterate to the next dimension. The base case
$n = 0$ is the point, given by no data. There are no identity atoms — a
degenerate cell over $U$ would need $\partial^- = \partial^+$, which
roundness forbids ([[0001-no-identities]]).

Note the asymmetry of the two gates in the molecule grammar: (Paste) needs
*any* isomorphism of the shared boundary; (Atom) needs one that *respects
signs*. This asymmetry is exactly where the implementation has an open
question — below.

## Implementation

An atom is minted by **`Diagram::cell`** (`src/core/diagram.rs`,
[[core-diagram]]), dispatching on `CellData`: `Zero` for the point
(`cell0` *(internal)*), `Boundary { boundary_in, boundary_out }` for the
data of step 0 (`cell_n` → `cell_with_input_embedding` *(internal)*). The
three steps then read directly:

1. Roundness: `Diagram::parallelism` *(internal)* rejects either boundary
   failing `Diagram::is_round` → `Ogposet::is_round` (shape only; labels are
   never consulted — see [[boundary]] for why the check is correct exactly
   on molecules). This is the **only** place roundness is checked in the
   whole system; `Diagram::pastability` never asks for it.
2. Boundary agreement: `parallelism` extracts each argument's whole boundary
   sphere with `ogposet::boundary_traverse(Sign::Both, …)` — whose seeding
   (`build_stack_cell_n` *(internal)*) walks the input hemisphere first,
   then the output hemisphere — and compares the two canonical forms by
   table equality plus a positional label check. **Caveat:** this compares
   the spheres whole; the sign-restriction of step 2 is not checked
   explicitly, but inherited from the input-first traversal order. That
   inheritance is provably sound for generators of dimension $\le 3$ and
   open above — [[atom-gluing-sign-invariant]] is the full account.
3. Glue and cap: `pushout::pushout` amalgamates the two shapes along the
   shared sphere, and `build_cell_shape` *(internal)* appends the one new
   top cell with `faces_in` = the image of $U$'s top cells and `faces_out` =
   the image of $V$'s — a transcription of $\Delta^\mp \top := U_n, V_n$.
   Labels merge via `merge_pushout_labels`; the new top cell gets the
   generator's tag.

Atomicity is observable as `Diagram::is_cell` — the top input paste history
is a single `PasteTree::Leaf`, the runtime shadow of Lemma 3.3.10's "final
constructor was (Point) or (Atom)".

Generators live in the [[core-complex|Complex]]: `Complex::add_generator`
stores one classifier [[diagram]] per generator (debug-asserting
`classifier.top_label() == Some(&tag)`). The classifier's *shape* is this
atom; its *labelling* may identify cells — the loop `a : pt -> pt` labels
both 0-cells of the walking arrow with `pt` — which is how a regular shape
presents a non-regular [[directed-complex|type]].

## Related

[[molecule]] — the grammar this constructor belongs to · [[boundary]] —
roundness, the gate · [[diagram]] — the labelled value an atom classifier is ·
[[regular-directed-complex]] · [[directed-complex]] ·
[[oriented-graded-poset]] · [[0001-no-identities]] ·
[[0002-round-boundaries]] · [[atom-gluing-sign-invariant]]
