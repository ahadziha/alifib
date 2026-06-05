# The idea behind alifib

*Types as computational universes.*

This is the conceptual tour: what alifib is, why it is built the way it is, and
the ideas it grows out of. It assumes you like programming languages
and have a general fondness for mathematics; it assumes nothing about higher
category theory. For the practical reference — installation, syntax, commands —
see the [README](../README.md).

---

## From terms to diagrams

Nearly every programming language writes a program as a **term**: a tree of
symbols, `f(g(x), y)`, grown by slotting smaller terms into the branches of
larger ones. Computation is something that happens *to* a term — a reduction
relation, a sequence of rewrites — and the rules governing it live *outside* the
language, in the interpreter or the compiler.

alifib starts from a different syntax. The generalisation of a term is not a
larger tree but a **diagram**, and the step is taken in two stages, each with a
long history in category theory.

In a setting where data can be freely copied and discarded, terms-as-trees are
exactly right. But much of computation is not like that — a resource consumed
cannot be reused, a message sent cannot be un-sent — and there the natural syntax
is the **string diagram**: boxes joined by wires, each wire a value flowing from
one box into the next. String diagrams are the language of monoidal categories,
and they are more expressive than trees precisely because they drop the silent
assumption that anything may be duplicated for free.

**Pasting diagrams** take the second step, to *higher dimensions*: not only boxes
and wires, but cells of dimension 0, 1, 2, 3, and up, each bounded by an *input*
face and an *output* face. A point; an arrow between points; a filled region
between two arrows; a solid between two such regions. And each level is a *rewrite*
of the one below — an arrow rewrites its source point into its target point, a
2-cell rewrites one path of arrows into another — so the whole tower is a single
object.

That is the entire ontology of the language: one kind of thing, the diagram, and
one nontrivial operation on it, finding a diagram sitting inside another.
Everything else is built from these. (Diagrams also have an inescapably geometric
character — we return to that below — but first of all they are *syntax*.)

## The one idea: a computation is data, one dimension up

Here is the principle the whole language turns on, the founding observation of
*higher-dimensional rewriting*:

> A **computation** on *n*-dimensional data **is** itself *(n+1)*-dimensional
> data.

In most models of computation a program and a run of that program are different
kinds of object — a piece of syntax versus a sequence of machine configurations —
and there is no way to treat the second as data of the first kind without encoding
it in some external reference machine, to which the same complaint then applies.
Higher-dimensional rewriting is the rare model where the two are *uniform*: your
data is a diagram of *n*-cells, and a computation acting on it is a diagram of
*(n+1)*-cells — which is just data again, one storey up, ready to be acted on in
turn.

So each alifib type is, in a precise sense, **its own universe of computation**:
its generators create, at one stroke, the space in which computation happens, the
data that lives in that space, and the computations that move that data around.
Nothing is imported from outside.

## Computational transparency

This is alifib's reason to exist; the term for it is **computational
transparency.**

Recall the Curry–Howard correspondence, the idea that *programs are proofs*: a
well-typed term is, at the same time, a proof that the program it denotes is
correctly formed. But it speaks only about the *program*. A term also carries an
equational theory — a notion of when two terms are "the same", computed by
reduction — and that theory, the very mechanism of *running* the program, is
meta-theoretical: assumed to exist, assumed to be computable, and then handed off
to the compiler engineers. You can certify the program inside the language; to
certify its *execution* you must trust something outside.

With diagrams that boundary disappears. Because a computation is just data one
dimension up, the rules by which a type computes are no longer an external decree
— they are **extra generators of the type itself**, cells you write down
alongside the ordinary ones. Running a program is then literally *building a
higher diagram*, and what you get back is not a bare answer but the **entire
computational trace**, a term you can inspect, replay, and reason about. (If you
only wanted the answer, you ask the trace for its output boundary.)

> Well-typed terms are verified **programs**.
> Well-typed *higher* terms are verified **executions**.

This is the inversion at the centre of the project. A great deal of recent work
aims to give ordinary, term-based languages a higher-categorical *semantics* — to
explain, after the fact, what their programs *mean* in some richer mathematical
world. alifib goes the other way. It puts higher-categorical structure into the
*syntax*, at the very bottom, and lets meaning take care of itself.

## What a type is

Under the hood, an alifib type is a **finite directed complex**: a finite set of
cells ranked by dimension, each carrying a partition of its boundary into an
*input* half and an *output* half — an *oriented graded poset*, to give the object
its technical name, with the well-formed shapes, the *molecules*, cut out by a
short list of inductive rules. The reader need not retain any of this.

What makes the construction cohere is *topological soundness* — every such complex
is realised by an honest geometric cell complex, one genuine ball for each
generator, with nothing torn or doubled over. The combinatorics never drift away
from the geometry they describe. (This is what separates directed complexes from
the older notion of *polygraph*; securing it turns on a condition called
*roundness* — that the input and output halves of every cell's boundary be
themselves balls. Above dimension three the diagrams then begin to obey coherence
laws that the classical algebra of higher categories cannot derive: one of the
stranger things the theory has turned up, and nothing that needs following here.)

The reward for that discipline is that a single directed complex can be read,
faithfully, in four different ways at once — the four faces of one structure:

- a **cell complex** — a piece of (directed) topology;
- a presentation of a **higher category** — a vocabulary of composable cells;
- a **higher-dimensional rewrite system** — generators that rewrite data;
- a **directed higher inductive type** — a type defined by its constructors,
  including constructors *between* constructors.

Four vocabularies, one object: a definition you write as a rewrite system is, at
the same time and for nothing extra, a presentation of a higher category and a
description of a space.

## What you can do with it

Each face of that tetrahedron is a way to use the language.

- **Reason in higher-categorical structures.** Define a monoidal category, a
  bicategory, an algebraic theory; state an equation as a *directed* cell; and
  *prove* it by building the proof diagram interactively, the proof assistant
  offering you the available rewrites at each step. The classic Eckmann–Hilton
  argument — that two ways of composing commute — becomes a diagram you assemble
  by hand (`examples/EckmannHilton.ali`).
- **Specify abstract machines.** A Turing machine, an automaton, a term-rewriting
  system is just a type whose higher generators are its transition rules; *running*
  it produces the execution as a witnessed trace. And since parsing, evaluation,
  and execution are all the same kind of diagram, even *parsing is functorial*
  (`examples/TM.ali`, `examples/BinaryNat.ali`).
- **Build spaces and measure them.** Since a type *is* a cell complex, you can
  describe one explicitly and ask for its **homology** — its holes, counted by
  dimension — directly in the REPL (`examples/Delta_complexes.ali`).

## What it leaves out, on purpose

A reader from category theory will notice something missing: alifib types have no
**units** — no "identity" cells, no structural do-nothing morphisms. This is not
an oversight; it is the keystone of the design.

A unit is a *representation of a process that does nothing*. But the process that
does nothing already exists — it is simply the lower-dimensional cell itself,
sitting there unbothered. A unit re-packages that non-event as a positive
higher-dimensional thing, and the moment you allow it, computation can spin in
place forever: a rewrite that does nothing can always fire again. Units are a
*semantic* convenience, indispensable when the goal is to model spaces or homotopy
types: restore them — and, in their train, the unitors, invertors, and coherences
that a full theory of equivalences brings with it — and the same diagrams come to
model weak higher categories and homotopy types. That is the subject of the floor
above; on the raw, computational floor where alifib lives, units are exactly the
wrong thing.

Refusing them buys generality. The *same* diagrammatic data can
present quite different higher structures — monoidal categories, polycategories,
multiple categories, operads — and what distinguishes them is not the diagrams but
which diagrams count as *composable*. By staying agnostic, alifib remains a
substrate for all of them at once. The plan for recovering the unit-bearing,
semantic world when you want it is a *two-level* arrangement: alifib types as the
raw, flexible base, with structured types layered on top for a chosen meaning.

## Where it comes from

The combinatorics of directed complexes and molecules are developed in Amar
Hadzihasanovic's book *Combinatorics of higher-categorical diagrams*, itself an
expansion of far-sighted work by Richard Steiner in the early 1990s, in a lineage
running back through Ross Street to the founding days of higher category theory.
The computational side — the data structures, and the algorithm at the language's
core that decides when one diagram sits inside another — was developed with Diana
Kessler. The higher-categorical semantics, including a model of
(∞, n)-categories carried by directed complexes (and equivalent, where it has been
checked, to the established models), is joint work with Clémence Chanavat. The
intuition that structures from higher category theory belong on the *syntactic*
side of computation is one alifib inherits from the polygraphic rewriting
tradition.

alifib itself is built as part of [ARIA](https://www.aria.org.uk/)'s *Safeguarded
AI* programme — whose interest in *guaranteed executions* is exactly what
computational transparency offers — and the interpreter and proof assistant are
joint work with Alex Kavvos, with contributions from Wessel de Weijer.

## A taste

Programs and the rules that run them are written in the same place, as generators
of one type. Take a point `pt`, and two arrows `a`, `b` from the point to itself —
so that strings of `a`s and `b`s are one-dimensional diagrams, the data. Then add
a single rewrite rule, which is nothing but a two-dimensional generator:

```ali
@Type
Letters <<= {
    pt,
    a: pt -> pt,
    b: pt -> pt,

    (* a rewrite rule is just a 2-dimensional generator *)
    swap: a b -> b a
}
```

`swap` rewrites the pattern `a b`, wherever it occurs inside a string, into
`b a`. There is no separate notion of "evaluation rule": the rule and the data it
acts on are generators of the same type, one dimension apart. That is the core
idea, in miniature. (For substantial cases — term rewriting, automata, arithmetic
— see `examples/TRS.ali` and `examples/BinaryNat.ali`.)

---

*Open <http://compose.ee/alifib> to try the examples in your browser; the
[interactive guide](INTERACTIVE.md) explains the proof-assistant commands.*
