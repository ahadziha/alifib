---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Homology

Strip the labels off a [[diagram]] and what remains is an
[[oriented-graded-poset|oriented graded poset]]: a finite set of cells, graded
by dimension, with each cell's faces split by orientation into input ($-$)
and output ($+$). That orientation is exactly the datum a **chain complex**
needs. A **type** is a [[directed-complex]] — a cell complex that need *not* be
regular, since labels may identify distinct boundary cells (only the individual
[[atom|atoms]]/[[molecule|molecules]] it is built from are
[[regular-directed-complex|regular]]). alifib reads the signed face structure of
that complex as integer differentials and computes the **integer cellular
homology** $H_n(G; \mathbb{Z})$ — free ranks (the
[Betti numbers](https://en.wikipedia.org/wiki/Betti_number)), torsion
invariants, and the Euler characteristic — by **Smith Normal Form**.

Homology is a coarse but honest invariant: it forgets all directedness and
keeps only the additive boundary algebra, so two shapes with the same homology
may rewrite very differently. Its value here is diagnostic — torsion at $H_1$,
for instance, flags a generator that can only be cancelled an integer multiple
of times, which in a concurrency reading is a contention bug (see the
witness tests under *Implementation*).

## Definition

### The chain complex

Let $G$ be a [[directed-complex|directed complex]] (a type) with cell sets $G_n$
(the $n$-dimensional [[atom|generators]]). The **$n$-chains** form the free
abelian group on $G_n$:

$$ C_n \;=\; \mathbb{Z}\langle G_n \rangle, \qquad C_n \cong \mathbb{Z}^{|G_n|}. $$

The **differential** $\partial_n : C_n \to C_{n-1}$ is the *signed sum of
codimension-one faces*, with output faces counted $+1$ and input faces $-1$,
each with multiplicity:

$$ \partial_n(g) \;=\; \sum_{f \in \partial^+_{n-1} g} f \;-\; \sum_{f \in \partial^-_{n-1} g} f. $$

This is the orientation of the [[boundary|boundary]] $\partial^\pm_{n-1}$ read
off as a coefficient: $+1$ for each occurrence of $f$ in the output boundary,
$-1$ for each occurrence in the input boundary. A face appearing twice in the
same boundary contributes $\pm 2$; a face appearing once in each cancels to $0$.
The orientation conventions are those of Hadzihasanovic, *Combinatorics of
higher-categorical diagrams* (2024) (`docs/papers/`).

The defining law $\partial_{n-1} \circ \partial_n = 0$ ($d^2 = 0$) holds because
each $(n{-}2)$-face of $g$ is reached through both an input and an output
$(n{-}1)$-face, so its net coefficient cancels. This is the **acyclicity** of
the [[directed-complex]] structure made arithmetic; alifib re-checks it as a
runtime assertion (see *Implementation*).

### Homology groups

With $Z_n = \ker \partial_n$ the **cycles** and $B_n = \operatorname{im}
\partial_{n+1}$ the **boundaries**, $d^2 = 0$ gives $B_n \subseteq Z_n$, and the
**$n$-th homology group** is the quotient

$$ H_n(G; \mathbb{Z}) \;=\; Z_n / B_n. $$

As a finitely generated abelian group it decomposes (by the structure theorem)
into a free part and torsion:

$$ H_n \;\cong\; \mathbb{Z}^{\,\beta_n} \;\oplus\; \mathbb{Z}/d_1 \oplus \cdots \oplus \mathbb{Z}/d_k, \qquad 1 < d_1 \mid d_2 \mid \cdots \mid d_k. $$

The free rank $\beta_n$ is the **$n$-th Betti number**; the $d_i$ are the
**torsion invariants** satisfying the divisibility chain $d_1 \mid \cdots \mid
d_k$. The **Euler characteristic** is computable two ways — from chain ranks or
from Betti numbers — and they must agree:

$$ \chi \;=\; \sum_n (-1)^n |G_n| \;=\; \sum_n (-1)^n \beta_n. $$

### Computing via Smith Normal Form

Pick bases for $C_n$ and $C_{n+1}$ (the generators, in a fixed order) and write
$\partial_{n+1}$ as an integer matrix $M$. Its **Smith Normal Form** is a
factorisation $U M V = \operatorname{diag}(d_1, \dots, d_r, 0, \dots, 0)$ with
$U, V \in \mathrm{GL}(\mathbb{Z})$ and $d_1 \mid \cdots \mid d_r$. From the
diagonal:

- $\operatorname{rank} \partial_{n+1} = r$ (number of nonzero $d_i$);
- the $d_i > 1$ are exactly the torsion invariants of $H_n$;
- $\beta_n = |G_n| - \operatorname{rank}\partial_n - \operatorname{rank}\partial_{n+1}$
  (rank-nullity: $\dim Z_n = |G_n| - \operatorname{rank}\partial_n$, then quotient
  by $\operatorname{im}\partial_{n+1}$).

A $d_i > 1$ means a cycle $z \in C_n$ that is *not* a boundary, yet $d_i \cdot z
= \partial_{n+1}(p)$ *is* — a class of order $d_i$. Tracking the change-of-basis
matrices $U^{-1}$ and $V$ recovers an explicit such $z$ (column of $U^{-1}$) and
its preimage $p$ (column of $V$): a **torsion witness** with the invariant
$d_i \cdot z = \partial_{n+1}(p)$.

#### Worked invariants

The gallery tests (`assert_homology` runs `homology_s1` through `homology_rp3`)
pin the arithmetic against known spaces, each presented as a one-line type with
a single top cell folding a word of 1-cells:

| Type | presentation (top 2-cell) | $H_1$ | reading |
|---|---|---|---|
| `S1` | $a : \mathrm{pt}\to\mathrm{pt}$ (no 2-cell) | $\mathbb{Z}$ | circle |
| `Klein` | $f : a \to abb$ | $\mathbb{Z} \oplus \mathbb{Z}/2$ | Klein bottle |
| `RP2` | $\mathrm{face} : \mathrm{id} \to aa$ | $\mathbb{Z}/2$ | $\mathbb{RP}^2$ |
| `Lens5` | $\mathrm{face} : \mathrm{id} \to a^5$ | $\mathbb{Z}/5$ | lens space $L(5,1)$ |
| `Klein4` | two relations $a^2, b^2$ | $(\mathbb{Z}/2)^2$ | invariant factors $2 \mid 2$ |

The `Lens5` case (SNF of $\partial_2$ is $\operatorname{diag}(1, 5)$, so $H_1 =
\mathbb{Z}/5$) and `Klein4` (invariant factors $2 \mid 2$ — two $\mathbb{Z}/2$
summands, not a $\mathbb{Z}/4$) are the cleanest demonstrations that the SNF,
not a $\mathbb{Z}/2$ shortcut, is doing the work.

## Implementation

Realised by [[analysis]] in `src/analysis/homology.rs`; that page documents the
linear algebra in full. The bridge:

- **Entry point.** `homology::compute_homology(&Complex) -> Homology`:
  per-dimension `groups` (each an `AbelianGroup` — `free_rank` +
  divisibility-ordered `torsion`), `euler_characteristic`, and
  `torsion_witnesses`.
- **The differential, literally.** Per generator, the signed codim-1 faces are
  read off the [[oriented-graded-poset]] of its classifier via
  `Ogposet::faces_of` *(internal)* — `Sign::Output` $\mapsto +1$, `Sign::Input`
  $\mapsto -1$ — each face tag resolved by `Complex::find_generator_by_tag`.
  Generators are name-sorted per dimension; that sort *is* the chosen basis.
- **Assertions.** $d^2 = 0$ is a `debug_assert!` (`mat_mul` of adjacent
  differentials is zero); $\chi_{\text{chain}} = \chi_{\text{homology}}$ a
  `debug_assert_eq!` via `Homology::euler_from_homology`.
- **One SNF driver.** `homology::snf_reduce<T: Tracker>` *(internal)* runs the
  pivot loop once for both callers: zero-cost `NoTrack` drives
  `smith_normal_form` → `matrix_rank`; `FullTrack` (row ops mirrored *inverted*
  onto $U^{-1}$, column ops directly onto $V$) drives
  `smith_normal_form_with_basis` for witnesses. The tracked normalisations
  `enforce_divisibility_tracked` / `sort_diag_tracked` keep witness columns
  paired with the canonical invariants even when divisibility mixes pairs (raw
  $[3,2] \to$ canonical $[1,6]$) — mechanics on [[analysis]]. Pinned by
  `smith_with_torsion` (`[[2,4],[0,6]] → [2,6]`), `smith_unit_d3`,
  `smith_zero_matrix`, `tracked_enforce_divisibility_crt`,
  `tracked_sort_diag_permutes_witnesses`.
- **Witnesses.** Each $d_i > 1$ yields a `TorsionWitness` $\{order, cycle,
  preimage\}$ with $d_i \cdot z = \partial_{n+1}(p)$. `all_witnesses_are_cycles`
  (via the helper `verify_witnesses_valid`) re-applies the real differential to
  every reported witness; the concurrency-flavoured `witness_torsion_example`,
  `witness_races_k_contention`, and `witness_aba_bug` read torsion as
  contention, and `witness_none_when_torsion_free` pins the silent case.
- **Surfaced to users.** The `homology <Type>` command is served *straight from
  the loaded store* — no rewrite engine:
  `interactive::protocol::build_homology_data` flattens each group into a
  `HomologyGroupInfo` carrying its `Vec<TorsionWitnessInfo>` (order + formatted
  cycle/preimage via `TorsionWitness::cycle_str` / `preimage_str`), and the
  shared `richtext::homology` renderer prints each `H_d` line with an indented
  `Z/d cycle: … (preimage: …)` sub-line per witness. The REPL, web, and MCP
  front-ends all answer it; the headless daemon alone *refuses* it
  (`"homology not supported in daemon mode"`). See [[interactive-repl]] and
  [[interactive-daemon-web]]; `docs/HOMOLOGY.md` walks the `homology RP2`
  output.

## Related

[[oriented-graded-poset]] · [[boundary]] · [[directed-complex]] ·
[[regular-directed-complex]] · [[diagram]] · [[atom]] · [[analysis]]
