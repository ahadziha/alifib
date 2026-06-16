# alifib — conceptual notes

Running notes on the *mathematical and conceptual* picture behind alifib, as
explained by Amar (beyond what the code shows). Intended to ground the eventual
documentation rewrite. When something here conflicts with the code or the
papers, those win — but the *why* lives here.

Source of the motivating vision: Hadzihasanovic, *A blueprint for a programming
language founded on higher-dimensional rewriting* (2025, white paper) — the seed
of alifib. "The blueprint" below always means this. For the intellectual lineage
across Amar's papers (with full citations), see `THREADS.md` (same directory).

---

## Overarching vision (the spine)

**Directed complexes are the unifying language between the *semantics of
computation* and the *machines* (raw syntax / process theory) of computation.**
Same combinatorial material, two storeys:

- **Raw diagrammatic data (unit-less)** — the correct structure for
  *computational universes*: rewriting, machines, process theories. A
  "directed homotopy theory" that is fundamentally about computation, and so
  must be **unit-less** (a unit *represents* a do-nothing process; literally
  doing nothing is just the lower-dimensional cell). **alifib lives here.**
- **Semantics (with units)** — add a good algebra of units → higher categories,
  monoidal/cartesian categories, spaces, homotopy types, where *meaning* lives.
  **Diagrammatic sets / the 2024–25 papers live here.**

alifib is the ground floor made into a working tool. (Full detail and the
twelve supporting threads: `THREADS.md`.)

**Reconnecting the storeys — two-level type theories.** The intended bridge back
up: build *two-level type theories* on top of alifib, where alifib types are the
**cofibrant** level and certain types-with-structural-constructors are
**fibrant** for a chosen semantics (e.g. genuine higher categories with units).
This is how the unit-bearing upper storey is meant to be recovered *without*
compromising the raw computational base — and it is Amar's guess for how alifib
connects to the *formalisation* of higher category theory.

---

## Diagrams as syntax (generalising terms)

The project's entry point, and the on-ramp for a categorical audience:
**diagrams are a *syntax*, generalising classical *term* syntax.** Two rungs:

- **String diagrams** extend term syntax from *cartesian* monoidal categories to
  *non-cartesian* ones. Via the Lawvere correspondences —
  - term algebra = free cartesian monoidal category = free monoidal category +
    structural generators (copy, discard, swap),
  - presentation of an algebraic theory = cartesian monoidal category + equations
  — dropping the cartesian/structural generators leaves a genuinely more general
  syntax.
- **Pasting diagrams** extend string diagrams to *higher-dimensional* structures.
  (String diagrams are the Poincaré duals of 2-dimensional pasting diagrams with
  a single 0-cell.)

What makes the specific combinatorial model (Steiner → the book → computational
side with Kessler) usable *as a syntax*:
- **rigidity** — no non-trivial automorphisms, so a diagram has a canonical
  identity;
- **efficient, unique isomorphism** — decidable equality with canonical forms.

**Composition primitive.** Terms compose by **substitution** (grafting syntax
trees onto branches); diagrams compose by **pasting**. Substitution is the
special case of pasting with tree-like ("operadic") diagrams — so pasting is the
true generalisation. This is why the *term → diagram* move is the foundation of
the whole picture.

---

## The core idea (still current)

A **(small) type** is, from three perspectives at once:
1. a directed higher inductive type,
2. a finite **directed complex**,
3. a finitely presented higher-dimensional rewrite system.

**Terms are labelled molecules** — pasting diagrams in the type, labelled in its
generators. An (n+1)-dimensional term `t : u ⇒ v` *is* a rewrite sequence from
the n-term `u` to the n-term `v`. Computation is therefore **internal and
witnessed**: no external reduction relation; every computation has a concrete
term as its witness. "Each type is its own computational universe — its
generators create the space, the data, and the computations at once."

**No units / degeneracies**, by design: units would make rewriting
non-terminating. The semantic home is non-unital — what the blueprint called a
"combinatorial computad" and what is now a **directed complex**. (Expanded in
*What alifib is NOT*, below — it is a deeper design choice than just termination.)

The whole system rests on **one nontrivial operation: subdiagram search.**

---

## Computational transparency — internalising judgmental equality

The **core design principle** of alifib. The founding
principle of higher-dimensional rewriting: *a computation on n-dimensional data
**is** (n+1)-dimensional data* — the only model of computation with a truly
uniform representation of programs and executions.

The Curry–Howard gap it closes:
- terms = programs; well-typed terms = proofs of *program* correctness;
- but terms also carry a **judgmental equality**, presumed computable, whose
  computation is **external / meta-theoretical** — in practice left to compilers
  and interpreters. So program correctness can be internal, but *execution*
  correctness needs an outside oracle.

Replacing terms with diagrams lets the **computation rules live internally at
each type, as generators of higher-dimensional terms.** Hence:

> well-typed terms = **verified programs** · well-typed *higher* terms =
> **verified executions**.

"Computational traces are first-class generators." In this light the tetrahedron
(below) is *the syntactic side of Curry–Howard*.

---

## The four entities

alifib's minimal ontology — four entities, one composition primitive (pasting):

- **module** — for now, a set of types + inclusions of other modules (each
  included module gets a sub-namespace). *Future:* a module is itself a
  **complex** whose 0-cells are types and whose higher cells are "higher
  profunctors" between directed complexes (see *The global store*, below).
- **type** — a finite directed complex (+ names for diagrams and maps with it as
  codomain). Built by **successive cellular extension**: (1) an explicit named
  generator; (2) `include` another type as a sub-type; (3) `attach` another type
  along a map (a pushout). (2)/(3) grant a sub-namespace. One may start from an
  existing type and extend it, and make **ephemeral** extensions that are not
  stored.
- **map** — a (partial) functor between the ω-categories presented by two types,
  given by its action on generators (by *freeness*, with inference). Markable
  `total`; holes = pending images (still total); see *Maps in practice*. Composed
  with dotted expressions, `F.G`.
- **diagram** — generated from the generators by **pasting**: explicit `x #k y`,
  or *principal* pasting `x y` (juxtaposition; k = min(dim x, dim y) − 1). A
  diagram is also a **map of implicit (globe) shape**, so dotted application
  works (`F.G.x`); the destructors `.in` / `.out` are composition with globe
  boundary inclusions.

**Runtime.** To *run* an n-diagram is to apply (n+1)-dimensional rewrites by a
strategy (currently only greedy auto). The result is the **computational trace
itself** — not merely an output; `.out` recovers the output. (Computational
transparency in action: the execution is a term.)

---

## What alifib is NOT — raw diagrammatic data, by design

Directed complexes / alifib types can *present* higher-categorical structures,
but they are deliberately **rawer / more general**: raw diagrammatic data that
can also present *different* higher-dimensional structures (double and multiple
categories, higher operads, …). In particular they have **no structural
homotopies — no units** (nor unitors, etc.). Two reasons this is a feature:

1. **Termination.** In the abstract-machine / rewriting use case, any unit or
   equivalence breaks naive termination (a unit "loops"). Concretely, the
   encoding of **term rewrite systems** is a *laxified* cartesian monoidal
   category: the structural equivalences (copy/discard/swap coherence) are turned
   into **directed 3-cells**. (This is the concrete answer to "how do the
   examples put units in by hand" — they don't add units; they *direct* the
   would-be structural equivalences.)
2. **Generality.** Which diagrams are *composable* (reduce to a single cell)
   depends on what structural cells exist; the **same diagrammatic data** is
   valid in, e.g., monoidal categories and polycategories — what differs is
   composability. Leaving this open makes the language broadly applicable across
   higher and categorical algebra.

The way *back* to unit-bearing semantics is the **two-level type theory** of the
[[#overarching-vision]] (alifib cofibrant base + fibrant structured types).

---

## Semantics — current vocabulary

- A **type** = a finite **directed complex** (a finite presheaf on the category
  of *atoms and embeddings*; several equivalent definitions exist), plus
  annotations: named diagrams, indices, maps pointing to it.
  Theory: Chanavat & Hadzihasanovic, *Semi-strictification of (∞,n)-categories*
  (2025).
  ⚠️ **Not necessarily *regular*.** Precise picture:
  - oriented graded posets ⊃ **RDCs** (regular directed complexes) ⊃ molecules ⊃
    atoms (= molecules with a top element);
  - a **directed complex** is a *presheaf* on the category of (atoms, embeddings)
    — a *different kind of object* from an RDC (which is an OGP).
  - **Correspondence (justifies the name):** RDCs ↔ directed complexes that are
    "regular" in the CW sense, i.e. whose cells' *classifying maps are
    embeddings*. The full subcategory of regular directed complexes is equivalent
    to (RDCs + *local embeddings*). Hence we may loosely conflate them.
  - A **type = a directed complex, not necessarily regular.** Counterexample: one
    vertex + one directed loop — a fine directed complex, but *not* regular (the
    loop's classifying map identifies its two endpoints → not an embedding).
  - **Roundness ≠ regularity.** Roundness/sphericality constrains the *shape* of
    the pasting diagrams in a cell's boundary (always — cells are atoms). It does
    **not** force regularity: *rigid identifications* in the boundary (classifying
    map not an embedding) are allowed → non-regular but valid. So
    [[project_maps_may_lower_dimension]]'s "real cell constraint is roundness" is
    about boundary *shape*, and does not make types regular.
  - Soundness (Amar's sense) = realised by a CW complex with **one cell per
    generator** (general); "one *ball* per cell" is the regular case only.
  See [[project_type_is_directed_complex_not_regular]].
- **Two connections to (∞,n)-categories:**
  1. every type `X` carries a strict ω-category **`Mol/X`** of its pasting
     diagrams; up to dimension 3, `Mol/X` is at once a *strict* and a *weak*
     computad (Chanavat 2026).
  2. **marked directed complexes** carry a model of (∞,n)-categories, *known or
     believed* equivalent to the standard models **up to n = 3** (Chanavat–H.
     2025). Known: n=0 (proved), n=1 (unpublished); believed: n=2,3. Past n=3 is
     the higher-exchange divergence (see [[THREADS]] thread 9).
  `Mol/X` is the notation for that strict ω-category of pasting diagrams; a
  *diagram in `X`* is a cell of `Mol/X`, and a *map* `X → Y` is a partial functor
  `Mol/X → Mol/Y`. Semi-strictification is *where directed complexes are
  mathematically developed*, not needed for the weaker "alifib proofs have
  higher-categorical semantics" claim.
- A **total map** between types `X` and `Y` = a **strict functor** between their
  strict ω-categories of molecules ("stricter" ω-categories).
  Theory: Chanavat, *Homotopy theory of stricter n-categories* (2025).

  **Maps in practice — holes ≠ partiality** (interpreter-level, three states for a
  map's action on a generator):
  - **total** — defined on every generator, image supplied;
  - **has holes** — defined on the generator but its *image is pending*; this
    still **passes** the totality check (the generator is covered) and holes are
    what you fill interactively in the proof assistant;
  - **partial / undefined** — not defined on some generator at all; *this* is
    what fails totality.

  A map can be marked `total` at definition time and the interpreter flags it if
  it is genuinely partial (undefined somewhere) — holes do not make it partial.
  Maps are defined by their action on generators (by *freeness*), with some
  inference; composed with dotted expressions, e.g. `F.G`.

  **Categorical home (precise).** alifib maps do **not** live in the presheaf
  category. A map `X → Y` is a morphism in the **Kleisli category of a composite
  "pasting diagrams" + "maybe" monad** (maybe = partiality): it sends each
  generator (atom) of `X` to a *pasting diagram* in `Y`, or to nothing (partial).
  Partiality = the maybe-`Nothing`; a *hole* is a present-but-pending image, not
  `Nothing`. The "total map = strict functor between the ω-categories of
  molecules" reading is the *semantic interpretation* of a total such map.

---

## The global store: three layers

The interpreter's global store is stratified into three stores:

1. **modules**
2. **global types**
3. **global cells**

The layers nest by "generators of one layer are inhabitants of the layer below":

- A **global cell** is a **pure name** — *no semantics attached*, it refers to
  nothing. Global cells are the generators of types' complexes.
- A **global type** ("small type") points to a **complex whose generators are
  global cells**. Currently these complexes may have higher-dimensional cells.
  The generators (global cells) carry no semantics — pure names.
- A **module** points to a **module complex**: a **complex whose generators are
  global types**. Unlike global cells, these generators *do* carry semantics —
  namely the global types themselves (each being a complex).

So the same combinatorial gadget (a directed complex) appears at two levels,
distinguished only by what its generators mean: cells-as-pure-names (a type) vs
types-as-complexes (a module).

### Module complexes — what they are, and the role of generator dimension

A **module complex** is the complex a module points to: generators = global
types. The semantics of a generator is

> **a complex together with a total map (functor) to a molecule — the
> generator's own shape as a generator.**

This single definition unifies the whole picture by dimension:

- **0-dimensional generator.** Its shape is the **point**. There is always a
  *unique* total map to the point from any complex, so "complex + total map to
  the point" degenerates to just "a complex" — i.e. a plain global type.
  **Currently every global type is a 0-dimensional generator of its module
  complex**, so module complexes have no higher structure yet and a module is,
  for now, barely more than a *set of named types*.

- **Higher-dimensional generator.** This is where **cographs (and higher
  cographs)** will live — as the higher generators of module complexes
  (**not yet implemented**). Such a generator's shape is some molecule `U`, and
  its semantics is a complex fibred over `U` (complex + total map to `U`). A
  map `X ⇒ Y` is the 1-dimensional case: shape = the **arrow**, semantics =
  a complex with a total map to the arrow — exactly the blueprint's
  "fibred over the arrow" cograph.

This also reconciles the two map descriptions that earlier looked separate:
"total map = strict functor to a molecule" *is* the semantics of a module-complex
generator, with the molecule being its shape.

**The guiding analogy (profunctors).** The relationship between a *general*
1-cell of a module complex and the 1-cells that come from *maps* mirrors the
relationship between profunctors and functors:

- the **cograph (collage) of a category over the arrow encodes a profunctor**
  `X ⇸ Y`; a general 1-cell of a module complex is like a general profunctor;
- every **functor induces a (representable) profunctor**, but **not every
  profunctor is representable**;
- the cographs that come from **(total) maps are like the representable
  profunctors** — the restricted sub-class.

So: map ↔ functor ↔ representable; general 1-cell ↔ general profunctor. This is
the right *picture*, but kept deliberately informal — Amar does **not** expect
the representation of a map as a 1-cell to be a **primitive of the language**,
so we should not over-engineer docs around it.

---

## Supersession map (blueprint → now)

The blueprint is the clearest statement of *motivation*, but its specifics have
moved. Do **not** anchor docs on these:

| Blueprint                                   | Now                                                            |
|---------------------------------------------|---------------------------------------------------------------|
| "combinatorial computad"                    | **directed complex** (just a rename)                          |
| maps = cographs, action *computed by rewriting* | **NOT how maps work.** See below.                         |
| pseudo-syntax (`=>`, `>>`, `t <{0,2,3} m`)  | **completely superseded** — read real syntax from code/examples |

### Maps: the important correction

The blueprint's "cograph / fibred-over-the-arrow / map-as-simulation-computed-
by-rewriting" picture is **the seed for a future feature, not current maps.**

- That picture is the seed of **higher generators in module complexes** —
  **not yet implemented**.
- **Current maps** admit a cograph *representation*, but are **more restrictive
  than general 1-cells in module complexes**. Their role is **translational**:
  a map turns a diagram of one type into a diagram of another type **at once**,
  *without* a rewrite-style computation.

(For what a "module complex" is and where maps/higher generators sit, see
"The global store" above — resolved.)

---

## Open questions / threads to follow up

- (Resolved as a *picture*, see above: map : general 1-cell :: representable
  profunctor : general profunctor. Not a language primitive, so left informal.)
- How the **examples** put units in "by hand" — *partially answered* (see *What
  alifib is NOT*): the TRS encoding directs would-be structural equivalences into
  directed 3-cells rather than adding units. Still worth cataloguing per example
  which generators encode unitors in `Bicategory` / `Monoidal`.
- The **higher-interchange ("stricter")** equations beyond dim 3: how they show
  up combinatorially and why standard polygraph-based models miss them.
