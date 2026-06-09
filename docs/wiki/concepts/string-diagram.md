---
kind: concept
status: stable
last-touched: 2026-06-09
---

# String diagram

A **string diagram** is the *Poincaré dual* presentation of a [[diagram]]
(equivalently a [[molecule]] or, structurally, an [[oriented-graded-poset]]).
Where a pasting diagram draws a $k$-cell as a $k$-dimensional region, its string
dual reads dimensions backwards: a top cell becomes a point, a codimension-1 cell
becomes a line, and the ambient space becomes the page. The two carry the same
combinatorial content — they are the same poset of cells — but the dual layout is
the one a human can *see*, because composition becomes adjacency in the plane.

## Definition

Fix a diagram $U$ of dimension $n$ (an oriented graded poset together with a
labelling of its cells by generators). Its cells stratify by dimension $0 \le k
\le n$. The **Poincaré dual** of $U$ assigns to a $k$-cell a stratum of
*codimension* $k$ in an $n$-dimensional drawing space:

| cell dimension | pasting picture | string-dual stratum |
| --- | --- | --- |
| $n$ (top) | a top region | a **node** (point) |
| $n-1$ | a bounding face | a **wire** (line) |
| $n-2$ | a sub-face | a **region** / wire-crossing datum |
| $\dim 0$ | a corner | the background |

The duality is *orientation-aware*: alifib lives in directed higher categories,
so the input/output split of each boundary survives the dualisation. The
face data of a node — its input boundary $\partial^-_{n-1}$ and output boundary
$\partial^+_{n-1}$ — become the wires *entering from below* and *leaving above*.
This is exactly the geometric reading of a string diagram: a node consumes its
input wires and emits its output wires, and pasting $U \#_k V$ glues the output
wires of $U$ to the input wires of $V$ at codimension $k$.

### What a layout must preserve

A concrete drawing is a choice of coordinates for every wire and node. It is
*faithful* iff it respects three independent partial orders read off the face
relation, one per spatial axis:

- **Height** ($\partial_{n-1}$, the face relation between top cells and their
  codim-1 faces). An input face of a node sits *below* it; an output face sits
  *above*. This order is **bipartite**, alternating wire $\to$ node $\to$ wire,
  and is forced exactly — no freedom. It is the directed flow of the diagram and
  coincides with the [[flow-graph]] reading one dimension down.

- **Width** (codim-2 cascade). Two strata that share a codim-2 face must be
  ordered left-to-right. This comes from the $\partial_{n-2}$ data, refined by a
  *coface-exclusion* filter (below), and unlike height it can be cyclic in
  principle; a faithful layout requires a chosen acyclic resolution.

- **Depth** (codim-3 cascade, wires only). When wires cross, which passes behind
  which is governed by $\partial_{n-3}$ data. Below dimension 3 there is nothing
  to order and the relation is empty.

The non-trivial content is the **exclusion principle** borrowed from
Hadzihasanovic's *rewalt*: a lower-dimensional face contributes a width/depth
constraint only if it is *not already accounted for* by a higher-dimensional
flow — formally, only if its cofaces of the opposite sign are disjoint from the
cells generating the cascade. Without this filter the dual graph fills with
spurious constraints and no acyclic layout exists. The width and depth orders
are therefore *derived*, not primitive: build the raw constraint graph from
$\partial$-cascades, then quotient out cycles.

## Implementation

The dual is computed structurally — no geometry, only the three constraint
graphs — by [[analysis]] in `src/analysis/strdiag.rs`; that page has the full
data flow. The bridge:

- **`strdiag::StrDiag`** holds the dualised data: `num_wires`, `num_nodes`,
  per-vertex `labels`/`tags`/`kinds` (`VertexKind::{Wire, Node}`), and the three
  `DiGraph`s `height`, `width`, `depth` over one index space, wires first
  (`0..num_wires`), then nodes.
- **`StrDiag::from_diagram_at_dim`** is the table above made literal: nodes are
  the `dim`-cells, wires the `(dim-1)`-cells. The **height** graph transcribes
  `Ogposet::faces_of` — wire $\to$ node per input face, node $\to$ wire per
  output face — so it is bipartite by construction (tests
  `strdiag_single_arrow`, `strdiag_two_arrow_paste`: one arrow is 1 node +
  2 wires; two pasted arrows share a middle wire). **Width** ($\dim \ge 2$)
  draws $x \to y$ whenever the codim-2 output cascade of $x$ meets the input
  cascade of $y$; **depth** ($\dim \ge 3$) is the analogous wires-only codim-3
  relation.
- The exclusion principle is **`strdiag::filtered_faces`** *(internal)*: keep a
  face only when its opposite-sign cofaces are disjoint from the source set.
  The acyclic resolution is **`strdiag::remove_cycles`** *(internal)*: an
  iterative Tarjan SCC pass discarding every edge inside a non-trivial
  component.
- `from_diagram` defaults `dim` to the diagram's `top_dim()`; `from_named`
  resolves a name in a [[core-complex]] (`find_diagram`, then `classifier`).
  The proof view is the one explicit-dim caller: `WebRepl::get_proof_strdiag`
  views the proof diagram at `top_dim() + 1`, where at step 0 the node set is
  empty.

### Surfacing

`interactive::protocol::strdiag_to_json` serialises a `StrDiag` for the canvas
renderer: `num_wires`, `num_nodes`, a `vertices` array (`index`, `kind`
`"wire"|"node"`, `label`, `tag`), and the three graphs each as
`{ "edges": [[u,v], …] }`. `WebRepl`'s `get_*_strdiag` family wraps the
protocol builders:

- `get_strdiag` → `protocol::build_strdiag_response` — a named
  diagram/classifier inside a type, with optional $\partial^\pm_k$
  pre-extraction (`(boundary_dim, boundary_sign)`, `"input"` $\mapsto$
  `Sign::Input`) to view a boundary one dimension down;
- `get_map_image_strdiag` → `protocol::build_map_image_strdiag` — the image of
  a domain generator under a [[partial-map|map]];
- `get_session_strdiag` / `get_target_strdiag` →
  `protocol::strdiag_json_from_diagram`;
- `get_rewrite_preview_strdiag` → `protocol::step_output_strdiag_json` — the
  `Sign::Output`-boundary of a rewrite step, the geometric "after" picture.

The server's `/api/get_*_strdiag` routes and the MCP tools (`get_strdiag`,
`get_session_strdiag`, `get_rewrite_preview_strdiag`) map 1:1 onto these
methods — see [[web-backends]] for the transports and [[web-frontend]] for the
renderer that positions and paints the graphs.

## Related

- [[diagram]], [[molecule]] — the primal objects this dualises.
- [[oriented-graded-poset]], [[boundary]] — the face relation $\partial^\pm_k$
  the three constraint graphs are read from.
- [[flow-graph]] — the height order is the diagram's directed flow.
- [[analysis]] — the implementing module.
