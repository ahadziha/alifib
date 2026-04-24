# How `alifib homology` works

`alifib homology T` prints the cellular homology of the polygraph that
presents type `T`. This note explains, in one sitting, what is being
computed and how the numbers come out.

## The one-sentence version

A polygraph is a CW complex. Its cellular chain complex has one free
Z-generator per cell in each dimension, the differentials count faces
with sign, and the homology — reduced to canonical form over the
integers by Smith Normal Form — is what `alifib homology` prints.

## The chain complex

For a polygraph with generator set `G_n` at dimension `n`, put

```
C_n  =  Z^{|G_n|}       (free abelian group on the n-cells)
```

and define the boundary `d_n : C_n → C_{n-1}` on each generator
`g ∈ G_n` by

```
d_n(g)  =  (sum of output faces)  −  (sum of input faces)
```

counted with multiplicity: if the same `(n−1)`-cell appears three times
in the target of `g` it contributes `+3`, and so on.

alifib reads those faces directly off each generator's classifier
(`labels_at(d − 1)` together with `faces_of(Output/Input, d, 0)`) — see
`compute_homology` in [`src/core/homology.rs`](../src/core/homology.rs).

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

## Worked example I — `Pair` (Act I of the demo)

```
Pair <<= {
    pt,
    a, b : pt → pt,
    comm : a b → b a
}
```

- `C_0 = Z⟨pt⟩`, `C_1 = Z⟨a, b⟩`, `C_2 = Z⟨comm⟩`.
- `d_1(a) = pt − pt = 0`, `d_1(b) = 0`. So `d_1 = 0`.
- `d_2(comm) = (b + a) − (a + b) = 0`. So `d_2 = 0`.

Both differentials vanish, so every chain group is its own homology:
`H_0 = Z`, `H_1 = Z²`, `H_2 = Z`. That is the integer homology of the
2-torus.

## Worked example II — `Torsion` (Act IV of the demo)

```
Torsion <<= {
    pt,
    a : pt → pt,
    double : a a → a a a a
}
```

- `C_1 = Z⟨a⟩`, `C_2 = Z⟨double⟩`.
- `d_2(double) = 4·a − 2·a = 2·a`. The matrix of `d_2` is the 1×1
  block `[2]`, already in SNF.
- `ker(d_2) = 0`, so `H_2 = 0`.
- `im(d_2) = 2Z ⊂ Z = ker(d_1)`, so `H_1 = Z / 2Z = Z/2`.

The single invariant `s_1 = 2 > 1` shows up as torsion in `H_1`. No
amount of rank-only reasoning (working over Q) would have detected it.

## The Euler characteristic

`alifib homology` also reports

```
χ  =  Σ (−1)^n · |G_n|.
```

By the rank part of the calculation above, this also equals
`Σ (−1)^n · rank(H_n)` — the torsion contributes nothing to `χ`. The
Euler characteristic is cheap to eyeball and is a fast sanity check
for missing top-dimensional cells: if a presentation "looks like" it
should give the `k`-torus but `χ ≠ 0` for even `k` (or ≠ 0, full stop,
for any torus `T^k`), something is missing upstairs. That is the
signal Act II of the demo picks up on.

## References

- Hatcher, *Algebraic Topology*, §2.2 — the textbook account of
  cellular homology.
- Dummit & Foote, *Abstract Algebra*, §12.1 — Smith Normal Form over
  a PID.
- Lafont & Métayer, *Polygraphic resolutions and homology of monoids*
  (JPAA 213, 2009) — the direct ancestor of what alifib computes,
  specialised to monoids and their coherences.
- Implementation: [`src/core/homology.rs`](../src/core/homology.rs).
