# The Shape of Concurrency

A four-act `alifib` demo that computes cellular homology of polygraphic
rewriting systems and uses the results to make a point no 1-D term
rewriter (Maude, ELAN, CafeOBJ, Stratego) can make.

```
bash scripts/smoke.sh
```

## Setup in one paragraph

A polygraph (Burroni 1993) is a presentation of a higher category by
generators in each dimension: 0-cells (objects), 1-cells (morphisms),
2-cells (rewrite rules), 3-cells (rewrites between rewrites), and so on.
Equivalently, it is a CW complex whose cells happen to come with direction.
Every rewriting system therefore has a topology, and `alifib homology`
computes the integer homology of that topology directly — via the chain
complex of free abelian groups on the generators, reduced by Smith Normal
Form. None of this is accessible to a rewriter that only sees 1-D terms
and reduction sequences.

## The four acts

The demo file is [`examples/ShapeOfConcurrency.ali`](../examples/ShapeOfConcurrency.ali).
Each act declares a small polygraph, then `alifib` computes its homology.

### I. Two concurrent threads weave a torus

```
Pair <<= {
    pt,
    a : pt -> pt,
    b : pt -> pt,
    comm : a b -> b a
}
```

Two independent threads, one atomic action each, and a 2-cell `comm`
that witnesses their commutativity — the Mazurkiewicz independence
square. The chain complex:

```
C_0 = Z (pt)         d_1(a) = d_1(b) = 0  (both : pt → pt)
C_1 = Z²  (a, b)     d_2(comm) = (b + a) − (a + b) = 0   (abelianised)
C_2 = Z   (comm)
```

Homology: `H_0 = Z, H_1 = Z², H_2 = Z`. That is the integer homology of
the 2-torus **T²** — the classifying space of Z×Z — and from the
concurrency angle it is the space of equivalent interleavings of two
independent actions.

### II. Three threads, naively

```
Triple <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab, comm_ac, comm_bc   (three commutation 2-cells)
}
```

Expected (if we had the 3-torus): `H_0=Z, H_1=Z³, H_2=Z³, H_3=Z, χ=0`.
What `alifib` reports: `H_0=Z, H_1=Z³, H_2=Z³, χ=1`. **No `H_3`, and the
Euler characteristic is off by one.** The presentation is complete at
dimension 2 — every critical pair of 1-cells is joined — but the
topology has a 3-dimensional hole.

### III. The Zamolodchikov tetrahedron

```
TripleCoh <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab, comm_ac, comm_bc,
    zamo : (comm_ab c)(b comm_ac)(comm_bc a)
         -> (a comm_bc)(comm_ac b)(c comm_ab)
}
```

There are two compositions of three 2-cells taking `a b c` to `c b a`
(the two sides of the hexagon/tetrahedron). `zamo` is a 3-cell that
identifies them. Adding this single 3-cell gives

```
H_0 = Z, H_1 = Z³, H_2 = Z³, H_3 = Z, χ = 0
```

i.e. `H_*(T³)`. **The coherence cell was not an aesthetic choice.** It
is forced by algebraic topology: any coherent extension must identify
those two pastings, and the 3-cell is the extension witness. This is
the concrete content of Squier's homological finiteness theorem
(Squier–Otto–Kobayashi 1994; Lafont–Métayer 2009;
Guiraud–Malbos–Mimram 2013) translated into mechanically checkable
output.

### IV. Torsion

```
Torsion <<= {
    pt,
    a : pt -> pt,
    double : a a -> a a a a
}
```

The 2-cell sends the abelianised generator `a` to `4a − 2a = 2a`. The
cokernel is `Z/2`. `alifib` reports `H_0=Z, H_1=Z/2, H_2=0`,
demonstrating that its Smith Normal Form computation detects torsion,
not just free ranks. A 1-D term rewriter would see `a²→a⁴` as
"non-terminating modulo power"; a polygraphic tool sees a Moore space.

## Why this is beyond 1-D term rewriting

Maude and its cousins are optimised around a different question: given
a term rewriting system, what terms reach what other terms, and under
what strategies? They are extraordinarily good at it — matching modulo
AC, narrowing, LTL model checking, and so on. None of that machinery
*sees* the underlying rewriting complex as a space.

The polygraphic viewpoint sees rewriting as CW geometry. Two rewrites
that share a generator are 2-cells that share a 1-cell face; two
rewrite paths that reduce the same source to the same target are the
boundary of a potential 3-cell. The Squier obstruction says: if the
polygraph's `H_2` is not finitely generated, the rewriting system has
*no* finite convergent presentation, full stop. That is a theorem
about the existence of algorithms, proved by computing a homology
group — and it is invisible to a 1-D rewriter because the obstruction
lives in a dimension the 1-D rewriter cannot even represent.

The demo shows `alifib` doing precisely this kind of computation on
four tiny examples and getting the right answers, including two
different classifying-space homologies and one torsion class.

## Concurrency, in case the link to threads looks rhetorical

The connection to concurrency is not a metaphor. Higher-dimensional
automata (Pratt 1991; van Glabbeek 2006) present a concurrent system as
a cubical complex whose `n`-cells are `n`-fold simultaneous transitions;
Mazurkiewicz traces are the 1-skeleton quotiented by the independence
relation, and the higher homology records genuinely concurrent
invariants. Polygraphs generalise both — they are strictly more
expressive, since they allow direction on every cell — and the homology
of the polygraph is the homology of the concurrent-trace space. Acts I
and III compute exactly that: the concurrent-trace space of two and
three independent actions.

## Files

- `examples/ShapeOfConcurrency.ali` — the four polygraphs.
- `scripts/smoke.sh` — the narrated demo driver.
- `docs/SMOKE.md` — this document.

## Further reading

- Squier–Otto–Kobayashi, *A finiteness condition for rewriting systems*
  (TCS 131, 1994).
- Lafont–Métayer, *Polygraphic resolutions and homology of monoids*
  (JPAA 213, 2009).
- Guiraud–Malbos–Mimram, *A homotopical completion procedure with
  applications to coherence of monoids* (RTA 2013).
- Mimram, *Towards 3-dimensional rewriting theory* (LMCS 2014).
- Pratt, *Modelling concurrency with geometry* (POPL 1991).
- van Glabbeek, *On the expressiveness of higher-dimensional automata*
  (TCS 2006).
