//! String diagram data extracted from a [`Diagram`].
//!
//! A [`StrDiag`] consists of a set of vertices — *nodes* (top-dimensional cells)
//! and *wires* (codimension-1 cells) — together with three directed acyclic
//! graphs encoding layout constraints:
//!
//! - **height**: an edge means "source must be below target"
//! - **width**: an edge means "source must be left of target"
//! - **depth**: an edge means "source must be behind target" (for wire crossings)
//!
//! The height graph is bipartite (wires → nodes → wires) and directly reflects
//! the face relationships. The width and depth graphs are derived from
//! codimension-2 and codimension-3 face cascades respectively, with cycles
//! removed to ensure acyclicity.
//!
//! The construction follows Hadzihasanovic's `rewalt` library.

use super::complex::Complex;
use super::diagram::Diagram;
use super::graph::DiGraph;
use super::intset::{self, IntSet};
use super::ogposet::Sign;

/// Whether a vertex represents a node (top-dim cell) or a wire (codim-1 cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexKind {
    Node,
    Wire,
}

/// String diagram data extracted from a diagram.
///
/// Vertices are indexed with wires first (`0..num_wires`), then nodes
/// (`num_wires..num_wires + num_nodes`). All three graphs share this
/// vertex index space.
#[derive(Debug)]
pub struct StrDiag {
    /// Number of wire vertices (codim-1 cells).
    pub num_wires: usize,
    /// Number of node vertices (top-dim cells).
    pub num_nodes: usize,
    /// Resolved generator name for each vertex.
    pub labels: Vec<String>,
    /// Kind of each vertex.
    pub kinds: Vec<VertexKind>,
    /// Height constraint graph: "source is below target".
    pub(crate) height: DiGraph,
    /// Width constraint graph: "source is left of target".
    pub(crate) width: DiGraph,
    /// Depth constraint graph: "source is behind target" (wires only).
    pub(crate) depth: DiGraph,
}

impl StrDiag {
    /// Extract string diagram data from a `(diagram, complex)` pair.
    ///
    /// The diagram provides the shape (ogposet) and cell labels; the complex
    /// maps labels to generator names.
    pub fn from_diagram(diagram: &Diagram, complex: &Complex) -> Self {
        let shape = &diagram.shape;
        let dim = shape.dim.max(0) as usize;
        let sizes = shape.sizes();

        let num_nodes = sizes.get(dim).copied().unwrap_or(0);
        let num_wires = if dim >= 1 { sizes.get(dim - 1).copied().unwrap_or(0) } else { 0 };
        let total = num_wires + num_nodes;

        // ── Build vertex labels and kinds ────────────────────────────────

        let mut labels = Vec::with_capacity(total);
        let mut kinds = Vec::with_capacity(total);

        // Wires first (dim-1 cells, indices 0..num_wires).
        if dim >= 1 {
            for pos in 0..num_wires {
                labels.push(resolve_label(diagram, complex, dim - 1, pos));
                kinds.push(VertexKind::Wire);
            }
        }
        // Then nodes (dim cells, indices num_wires..total).
        for pos in 0..num_nodes {
            labels.push(resolve_label(diagram, complex, dim, pos));
            kinds.push(VertexKind::Node);
        }

        let wire_idx = |pos: usize| -> usize { pos };
        let node_idx = |pos: usize| -> usize { num_wires + pos };

        // ── Height graph ─────────────────────────────────────────────────
        // Bipartite: wire → node (input face) and node → wire (output face).

        let mut height = DiGraph::new(total);
        let mut out_1: Vec<IntSet> = vec![vec![]; num_nodes]; // output wires per node
        let mut in_1: Vec<IntSet> = vec![vec![]; num_nodes];  // input wires per node

        if dim >= 1 {
            for np in 0..num_nodes {
                in_1[np] = shape.faces_of(Sign::Input, dim, np);
                out_1[np] = shape.faces_of(Sign::Output, dim, np);
                for &wp in &in_1[np] {
                    height.add_edge(wire_idx(wp), node_idx(np));
                }
                for &wp in &out_1[np] {
                    height.add_edge(node_idx(np), wire_idx(wp));
                }
            }
        }

        // ── Width graph ──────────────────────────────────────────────────
        // Edges from codim-2 face cascades with coface exclusion filtering.

        let mut width = DiGraph::new(total);
        let mut out_2: Vec<IntSet> = vec![vec![]; total];
        let mut in_2: Vec<IntSet> = vec![vec![]; total];

        if dim >= 2 {
            // For wires: direct faces at dim-2.
            for wp in 0..num_wires {
                out_2[wire_idx(wp)] = shape.faces_of(Sign::Output, dim - 1, wp);
                in_2[wire_idx(wp)] = shape.faces_of(Sign::Input, dim - 1, wp);
            }

            // For nodes: faces of output/input wires, filtered by coface exclusion.
            for np in 0..num_nodes {
                out_2[node_idx(np)] = filtered_faces(
                    shape, dim - 1, Sign::Output, &out_1[np], Sign::Input,
                );
                in_2[node_idx(np)] = filtered_faces(
                    shape, dim - 1, Sign::Input, &in_1[np], Sign::Output,
                );
            }

            // Edge x → y if out_2[x] ∩ in_2[y] ≠ ∅.
            for x in 0..total {
                for y in 0..total {
                    if x != y && !intset::is_disjoint(&out_2[x], &in_2[y]) {
                        width.add_edge(x, y);
                    }
                }
            }
            remove_cycles(&mut width);
        }

        // ── Depth graph ──────────────────────────────────────────────────
        // Wires only, from codim-3 face cascades.

        let mut depth = DiGraph::new(total);

        if dim >= 3 {
            let out_3s: Vec<IntSet> = (0..num_wires)
                .map(|wp| {
                    filtered_faces(
                        shape, dim - 2, Sign::Output, &out_2[wire_idx(wp)], Sign::Input,
                    )
                })
                .collect();
            let in_3s: Vec<IntSet> = (0..num_wires)
                .map(|wp| {
                    filtered_faces(
                        shape, dim - 2, Sign::Input, &in_2[wire_idx(wp)], Sign::Output,
                    )
                })
                .collect();

            for x in 0..num_wires {
                for y in 0..num_wires {
                    if x != y && !intset::is_disjoint(&out_3s[x], &in_3s[y]) {
                        depth.add_edge(wire_idx(x), wire_idx(y));
                    }
                }
            }
            remove_cycles(&mut depth);
        }

        Self { num_wires, num_nodes, labels, kinds, height, width, depth }
    }

    /// Total number of vertices (wires + nodes).
    pub fn num_vertices(&self) -> usize {
        self.num_wires + self.num_nodes
    }

    /// Look up a diagram by name in a complex (try named diagrams first,
    /// then generator classifiers) and build a `StrDiag` from it.
    pub fn from_named(name: &str, complex: &Complex) -> Option<Self> {
        let diagram = complex.find_diagram(name)
            .or_else(|| complex.classifier(name))?;
        Some(Self::from_diagram(diagram, complex))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Resolve the label of cell `(dim, pos)` in the diagram to a generator name.
fn resolve_label(diagram: &Diagram, complex: &Complex, dim: usize, pos: usize) -> String {
    diagram
        .labels_at(dim)
        .and_then(|labels| labels.get(pos))
        .and_then(|tag| complex.find_generator_by_tag(tag).cloned())
        .unwrap_or_else(|| "?".to_string())
}

/// Compute filtered faces for the width/depth graph construction.
///
/// For each cell `w` in `sources` (at dimension `face_dim`), collect its faces
/// with sign `face_sign` at dimension `face_dim - 1`. Keep only those faces `z`
/// whose cofaces (with sign `exclude_sign`, at dimension `face_dim - 1`) are
/// disjoint from `sources`.
///
/// This implements the hierarchical exclusion pattern from rewalt: a lower-dimensional
/// cell is included only if it is not "already accounted for" by the higher-dimensional
/// flow.
fn filtered_faces(
    shape: &super::ogposet::Ogposet,
    face_dim: usize,
    face_sign: Sign,
    sources: &IntSet,
    exclude_sign: Sign,
) -> IntSet {
    let mut result = Vec::new();
    for &w in sources {
        for &z in &shape.faces_of(face_sign, face_dim, w) {
            if face_dim >= 1
                && intset::is_disjoint(
                    &shape.cofaces_of(exclude_sign, face_dim - 1, z),
                    sources,
                )
            {
                result.push(z);
            }
        }
    }
    result.sort_unstable();
    result.dedup();
    result
}

/// Remove all edges that participate in cycles by computing strongly connected
/// components (Tarjan's algorithm) and discarding intra-component edges.
fn remove_cycles(graph: &mut DiGraph) {
    let n = graph.node_count();
    if n == 0 {
        return;
    }

    // Tarjan's SCC algorithm.
    let mut index_counter = 0usize;
    let mut stack: Vec<usize> = Vec::new();
    let mut on_stack = vec![false; n];
    let mut indices = vec![usize::MAX; n];
    let mut lowlinks = vec![0usize; n];
    let mut component_of = vec![usize::MAX; n];
    let mut num_components = 0usize;
    let mut component_sizes = Vec::new();

    // Iterative Tarjan to avoid stack overflow on large graphs.
    // Each frame records (node, successor_index).
    for start in 0..n {
        if indices[start] != usize::MAX {
            continue;
        }
        let mut call_stack: Vec<(usize, usize)> = vec![(start, 0)];
        indices[start] = index_counter;
        lowlinks[start] = index_counter;
        index_counter += 1;
        stack.push(start);
        on_stack[start] = true;

        while let Some((v, si)) = call_stack.last_mut() {
            let v = *v;
            if *si < graph.successors[v].len() {
                let w = graph.successors[v][*si];
                *si += 1;
                if indices[w] == usize::MAX {
                    indices[w] = index_counter;
                    lowlinks[w] = index_counter;
                    index_counter += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    call_stack.push((w, 0));
                } else if on_stack[w] {
                    lowlinks[v] = lowlinks[v].min(indices[w]);
                }
            } else {
                // All successors processed.
                if lowlinks[v] == indices[v] {
                    let mut size = 0;
                    loop {
                        let w = stack.pop().unwrap();
                        on_stack[w] = false;
                        component_of[w] = num_components;
                        size += 1;
                        if w == v {
                            break;
                        }
                    }
                    component_sizes.push(size);
                    num_components += 1;
                }
                call_stack.pop();
                // Propagate lowlink to parent.
                if let Some((parent, _)) = call_stack.last() {
                    lowlinks[*parent] = lowlinks[*parent].min(lowlinks[v]);
                }
            }
        }
    }

    // Remove edges within non-trivial SCCs (size > 1).
    let non_trivial: Vec<bool> = component_sizes.iter().map(|&s| s > 1).collect();

    let mut new_succ = vec![vec![]; n];
    let mut new_pred = vec![vec![]; n];
    for u in 0..n {
        for &v in &graph.successors[u] {
            let same_component = component_of[u] == component_of[v];
            if !(same_component && non_trivial[component_of[u]]) {
                intset::insert(&mut new_succ[u], v);
                intset::insert(&mut new_pred[v], u);
            }
        }
    }
    graph.successors = new_succ;
    graph.predecessors = new_pred;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::Tag;
    use crate::core::complex::Complex;
    use crate::core::diagram::{CellData, Diagram};
    use std::sync::Arc;

    /// Build a test complex and diagrams: pt (0-cell), ob : pt -> pt (1-cell).
    fn make_ob_complex() -> (Complex, Diagram, Diagram) {
        let pt_tag = Tag::Local("pt".into());
        let ob_tag = Tag::Local("ob".into());

        let pt = Diagram::cell(pt_tag.clone(), &CellData::Zero).unwrap();
        let ob = Diagram::cell(ob_tag.clone(), &CellData::Boundary {
            boundary_in: Arc::new(pt.clone()),
            boundary_out: Arc::new(pt.clone()),
        }).unwrap();

        let mut complex = Complex::empty();
        complex.add_generator("pt".into(), pt_tag, pt.clone());
        complex.add_generator("ob".into(), ob_tag, ob.clone());

        (complex, pt, ob)
    }

    #[test]
    fn strdiag_single_arrow() {
        let (complex, _pt, ob) = make_ob_complex();
        let sd = StrDiag::from_diagram(&ob, &complex);

        // ob is a 1-cell: 1 node (ob), 2 wires (the two pt's)
        assert_eq!(sd.num_nodes, 1);
        assert_eq!(sd.num_wires, 2);
        assert_eq!(sd.kinds[0], VertexKind::Wire);
        assert_eq!(sd.kinds[1], VertexKind::Wire);
        assert_eq!(sd.kinds[2], VertexKind::Node);

        // Height: wire → node → wire
        let node = 2;
        assert!(!sd.height.predecessors[node].is_empty(), "node should have input wire");
        assert!(!sd.height.successors[node].is_empty(), "node should have output wire");
    }

    #[test]
    fn strdiag_two_arrow_paste() {
        let (complex, _pt, ob) = make_ob_complex();
        let ob_ob = Diagram::paste(0, &ob, &ob).unwrap();
        let sd = StrDiag::from_diagram(&ob_ob, &complex);

        // ob #0 ob is 1-dim: 2 nodes, 3 wires (source pt, middle pt, target pt)
        assert_eq!(sd.num_nodes, 2);
        assert_eq!(sd.num_wires, 3);

        // Each node has 1 input wire and 1 output wire
        for np in 0..2 {
            let vi = sd.num_wires + np;
            assert_eq!(sd.height.predecessors[vi].len(), 1, "node {np} should have 1 input");
            assert_eq!(sd.height.successors[vi].len(), 1, "node {np} should have 1 output");
        }

        // The middle wire connects to both nodes (output of first, input of second)
        // Find it: the wire that has both a predecessor node and a successor node
        let middle = (0..sd.num_wires).find(|&wi| {
            !sd.height.predecessors[wi].is_empty() && !sd.height.successors[wi].is_empty()
        });
        assert!(middle.is_some(), "should have a middle wire connecting two nodes");
    }
}
