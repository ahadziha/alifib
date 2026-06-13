# Threads running through Amar's work → alifib

Synthesis from reading the introductions in order of writing:

- Hadzihasanovic & Kessler, *Data Structures for Topologically Sound
  Higher-Dimensional Diagram Rewriting* (2022)
- Hadzihasanovic & Kessler, *Higher-Dimensional Subdiagram Matching* (2023)
- Hadzihasanovic, *Combinatorics of higher-categorical diagrams* (2024, book)
- Chanavat & Hadzihasanovic, *Diagrammatic sets as a model of homotopy types* (2024)
- Chanavat & Hadzihasanovic, *Equivalences in diagrammatic sets* (2024)
- Chanavat & Hadzihasanovic, *Model structures for diagrammatic (∞,n)-categories* (2024)
- Chanavat & Hadzihasanovic, *Semi-strictification of (∞,n)-categories* (2025)
- Chanavat, *Homotopy theory of stricter n-categories* (2025)

Inline below, these are cited author–year, with a short-title tag where a year
is shared.

These are the conceptual through-lines, with where each appears and how it
lands in alifib — the "intellectual landscape" that the code alone can't convey.

---

## The overarching vision (keystone) {#overarching-vision}

> **Directed complexes are the unifying language between the *semantics of
> computation* and the *machines* — the raw syntax and process theory — of
> computation.**

Two storeys built from the same combinatorial material:

- **Ground floor — raw diagrammatic data (unit-less).** The correct structure
  for *computational universes*: rewriting, abstract machines, process theories.
  There is a **directed homotopy theory** which is fundamentally about
  computation, and *because of that* it must be **unit-less** — a unit is a
  *representation of a do-nothing process*, whereas literally doing nothing is
  just the lower-dimensional cell. This is where **alifib** lives.
- **Upper floor — semantics (with units).** Add a good algebra of units and you
  get **higher categories, monoidal/cartesian categories, spaces, homotopy
  types** — where the *meaning* of computations lives. This is where
  **diagrammatic sets** and the 2024–25 papers live.

The bet is that directed complexes are the single language spanning both: from
the semantic models of computation (monoidal categories, cartesian categories,
and more) down to the raw machines and syntax. alifib is the ground floor made
into a working tool. See thread 7 for the units detail, thread 8 for the
philosophical stance this rests on.

**Sharpest articulation:** the move from *terms* to *diagrams* buys
**computational transparency** — computation rules are not meta-theoretic
(invented by compiler engineers) but **extra generators on a type**. Hence not
only *well-typed terms as verified programs* but *well-typed (higher) terms as
verified executions*. Where mainstream work gives term-based syntax
higher-categorical **semantics**, alifib programs with higher-categorical
**syntax** directly.

**On-ramp — diagrams as syntax.** The way in for a categorical audience: diagrams
are a *syntax* generalising terms. String diagrams extend term syntax from
cartesian to non-cartesian monoidal categories (Lawvere correspondences); pasting
diagrams extend string diagrams to higher dimensions; substitution is the
operadic special case of pasting. The model is "syntax-like" — rigid + decidable
unique iso. (Full treatment: `CONCEPTS.md` → *Diagrams as syntax*.)

**Reconnecting the storeys (the bridge to formalisation).** The upper,
unit-bearing storey is meant to be recovered via **two-level type theories**:
alifib types as the *cofibrant* level, structure-bearing types as *fibrant* for a
chosen higher-categorical semantics. This — not the semi-strictification result
itself — is Amar's guess for how alifib meets the formalisation community.

---

## 1. Topological soundness — the non-negotiable

The recurring criterion, present from the start. Higher-dimensional rewriting
rests on *rewrites as directed homotopies*; the demand is a **functorial
interpretation of rewrite systems as cell complexes, and of rewrites as
homotopies**. A framework is acceptable only if diagrams provably present
genuine topological cell complexes.

This is the stick that beats **polygraphs**: it is only conjectural that
associative/strict polygraphal models are topologically sound. Regular directed
complexes *are* sound — each presents a regular CW complex, one cell per
generator. → In alifib this is why types are directed complexes, not polygraphs.

## 2. The triality: topology ↔ higher algebra ↔ computation

Directed cell complexes are the common universe where all three meet (Burroni
brought *rewriting* into the older topology/category mix). Rewrite systems
"situate objects traditionally associated with syntactic and quantitative
aspects of computation in the same universe as objects associated with semantic
and logical aspects." → alifib is the computational corner taken seriously as a
*language*.

## 3. Uniformity of data and computations  ← the seed of alifib's core

The distinctive computational pay-off, stated cleanly in Hadzihasanovic–Kessler
2023 (*Subdiagram Matching*): in
most models, computations are objects of *different nature* than the data
(sequences of configurations vs terms). In higher-dimensional rewriting, **data
is a diagram of n-cells and a computation is a diagram of (n+1)-cells — itself
data one dimension up.** No external reference machine; computations are
manipulable as data. Also: a rewrite system presents *the space in which
computations happen* — parallelism, stack-vs-free access become topological
features, not external constraints.

→ This is *exactly* alifib's "computations are first-class, witnessed by the
same terms as data" / "each type is its own computational universe." The
blueprint inherits it verbatim.

## 4. The categorical point of view — morphisms of shapes (THE book's innovation)

The "single main technical innovation" of the book: study not just diagram
shapes but their **morphisms**, conspicuously absent from the classical
literature. Pay-offs:
- **Dissolves the acyclicity restriction.** Earlier approaches forbid simple
  cyclic shapes (already in dim 1) and common composable shapes (from dim 3)
  because, lacking good morphisms, they only consider "subshapes" and need these
  to form an ω-category, which only acyclicity guarantees. Analogy: *linear
  subgraphs* of a digraph form a category only if acyclic, but *paths* always
  do. General morphisms = paths.
- **Restores pasting as a universal construction** (a pushout of inclusions);
  the ω-category equations hold up to unique iso because pastings compute
  colimit cones.
- **Two natural notions:** `maps` and `comaps`, inducing strict functors of
  ω-categories **covariantly** / **contravariantly**.
- **Ternary factorisation of maps:** final maps (collapse — dual to
  units/degeneracies) · surjective local embeddings (rigid identification) ·
  inclusions (embed — dual to faces). Comaps are dual to **subdivisions**
  (a restricted form of composition).

→ This trichotomy **embeddings = faces · collapses = units · subdivisions =
composition** is the architecture that governs all the later models, and it is
the precise frame for alifib's notion of map (a [[CONCEPTS]] total map = strict
functor; translational; cf. profunctor analogy).

## 5. Synthetic, not analytic — molecules defined inductively

Molecules are generated by **constructors** (point; pasting `U #k V`; atom
`U ⇒ V`), not characterised by axioms a poset must satisfy. Stated motivation:
this is "the obvious choice if one is interested in higher-dimensional rewriting
as a computational tool, since the definition of well-formed cell shapes in
terms of constructors translates smoothly into constructors for a rewrite
system." Developed *alongside* rewalt.

→ Directly underwrites alifib's terms-as-molecules data structure and the whole
"a type is a finitely presented rewrite system" view.

## 6. Roundness — the central restriction, and its price

`round` = boundary is, topologically, a sphere split into two ball-halves; an
atom's input/output boundaries must be round. **Roundness is the key to
topological soundness** (cells are genuine balls). Same restriction as Henry's
*regular polygraphs*. But roundness **costs expressiveness**: many natural
pasting shapes aren't round.

The fork:
- **Diagrammatic sets** pay for roundness with a rich algebra of **weak units**
  that "pad" any diagram until round.
- **alifib** refuses units (they break termination), so handles the roundness
  price differently / puts units in by hand. → This is the single sharpest
  technical difference between the homotopy-theory line and alifib.

## 7. Units are *semantics*; the unit-less layer is more fundamental

⚠️ This is the keystone — read [[#overarching-vision]] below; the earlier
"alifib omits units because they cause non-termination" framing was too shallow.

The diagrammatic-sets quartet (homotopy → equivalences → model → semistrict) is,
to a large extent, *about units*: degeneracies, unitors, invertors; the
**inflate monad**; coinductive equivalences built from units; the homotopy
hypothesis. A **good algebra of units is precisely what directed complexes need
in order to model higher categories** — and, in particular, homotopy types.

Amar's deeper claim: beneath that sits a more fundamental layer of **raw
diagrammatic data**, which is the correct structure for *computational
universes*. A **unit is a *representation of a process that does nothing* — not
literally a process that does nothing.** "Literally doing nothing" already
exists: it is the lower-dimensional cell itself. So a unit is a semantic
embellishment (it lets an n-diagram stand in as an (n+1)-cell), and the raw
computational layer must be **unit-less** to be honest about processes.

Non-termination is then a *symptom*, not the reason: because a unit witnesses a
do-nothing process, admitting units immediately makes the rewrite system
non-terminating. alifib = the unit-less layer = "non-unital diagrammatic sets" =
**combinatorial computads** = (now) directed complexes. The homotopy papers are
"context, not the thing" because they live one storey up, where *semantics*
live; alifib lives on the raw-data ground floor, where *machines* live.

**Two reasons the unit-less choice is a feature (from the STORY):**
1. *Termination* — units loop. Concretely, the **TRS encoding is a laxified
   cartesian monoidal category**: structural equivalences (copy/discard/swap
   coherence) become **directed 3-cells**. This is the concrete "units by hand" —
   not added units but *directed* would-be equivalences (answers the old open
   question).
2. *Generality* — the **same diagrammatic data presents different structures**;
   what differs is *composability* (e.g. monoidal vs polycategory: same diagrams,
   different composable ones). Leaving this free widens applicability (double /
   multiple categories, operads, …).

The bridge back to unit-bearing semantics is the **two-level type theory** (see
the keystone vision above).

## 8. Computationalist vs homotopist (existentialist vs essentialist)

From Chanavat–Hadzihasanovic 2024 (*Equivalences*), the cleanest philosophical
statement: *what comes first,
higher categories or higher groupoids?* The **computationalist** position —
computation is directed and potentially irreversible, hence more fundamental
than reversible homotopy/equality; higher categories precede groupoids. An
equivalence is a cell that *behaves* like one (existentialist), not one that *is*
a distinguished homotopy (essentialist).

→ alifib is the computationalist position made into a tool: directed,
irreversible computation as primary; no presupposed space.

## 9. Higher exchange / "stricter" — the claim beyond dimension 3

The boldest thread, surfacing in Chanavat–Hadzihasanovic 2024 (*Model
structures*) and central to Chanavat 2025 (*Stricter*):
combinatorial pasting of molecules satisfies **extra equations** — forms of
"interchange in higher codimension," not reducible to the codimension-1 case —
that are **not provable in the algebra of strict n-categories**, first appearing
at **dimension 4**. These equations are **topologically sound** (satisfied by
pasting of actual cells).

Position: this is evidence that strict n-categories are *incomplete* for pasting.
**Stricter n-categories** = strict + these extra axioms; they have a pasting
theorem for *all* regular directed complexes, and `Strict = Stricter` for n ≤ 3.
Reflective subcategory of strict ω-Cat.

**The honest framing puts the agreement first, the divergence second.** The
*positive* equivalence results come first — the diagrammatic model is
**equivalent to all other models** for `(∞,0)` (proved) and `(∞,1)`
(unpublished); **Chanavat is actively working on the `(∞,2)` and `(∞,3)`
proofs** (~99.9% confidence). The difference only kicks in at **`(∞,4)`**, and
so far very few people care about `(∞,4)`-categories proper. So the
higher-exchange story is the *interesting tail* — read after a solid record of
agreement, not as a provocation.

→ alifib semantics: a **total map = strict functor between the stricter
ω-categories of molecules** over the two directed complexes. ([[CONCEPTS]])

## 10. Freeness / "define a functor on generators"  ← alifib's map-definition UX

The *freeness condition* (polygraph sense): a complex `P` should be freely
generated by its elements, so a strict functor `P → C` is **exactly** a choice
of one cell `c_x ∈ C` per generator `x ∈ P` (with boundary compatibility).
Stricter n-categories are engineered precisely so this holds for *every* regular
directed complex (not only acyclic ones).

→ This is the mathematical content behind the blueprint's "to define a map out
of a type is simply to define its action on its generators in functional style,"
and the proof-assistant workflow that fills in generator images one dimension at
a time.

## 11. The computational programme — one structure, one operation

From 2022/2023 and the book's implementation footnotes:
- **ogposet** data structure; isomorphism of molecules decided in `O(n³ log n)`,
  giving canonical forms / unique representation.
- **subdiagram matching** = the basic machine step; feasible (low-degree poly)
  **up to dimension 3**, open/superpolynomial beyond — but dim >3 rewriting is
  rare, so optimise low dimensions aggressively (dim 1 ≈ string matching).
- The "reasonable machine" question: is constant-cost-per-rewrite a reasonable
  cost model? ⇔ is subdiagram matching feasible.
- **Locality**: face-poset representation lets a local rewrite stay local, vs
  the "global duplication" forced by associative n-categories' cubical tilings →
  better parallelisability.
- Blueprint's distillation: the interpreter deals with **one data structure**
  and performs **one nontrivial operation — subdiagram search**.

## 12. Steiner as the taproot

*The algebra of directed complexes* (Steiner 1993) is "by quite a distance the
single most important influence" — molecules, frame dimension, splitness →
frame-acyclicity. Roundness comes from Steiner 1998. The book is in large part
"an expansion and commentary" on Steiner 1993. The lineage worth keeping in
view: Street → Steiner → Hadzihasanovic, with the synthetic/morphism-aware turn
as the new contribution.

---

## How the threads converge on alifib (one paragraph)

Topological soundness (1) forces roundness (6), which the computational reading
(2,3,8) wants *without* the units (7) that the homotopy-theoretic reading needs;
the morphism-aware, synthetic theory of molecules (4,5) supplies the data
structure and the one operation, subdiagram search (11), on which an
implementation can stand; freeness (10) makes "define a map = act on generators"
literally true; and the semantics of types and maps is pinned by directed
complexes and the stricter ω-categories (9) that capture the higher-exchange
laws real pasting obeys. alifib is the computationalist distillate: keep
soundness and the diagrammatic language, drop units, and build the interpreter
around the single combinatorial operation.
