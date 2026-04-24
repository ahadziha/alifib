# The Shape of Concurrency

An `alifib` demo that computes cellular homology of four polygraphic
rewriting systems and checks that each one matches the integer homology
of a known topological space.

```
bash scripts/demo.sh
```

## Setup in one paragraph

A polygraph (Burroni 1993) is a presentation of a higher category by
generators in each dimension: 0-cells (objects), 1-cells (morphisms),
2-cells (rewrite rules), 3-cells (rewrites between rewrites), and so on.
Its underlying combinatorial skeleton is a **regular directed complex**
in Hadzihasanovic's sense: each cell has a source and a target that
together tile its boundary sphere. Forget the source/target distinction
and you recover an ordinary CW complex, one `n`-cell per generator in
dimension `n`. `alifib homology` computes the integer cellular homology
of that CW complex — via the chain complex of free abelian groups on
the generators, reduced by Smith Normal Form. None of this is
accessible to a rewriter that only sees 1-D terms and reduction
sequences.

## Examples

The demo file is [`examples/ShapeOfConcurrency.ali`](../examples/ShapeOfConcurrency.ali).
Each example declares a small polygraph and `alifib` computes its homology.

### I. Pair — two concurrent threads

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

### II. Triple — three threads, pairwise commuting

```
Triple <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab, comm_ac, comm_bc   (three commutation 2-cells)
}
```

The 3-torus would give `H_0=Z, H_1=Z³, H_2=Z³, H_3=Z, χ=0`.
What `alifib` reports: `H_0=Z, H_1=Z³, H_2=Z³, χ=1`. No `H_3`, and the
Euler characteristic is 1, not 0. The presentation is complete at
dimension 2 — every critical pair of 1-cells is joined — but there is
a 3-dimensional gap in the homology.

### III. TripleCoh — adding the Zamolodchikov tetrahedron

```
TripleCoh <<= {
    pt,
    a, b, c : pt -> pt,
    comm_ab, comm_ac, comm_bc,
    zamo : (comm_ab c)(b comm_ac)(comm_bc a)
         -> (a comm_bc)(comm_ac b)(c comm_ab)
}
```

There are two compositions of three 2-cells taking `a b c` to `c b a`.
`zamo` is a 3-cell that identifies them. Adding it gives

```
H_0 = Z, H_1 = Z³, H_2 = Z³, H_3 = Z, χ = 0
```

i.e. `H_*(T³)`. The coherence cell is required: any coherent extension
must identify those two pastings, and `zamo` is the extension witness.
This is the concrete content of Squier's homological finiteness theorem
(Squier–Otto–Kobayashi 1994; Lafont–Métayer 2009;
Guiraud–Malbos–Mimram 2013).

### IV. Torsion — Smith Normal Form

```
Torsion <<= {
    pt,
    a : pt -> pt,
    double : a a -> a a a a
}
```

The 2-cell sends the abelianised generator `a` to `4a − 2a = 2a`. The
cokernel is `Z/2`. `alifib` reports `H_0=Z, H_1=Z/2, H_2=0`,
The Smith Normal Form computation detects torsion, not just free ranks.

## Comparison with 1-D term rewriting

1-D rewriters (Maude, ELAN, CafeOBJ, Stratego) operate on rewriting
sequences. They do not represent the underlying rewriting complex as a
space and cannot compute its homology.

The polygraphic viewpoint attaches geometry to rewriting. The
underlying regular directed complex forgets to a CW complex in which
two rewrites sharing a generator are 2-cells sharing a 1-cell face, and
two rewrite paths reducing the same source to the same target bound a
potential 3-cell. The Squier obstruction says: if the polygraph's
`H_2` is not finitely generated, the rewriting system has no finite
convergent presentation. That conclusion is reached by computing a
homology group — it is not accessible to a 1-D engine because the
obstruction lives in a dimension that engine cannot represent.

## Concurrency

Higher-dimensional
automata (Pratt 1991; van Glabbeek 2006) present a concurrent system as
a cubical complex whose `n`-cells are `n`-fold simultaneous transitions;
Mazurkiewicz traces are the 1-skeleton quotiented by the independence
relation, and the higher homology records genuinely concurrent
invariants. Polygraphs generalise both — they are strictly more
expressive, since they allow direction on every cell — and the cellular
homology computed from the underlying directed complex is the homology
of the concurrent-trace space. Acts I and III compute exactly that: the
concurrent-trace space of two and three independent actions.

## Files

- `examples/ShapeOfConcurrency.ali` — the four polygraphs.
- `scripts/demo.sh` — the demo script.
- `docs/SMOKE.md` — this document.

## Further reading

- Squier–Otto–Kobayashi, *A finiteness condition for rewriting systems*
  (TCS 131, 1994).
- Lafont–Métayer, *Polygraphic resolutions and homology of monoids*
  (JPAA 213, 2009).
- Guiraud–Malbos–Mimram, *A homotopical completion procedure with
  applications to coherence of monoids* (RTA 2013).
- Mimram, *Towards 3-dimensional rewriting theory* (LMCS 2014).
- Hadzihasanovic, *Combinatorics of higher-categorical diagrams* —
  regular directed complexes and their geometric realisation as
  regular CW complexes (the variant alifib implements).
- Pratt, *Modelling concurrency with geometry* (POPL 1991).
- van Glabbeek, *On the expressiveness of higher-dimensional automata*
  (TCS 2006).
