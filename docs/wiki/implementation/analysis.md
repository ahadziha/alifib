---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/analysis/homology.rs, src/analysis/strdiag.rs]
---

# analysis — homology & string-diagram layout

Two read-only observers of the [[oriented-graded-poset|ogposet]] underlying a
[[core-complex|`Complex`]]/[[diagram|`Diagram`]]. Neither mutates anything: they
*derive* an invariant from the face/coface incidence already computed by
`Ogposet`. `homology.rs` builds the integer chain complex and computes its
homology by Smith Normal Form; `strdiag.rs` extracts the node/wire layout DAGs
of a string diagram.

## What it owns

- **`homology::compute_homology`** — the cellular homology $H_n$ of a complex:
  free rank, torsion invariants $1 < d_1 \mid d_2 \mid \dots$, Euler
  characteristic, and a *torsion witness* (a concrete cycle + bounding chain)
  for every torsion class.
- **`strdiag::StrDiag`** — a string-diagram's vertices (nodes = top cells,
  wires = codim-1 cells) plus three layout DAGs (height/width/depth).

Both are pure functions of the ogposet incidence; they own no state.

## Key public types

| Type / fn | Role |
| --- | --- |
| `homology::AbelianGroup` | f.g. abelian group: `free_rank: usize` + `torsion: Vec<i64>` (divisibility-ordered). |
| `homology::TorsionWitness` | `order`, `cycle` (in $C_n$), `preimage` (in $C_{n+1}$) — all in the *original* generator bases. |
| `homology::Homology` | per-dimension `groups`, `euler_characteristic`, `torsion_witnesses` map. |
| `homology::compute_homology(&Complex)` | the entry point. |
| `strdiag::VertexKind` | `Node` (top-dim cell) or `Wire` (codim-1 cell). |
| `strdiag::StrDiag` | `num_wires`, `num_nodes`, `labels`, `tags`, `kinds`, and three `DiGraph`s. |
| `StrDiag::from_diagram` / `from_diagram_at_dim` / `from_named` | builders. |

## Data flow — homology

1. **Generators by dimension.** `compute_homology` walks
   `Complex::generators_iter` into `gens_by_dim: HashMap<usize, Vec<String>>`,
   sorting each bucket by name so the chosen basis of $C_n$ is deterministic.
2. **Differentials.** For each $n$, the matrix of
   $d_n\colon C_n \to C_{n-1}$ has rows = $(n{-}1)$-generators, columns =
   $n$-generators. Column $g$ is read straight off `g`'s classifier
   (`Complex::classifier`): its single top cell at position $0$ contributes
   $+1$ for each `Sign::Output` face and $-1$ for each `Sign::Input` face
   (`Ogposet::faces_of`), each face's tag resolved back to a generator via
   `Complex::find_generator_by_tag`. So
   $$d_n(g) = \sum_{f \in \partial^+_{n-1} g} f \;-\; \sum_{f \in \partial^-_{n-1} g} f.$$
3. **$d^2 = 0$ check.** A `debug_assert!` multiplies $d_{n-1}\cdot d_n$
   (`homology::mat_mul`) and requires zero — the algebraic shadow of the
   ogposet's globularity.
4. **Homology per dimension.** $\operatorname{rank} d_n$ comes from
   `homology::matrix_rank` (= count of nonzero SNF diagonal entries, via
   `homology::smith_normal_form`). $\ker d_n / \operatorname{im} d_{n+1}$ is
   read off the SNF of $d_{n+1}$: free rank $= |C_n| - \operatorname{rank} d_n -
   \operatorname{rank} d_{n+1}$, torsion $=$ the SNF diagonal entries $> 1$.
5. **Euler characteristic.** Computed two ways —
   $\sum_n (-1)^n |C_n|$ directly, and $\sum_n (-1)^n \operatorname{rank} H_n$
   via `Homology::euler_from_homology` — and a `debug_assert_eq!` pins them
   equal.

## Data flow — strdiag

`StrDiag::from_diagram_at_dim` fixes a dimension `dim`: **nodes** are the
`dim`-cells, **wires** the `(dim-1)`-cells. Vertices are indexed wires-first
(`0..num_wires`) then nodes (`num_wires..total`), a single index space shared by
all three graphs. Labels/tags resolve through `strdiag::resolve_label` /
`resolve_tag` against the diagram and `Complex::find_generator_by_tag`.

- **height** (`DiGraph`, bipartite): wire→node for each input face, node→wire
  for each output face — a direct transcription of `Ogposet::faces_of` at `dim`.
- **width** (built when `dim ≥ 2`): edge $x \to y$ when the codim-2 *output*
  cascade of $x$ meets the *input* cascade of $y$ (`intset::is_disjoint`
  negated). Node cascades are `strdiag::filtered_faces` of their height
  neighbours; raw wires use their own faces. Cycles stripped by
  `strdiag::remove_cycles`.
- **depth** (built when `dim ≥ 3`, wires only): the same construction one
  codimension deeper, again cycle-stripped.

`strdiag::filtered_faces` is rewalt's hierarchical exclusion: a lower face is
kept only if its cofaces (under `Ogposet::cofaces_of`) are disjoint from the
source set, i.e. it is not already accounted for by the higher-dimensional flow.

## Non-obvious invariants & gotchas

- **Torsion-witness change-of-basis flows in lockstep with the diagonal.**
  This is the subtle part. `homology::smith_normal_form_with_basis` returns
  `(diag, u_inv, v)` in *raw* (pre-normalisation) order, where after row/col
  reduction $U\,M\,V = \operatorname{diag}(\dots)$: column $i$ of `u_inv` is the
  $i$-th new basis vector of $C_n$ in the original basis, column $i$ of `v` the
  same for $C_{n+1}$. The reported invariants, however, come from the
  *canonical* diagonal produced by `homology::enforce_divisibility_tracked`
  (collapse adjacent $(a,b)$ with $a \nmid b$ to $(\gcd, \operatorname{lcm})$)
  then `homology::sort_diag_tracked` (zeros last, increasing). Both rewrites
  apply their column operations to `u_inv` **and** `v` at the same moment they
  rewrite `diag`, so column $i$ of each matrix always pairs with `diag[i]`. If
  the diagonal were normalised alone, the raw $3$-$2$ block $[3,2]$ collapsing
  to $[1,6]$ would silently desync the witnesses from the reported $\mathbb{Z}/6$.
  Evidence: `tracked_enforce_divisibility_crt` checks the CRT $[3,2]\to[1,6]$
  case keeps $d_{n+1}(v_i) = d_i\,u\_{inv,i}$; `tracked_sort_diag_permutes_witnesses`
  checks the sort permutes both matrices' columns identically.
- **Witnesses are genuine cycles with a genuine bound.** For a torsion entry
  $d_i = d > 1$: `cycle` $= $ column $i$ of `u_inv` (a nonzero class of order
  $d$ in $H_n$), `preimage` $= $ column $i$ of `v`, satisfying
  $d_{n+1}(\text{preimage}) = d \cdot \text{cycle}$. Sign is canonicalised so
  the cycle's leading coefficient is positive (negating both chains together).
  `all_witnesses_are_cycles` re-derives $d_n(\text{cycle})$ and
  $d_{n+1}(\text{preimage})$ from the ogposet for eleven types and checks both
  identities; `witness_torsion_example`, `witness_races_k_contention`,
  `witness_aba_bug` pin specific cycles.
- **Two parallel SNF families — untracked and tracked.** They are *separate*
  code, not one parameterised driver. The **untracked** family —
  `smith_normal_form` (diagonal only) with helpers `find_and_move_pivot`,
  `eliminate_column`, `eliminate_row`, `enforce_divisibility` — has the single
  caller `matrix_rank`, used everywhere a rank is wanted. The **tracked** family
  — `smith_normal_form_with_basis` (returns `u_inv`/`v`) with the mirrored
  helpers `find_and_move_pivot_tracked`, `eliminate_column_tracked`,
  `eliminate_row_tracked`, `enforce_divisibility_tracked`, `sort_diag_tracked`,
  and the elementary-op carriers `row_swap_tracked` / `row_add_tracked` /
  `row_negate_tracked` / `row_gcd_tracked` — is called only from
  `compute_homology` (for witnesses). Both share the same pivot strategy:
  `find_and_move_pivot{,_tracked}` picks the *smallest* nonzero $|m_{rc}|$ to
  bound the GCD reductions, guaranteeing termination. The tracked variant
  additionally mirrors every row op as the inverse column op on `u_inv` and
  every column op onto `v` — that inverse-op bookkeeping is the whole point
  (see the doc comments on those `fn`s, all *(internal)*). The driver loops and
  the integer 2×2 row arithmetic are near line-for-line duplicated across the
  two families (~150 LOC of overlap), so the two copies of the subtle SNF logic
  must be kept in lockstep — a known correctness risk, **not yet DRY** (tracked
  in `source-drift.md`: parameterise over a `Tracker` trait so one generic
  driver serves both).
- **Generator ordering is the chosen basis.** Sorting `gens_by_dim` by name is
  load-bearing: it fixes which generator is row/column $i$, hence what the
  witness coefficients *mean*. Change the sort and the witness coordinates
  change with it.
- **Empty/edge cases.** No generators → empty `Homology`. A missing $d_n$
  (no $(n{-}1)$-cells) contributes rank $0$; a degenerate ($d < 1$) classifier
  column is skipped. SNF on a zero or empty matrix returns the obvious thing
  (`smith_zero_matrix`, `smith_identity`).
- **strdiag DAGs are forced acyclic by construction.** width/depth edges are
  produced by an all-pairs intersection test that can manufacture cycles;
  `remove_cycles` runs iterative Tarjan SCC and discards every intra-SCC edge
  (only size-1 components survive intact). The height graph is bipartite and
  never needs this.
- **`from_diagram_at_dim` may view a diagram one dimension up.** When `dim`
  exceeds the diagram's own top dimension the node set is empty — used by the
  proof view at step 0 (see the module doc). `from_named` tries named diagrams
  first, then falls back to a generator's classifier.
- **Stale path in legacy docs.** `docs/HOMOLOGY.md` cites `src/core/homology.rs`;
  the code now lives at `src/analysis/homology.rs`. Treat that path as rotted.

## Mathematics

These modules are real *realisations* of derived structure over the ogposet,
not mere support code:

- `homology.rs` realises the **integer cellular chain complex** $(C_\bullet,
  \partial)$ of a [[regular-directed-complex]] and its **[[homology]]** — free
  rank, torsion, Euler characteristic, with torsion classes made concrete as
  cycles and bounding chains. The differential is exactly the signed
  input/output face boundary $\partial^-, \partial^+$ of the
  [[oriented-graded-poset]].
- `strdiag.rs` realises the **[[string-diagram]]** layout of a [[diagram]]:
  the height/width/depth partial orders that place nodes and wires in the
  plane, derived from the codimension-$1$/$2$/$3$ face cascades of the
  [[oriented-graded-poset]].
