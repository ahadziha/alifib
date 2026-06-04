# How `alifib homology` works

`alifib homology T` prints the integer cellular homology of the
directed complex that presents type `T`. This note explains, in one
sitting, what is being computed and how the numbers come out.

## The one-sentence version

alifib builds, for each type, a **finite directed complex** (in
Hadzihasanovic's sense) — a finite presheaf on the category of atoms
and embeddings of atoms. Every cell carries a designated source and a
designated target that together tile its boundary. Forgetting the
source/target distinction — just remembering that the two halves glue
to an `(n−1)`-sphere — gives an ordinary CW complex with one `n`-cell
per generator in dimension `n`. Its cellular chain complex, with signs
recovered from the directed structure, is what `alifib homology`
reduces to Smith Normal Form and prints.

## The chain complex

Write `G_n` for the set of generators (= cells) at dimension `n` and put

```
C_n  =  Z^{|G_n|}       (free abelian group on the n-cells)
```

with boundary `d_n : C_n → C_{n-1}` defined on each generator
`g ∈ G_n` by

```
d_n(g)  =  (sum of output faces)  −  (sum of input faces)
```

counted with multiplicity: if the same `(n−1)`-cell appears three times
in the target of `g` it contributes `+3`, and so on.

alifib reads those faces directly off each generator's classifier
(`labels_at(d − 1)` together with `faces_of(Output/Input, d, 0)`) — see
`compute_homology` in [`src/analysis/homology.rs`](../src/analysis/homology.rs).

A short calculation (or Hatcher §2.2) shows `d_{n-1} ∘ d_n = 0`:
every `(n−2)`-face of an `n`-cell appears twice in the double boundary
with opposite signs. `compute_homology` verifies this as a debug
check.

## Homology

```
H_n  =  ker(d_n) / im(d_{n+1}).
```

To compute the group, put the integer matrix of each `d_n` into
**Smith Normal Form**. SNF is the integer analogue of Gaussian
elimination: there are invertible integer matrices `U, V` with

```
U · d_n · V  =  diag(s_1, s_2, …, s_k, 0, 0, …),     1 ≤ s_1 | s_2 | … | s_k.
```

The `s_i` are intrinsic — they do not depend on the choice of `U, V`.
Reading off the SNF gives:

- the rank of `ker(d_n)` (number of zero columns in the diagonal form);
- the rank of `im(d_n)` (number of nonzero `s_i`);
- the torsion summands `Z/s_i` for the `s_i > 1` — these end up as
  torsion in `H_{n-1}`, since `im(d_n)` sits inside `ker(d_{n-1})` with
  those cyclic quotients.

Putting it together: the free rank of `H_n` is
`rank(ker d_n) − rank(im d_{n+1})`, and the torsion of `H_n` is the
list of `s_i > 1` from the SNF of `d_{n+1}`.

## Worked example I — the torus `T`

[`examples/Delta_complexes.ali`](../examples/Delta_complexes.ali) presents a
triangulated torus as a Δ-complex with one vertex, three edges, and two
triangles:

```
T <<= {
    attach v :: Delta.0Simplex,
    attach a :: Delta.1Simplex along [ d0 => v, d1 => v ],
    attach b :: Delta.1Simplex along [ d0 => v, d1 => v ],
    attach c :: Delta.1Simplex along [ d0 => v, d1 => v ],
    attach U :: Delta.2Simplex along [ d0 => b, d1 => c, d2 => a ],
    attach L :: Delta.2Simplex along [ d0 => a, d1 => c, d2 => b ]
}
```

- `C_0 = Z⟨v⟩`, `C_1 = Z⟨a, b, c⟩`, `C_2 = Z⟨U, L⟩`.
- A 1-simplex runs `d1 → d0`, so `d_1(e) = d0 − d1`. Every edge here is a loop
  `v → v`, hence `d_1(a) = d_1(b) = d_1(c) = v − v = 0`: `d_1 = 0`.
- A 2-simplex has input face `d1` and output faces `d0, d2` (its boundary reads
  `d1 → d2 d0`), so `d_2(σ) = d0 + d2 − d1`. With the attachments above,
  `d_2(U) = b + a − c` and `d_2(L) = a + b − c` — the same element `a + b − c`.

So `im(d_2) = Z⟨a + b − c⟩` (rank 1) and `ker(d_2) = Z⟨U − L⟩` (rank 1). Hence

- `H_2 = ker(d_2) = Z` (nothing above bounds it);
- `H_1 = ker(d_1) / im(d_2) = Z³ / Z⟨a + b − c⟩ = Z²` (`a + b − c` is primitive);
- `H_0 = Z`.

That is the integer homology of the 2-torus — `homology T` prints
`H_0 = Z`, `H_1 = Z²`, `H_2 = Z`, `χ = 0`.

## Worked example II — the projective plane `RP2`

The same file presents `RP²` with two vertices, three edges, and two triangles:

```
RP2 <<= {
    attach v :: Delta.0Simplex,
    attach w :: Delta.0Simplex,
    attach a :: Delta.1Simplex along [ d0 => w, d1 => v ],
    attach b :: Delta.1Simplex along [ d0 => w, d1 => v ],
    attach c :: Delta.1Simplex along [ d0 => v, d1 => v ],
    attach U :: Delta.2Simplex along [ d0 => b, d1 => a, d2 => c ],
    attach L :: Delta.2Simplex along [ d0 => a, d1 => b, d2 => c ]
}
```

- `C_0 = Z⟨v, w⟩`, `C_1 = Z⟨a, b, c⟩`, `C_2 = Z⟨U, L⟩`.
- `a` and `b` run `v → w` while `c` is a loop, so `d_1(a) = d_1(b) = w − v` and
  `d_1(c) = 0`. Thus `im(d_1) = Z⟨w − v⟩` and `H_0 = Z² / Z⟨w − v⟩ = Z`.
- With `d_2(σ) = d0 + d2 − d1` as before, `d_2(U) = b + c − a` and
  `d_2(L) = a + c − b`. The matrix of `d_2` on rows `(a, b, c)` is

```
       U    L
  a [ −1    1 ]
  b [  1   −1 ]
  c [  1    1 ]
```

Its Smith Normal Form is `diag(1, 2)`: the entries have gcd 1 (so `s_1 = 1`),
while the 2×2 minors have gcd 2 (so `s_1 · s_2 = 2`, giving `s_2 = 2`).

- `ker(d_2) = 0`, so `H_2 = 0`.
- The invariant factor `s_2 = 2 > 1` becomes torsion in `H_1`: `H_1 = Z/2`. No
  amount of rank-only reasoning (working over Q) would have detected it.

`homology RP2` prints `H_0 = Z`, `H_1 = Z/2`, `H_2 = 0`, `χ = 1`, and — because
the tracked SNF also recovers change-of-basis data — a *witness* for the torsion
class: the generating 1-cycle `c`, paired with the 2-chain `U + L` whose boundary
`∂(U + L) = 2c` certifies that `c` has order 2 in `H_1`.

## The Euler characteristic

`alifib homology` also reports

```
χ  =  Σ (−1)^n · |G_n|.
```

By the rank part of the calculation above, this also equals
`Σ (−1)^n · rank(H_n)` — torsion contributes nothing to `χ`. The
Euler characteristic is cheap to eyeball and is a fast sanity check
for missing top-dimensional cells: every `k`-torus has `χ = 0` (as `T`
above does), so a presentation that "looks like" a torus but reports
`χ ≠ 0` is missing cells upstairs.

## References

- Hatcher, *Algebraic Topology*, §2.2 — the textbook account of
  cellular homology.
- Dummit & Foote, *Abstract Algebra*, §12.1 — Smith Normal Form over
  a PID.
- Lafont & Métayer, *Polygraphic resolutions and homology of monoids*
  (JPAA 213, 2009) — the direct ancestor of what alifib computes,
  specialised to monoids and their coherences.
- Hadzihasanovic, *Combinatorics of higher-categorical diagrams* —
  regular directed complexes and their geometric realisation as
  regular CW complexes.
- Worked examples: [`examples/Delta_complexes.ali`](../examples/Delta_complexes.ali)
  — runnable Δ-complexes (circle, torus, `RP²`, Klein bottle, spheres, lens
  spaces); try `homology <type>` in the repl.
- Implementation: [`src/analysis/homology.rs`](../src/analysis/homology.rs).
