---
kind: concept
status: stable
last-touched: 2026-06-01
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
graphs — by [[analysis]], in `src/analysis/strdiag.rs`.

- **`StrDiag`** holds the dualised data: `num_wires`, `num_nodes`, per-vertex
  `labels`/`tags`/`kinds`, and the three `DiGraph`s `height`, `width`, `depth`.
  Vertices are packed wires-first (`0..num_wires`) then nodes
  (`num_wires..num_wires+num_nodes`); all three graphs share this index space.
  `VertexKind::{Wire, Node}` tags each stratum.

- **`StrDiag::from_diagram_at_dim`** is the constructor and the literal encoding
  of the table above. Nodes are the `dim`-cells, wires the `(dim-1)`-cells
  (`shape.sizes()`). The **height** graph is built directly from
  `shape.faces_of(Sign::Input/Output, dim, np)` — wire $\to$ node for input
  faces, node $\to$ wire for output faces — so it is bipartite by construction
  (tests `strdiag_single_arrow`, `strdiag_two_arrow_paste` pin this: one arrow is
  1 node + 2 wires; two pasted arrows share a middle wire that is the output of
  the first node and the input of the second). The **width** graph
  ($\dim \ge 2$) draws an edge $x \to y$ whenever the codim-2 output cascade of
  $x$ meets the input cascade of $y$; **depth** ($\dim \ge 3$) is the analogous
  wires-only codim-3 relation.

- The exclusion principle is **`strdiag::filtered_faces`** *(internal)*: for each
  source cell it keeps a face only when that face's opposite-sign cofaces are
  disjoint from the source set. The cycle-removal step is
  **`strdiag::remove_cycles`** *(internal)*, an iterative Tarjan SCC pass that
  discards every edge inside a non-trivial strongly-connected component — the
  "chosen acyclic resolution" the math demands.

- `from_diagram` defaults `dim` to the diagram's `top_dim()`; `from_named` looks
  a diagram up in a [[core-complex]] via `find_diagram` then `classifier`.

### Interactive side channels

The dual is surfaced to the web UI and MCP front-ends as JSON. The serialiser is
`interactive::protocol::strdiag_to_json`: it emits `num_wires`, `num_nodes`, a
`vertices` array (`index`, `kind` `"wire"|"node"`, `label`, `tag`), and the three
graphs each as `{ "edges": [[u,v], …] }` (`protocol::strdiag_to_json::edges_json`
*(internal)* flattens the `DiGraph` adjacency).

- **`WebRepl::get_strdiag(type_name, item_name, boundary_dim, boundary_sign)`**
  (`src/interactive/web.rs`) resolves a named diagram/classifier inside a type
  and returns its dual; the optional `(boundary_dim, boundary_sign)` first
  extracts a $\partial^\pm_k$ boundary (`"input"` $\mapsto$ `Sign::Input`) so a
  caller can view a boundary one dimension down.
- Session views `get_session_strdiag` / `get_target_strdiag` and rewrite previews
  go through `protocol::strdiag_json_from_diagram` and
  `protocol::step_output_strdiag_json`; the latter takes the
  `Sign::Output`-boundary of a rewrite step, which is the geometric "after"
  picture. `build_strdiag_response` and `build_map_image_strdiag` are the
  store-level entry points behind the `/api/get_strdiag` route and the MCP
  `get_strdiag` tool.

## Related

- [[diagram]], [[molecule]] — the primal objects this dualises.
- [[oriented-graded-poset]], [[boundary]] — the face relation $\partial^\pm_k$
  the three constraint graphs are read from.
- [[flow-graph]] — the height order is the diagram's directed flow.
- [[analysis]] — the implementing module.
