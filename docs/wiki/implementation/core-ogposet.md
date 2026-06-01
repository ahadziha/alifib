---
kind: impl
status: stable
last-touched: 2026-06-01
code: [src/core/ogposet.rs]
---

# core-ogposet — the signed face substrate

> Beneath every [[diagram]] is a bare shape: cells stacked by dimension, each
> face relation tagged input or output. `Ogposet` is that shape and nothing
> more — no labels, no paste history. It owns the four signed adjacency tables
> and the traversals that read [[boundary|boundaries]], canonicalise cells, and
> decide shape isomorphism.

## What it owns

`Ogposet` is the combinatorial backbone of an [[oriented-graded-poset]]: a graded
set of cells `0..=dim` together with, for each cell, its signed faces (one
dimension down) and signed cofaces (one dimension up). The module owns the data
structure, the local geometric predicates (`extremal`, `maximal`, `is_pure`,
`is_round`, `layering_dimension`), and the four core *operations* that build a
new shape from an old one: `boundary`, `boundary_traverse`, `normalisation`, and
`find_isomorphism` — all driven by a single `traverse`. A `Diagram`
(`src/core/diagram.rs`) is exactly an `Arc<Ogposet>` shape plus a label per cell;
this module supplies the shape.

## Key public types

| Type / symbol | For |
|---|---|
| `Ogposet` | the shape: `dim: isize` plus four `Vec<Vec<IntSet>>` adjacency tables and a `normal` flag |
| `Sign` *(internal)* | `Input` ($\partial^-$), `Output` ($\partial^+$), `Both` — which side of a face relation |
| `Ogposet::{empty, point}` | the two atomic shapes: `dim = -1` (no cells) and `dim = 0` (one point) |
| `Ogposet::{equal, sizes, is_round}` (pub) | structural equality, per-dimension cell counts, roundness predicate |
| `boundary` *(pub(super))* | the sign-side $k$-boundary sub-ogposet + its `Embedding` into `g` |
| `boundary_traverse` *(pub(super))* | the *normalised* sign-side $k$-boundary; also the `Both` shared boundary of an $n$-cell |
| `normalisation` *(pub(super))* | canonical cell reordering + embedding back to the original |
| `find_isomorphism` *(pub(super))* | decide shape isomorphism via canonical forms |
| `traverse` *(pub(super))* | the engine under all three above: closure of a seed stack in canonical input-first order |
| `closure`, `signed_k_boundary_of_cell` *(pub(super))* | membership-only downward closure; $\Delta^\pm_k(x)$ of one cell |

The four tables, indexed `[dim][cell]` into an `IntSet` (sorted `Vec<usize>`):
`faces_in` / `faces_out` (neighbours at $d-1$) and `cofaces_in` / `cofaces_out`
(neighbours at $d+1$). `faces_of(sign, d, p)` and `cofaces_of(sign, d, p)` read
them uniformly, with `Sign::Both` taking the union. The whole struct is small,
`Clone`, and shared by `Arc` so embeddings can point back at it cheaply.

## Data flow

Three layered operations all funnel through one traversal:

```
build_stack_extremal ─┐
build_stack_paste    ─┼─▶ traverse(g, seed_stack, mark_normal)
build_stack_cell_n   ─┘        │  closure of seeds, canonical order
                               │  + remap_adjacency ×4
                               ▼
                       (sub: Arc<Ogposet>, emb: Embedding)
        ┌──────────────┬───────────────┴──────────────┐
   normalisation   boundary_traverse          (matching/reconstruct)
        │                │
        │           Input|Output ▶ build_stack_paste
        │           Both         ▶ build_stack_cell_n
        ▼
   find_isomorphism: normalise u, v ▶ Ogposet::equal ▶ compose embeddings
```

- `boundary(sign, k, g)` — the *order-preserving* extraction. It seeds with the
  sign-`extremal` cells at level $k$, walks down adding every face of an already-
  kept parent (plus any `maximal` cell at each level so nothing is dropped), then
  `remap_adjacency` rebuilds the four tables on the chosen subset. Cell order is
  whatever the walk produced — **not** normalised. `k >= g.dim` short-circuits to
  `g` with an identity `Embedding`; `g.dim < 0` to the empty shape.
- `traverse(g, stack, mark_normal)` — first computes the downward closure of the
  seeds as `BitSet`s, then runs a stack machine that emits cells in *canonical
  input-first order*: a cell is marked only once all its input faces are marked,
  candidates are chosen by lowest already-assigned input-face index (ties by old
  index). `remap_adjacency` then rebuilds the tables. `mark_normal` stamps the
  result's `normal` flag so downstream code can skip re-normalising.
- `normalisation` seeds `traverse` with `build_stack_extremal(Input, …)` and
  marks the result normal; `boundary_traverse` seeds with `build_stack_paste`
  (sided) or `build_stack_cell_n` (the `Both` shared boundary of an $n$-cell).
- `find_isomorphism(u, v)` normalises both and compares canonical forms with
  `Ogposet::equal`; on a match it composes the two normalisation embeddings (and
  their inverses) into the iso, with cheap pre-checks (`dim`, `sizes`, then a raw
  `equal`) that avoid normalising when the shapes are already identical.

## Non-obvious invariants and gotchas

- **`dim = -1` is the empty shape, not a bug.** `dim: isize` exists precisely so
  the empty ogposet is representable. Every operation guards `g.dim < 0` first.
- **`boundary` order ≠ `boundary_traverse` order.** `boundary` preserves a
  walk order; `boundary_traverse` returns the *normalised* boundary. Both are
  needed: the former for a faithful sub-shape, the latter when shapes must be
  compared by canonical form. Conflating them silently breaks isomorphism checks.
- **`extremal` is defined by *missing cofaces*, not by faces.** An `Input`-
  extremal $k$-cell is one with no *output* coface (nothing has consumed it as a
  target), and dually for `Output`. This is the input/output-boundary frontier,
  and `Sign::Both` is the union — the cells on either boundary.
- **`is_round` only inspects `is_pure` shapes** and reads layers via
  `build_layer`, checking input/output interiors are disjoint at every level. A
  single top input face (`faces_in[n].len() == 1`) is round by fiat.
- **`layering_dimension` counts maximal cells across *all* intermediate
  dimensions**, not just the top — non-pure shapes can have maximal cells below
  the top dimension, and they are counted. For a pure molecule of dimension $n$
  with $m$ top-cells it is $n-2$ when $m \le 1$, else $n-1$.
- **`remap_adjacency`'s `shift` is asymmetric.** `shift = -1` (faces) assumes
  every neighbour survives into the subset and `set_map`s them; `shift = +1`
  (cofaces) `set_filter_map`s, silently dropping neighbours outside the subset.
  Using the wrong shift corrupts the rebuilt tables.
- **`NO_PREIMAGE` (`= usize::MAX`, from `embeddings`) is the in-band "not in
  image" sentinel** throughout `inv_dom` / `inv` tables here; never index with it.
- **`Sign` is `pub(crate)`.** It threads through `diagram`, `matching`, `flow`,
  and `reconstruct` as the orientation marker; it is not part of the public API.
- **`closure` and `signed_k_boundary_of_cell` are the membership-only paths.**
  When code wants "is this cell in the downward closure?" or "what is
  $\Delta^\pm_k$ of one cell?" without building a sub-ogposet, these avoid the
  `traverse` + `remap_adjacency` cost: `closure` returns a `Vec<BitSet>` of the
  down-closure (called by `matching::check_match_isomorphism` and
  `reconstruct`), and `signed_k_boundary_of_cell` returns the $k$-cell indices of
  one cell's sign-side boundary (called by `flow`). `signed_k_boundary_of_cell`
  fast-paths `dim == k+1` to a direct face-table read. (`closure` carries a stale
  `#[allow(dead_code)]` despite these live callers — see [[source-drift]].)
- **`restrict` lives next door, not here.** The task framing pairs "closure /
  restrict"; `closure` is in this module, but `restrict_ogposet` — restrict an
  `Ogposet` to a per-dimension kept-cell mask (`&[BitSet]`) — is `reconstruct::restrict_ogposet`
  (`src/core/reconstruct.rs`), used by `matching` after a flow candidate. This
  module has no `#[test]` block of its own; its behaviour is pinned by the
  `reconstruct_*` tests in `src/core/reconstruct.rs` (e.g.
  `reconstruct_generator_with_composite_boundary`, which drives `boundary` and
  `restrict_ogposet`) and exercised throughout `matching`.

## Mathematics

`Ogposet` is the direct realisation of an [[oriented-graded-poset]]: the four
signed tables *are* the input/output face order, and `Sign` *is* the $\pm$
orientation. The roundness and purity predicates (`is_round`, `is_pure`) and the
canonical `normalisation` are the conditions and constructions that cut the
[[oriented-graded-poset|ogposets]] down to a [[regular-directed-complex]], the
substrate in which [[molecule|molecules]] live (Hadzihasanovic). The two
boundary operations realise the input/output [[boundary|boundaries]]
$\partial^-_k$ / $\partial^+_k$ at the shape level — `boundary` for a faithful
sub-shape, `boundary_traverse` for the normalised one — which `Diagram` lifts to
labelled boundaries (see [[core-diagram]]). The `Embedding` returned alongside
every operation is a total/injective [[partial-map|map of ogposets]]; shape
isomorphism (`find_isomorphism`) and the `Both` shared boundary feed the
[[rewriting]] engine — see [[core-matching]] for how matches and pushouts are
built on top of these primitives.
