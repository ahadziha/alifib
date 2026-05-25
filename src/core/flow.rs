//! Flow graph construction from ogposets.
//!
//! - [`flow_graph`] — constructs the k-flow graph **F**_k(U)
//! - [`maximal_flow_graph`] — constructs the maximal k-flow graph **M**_k(U)

use std::sync::Arc;
use crate::aux::intset::{self, IntSet};
use crate::aux::graph::DiGraph;
use super::ogposet::{self, Ogposet, Sign};

/// Constructs the k-flow graph **F**_k(U) (Definition 61 of Hadzihasanovic–Kessler).
///
/// **Nodes** — all cells of `g` at dimensions strictly greater than `k`.  The returned
/// `node_map[i] = (dim, pos)` gives the `(dimension, position)` of node `i` in `g`.
///
/// **Edges** — there is a directed edge `x → y` iff `Δ⁺_k(x) ∩ Δ⁻_k(y) ≠ ∅`,
/// i.e. the output k-boundary of `x` and the input k-boundary of `y` share a k-cell.
///
/// Returns `(graph, node_map)`.  When `k >= g.dim` or `g` is empty, returns an empty graph.
pub(super) fn flow_graph(g: &Arc<Ogposet>, k: usize) -> (DiGraph, Vec<(usize, usize)>) {
    if g.dim < 0 { return (DiGraph::new(0), vec![]); }
    let gd = g.dim as usize;
    if k >= gd { return (DiGraph::new(0), vec![]); }

    let mut node_map: Vec<(usize, usize)> = Vec::new();
    for dim in (k + 1)..=gd {
        let n_cells = g.faces_in[dim].len();
        for pos in 0..n_cells {
            node_map.push((dim, pos));
        }
    }
    let n = node_map.len();
    let mut graph = DiGraph::new(n);

    let out_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Output, k, dim, pos)
    }).collect();
    let in_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Input, k, dim, pos)
    }).collect();

    for (xi, out) in out_k.iter().enumerate() {
        for (yi, incoming) in in_k.iter().enumerate() {
            if xi == yi { continue; }
            if !intset::is_disjoint(out, incoming) {
                graph.add_edge(xi, yi);
            }
        }
    }

    (graph, node_map)
}

/// Constructs the maximal k-flow graph **M**_k(U).
///
/// This is the induced subgraph of **F**_k(U) restricted to cells that are *maximal* —
/// i.e. cells that have no cofaces in either direction within `g`.  For a pure regular
/// molecule, every top-dimensional cell is maximal, so **M**_{n-1}(U) and **F**_{n-1}(U)
/// coincide.  For lower k values, intermediate-dimensional cells without cofaces are
/// also included.
///
/// Returns `(graph, node_map)` where `node_map[i] = (dim, pos)`.
pub(crate) fn maximal_flow_graph(g: &Arc<Ogposet>, k: usize) -> (DiGraph, Vec<(usize, usize)>) {
    if g.dim < 0 { return (DiGraph::new(0), vec![]); }
    let gd = g.dim as usize;
    if k >= gd { return (DiGraph::new(0), vec![]); }

    let mut node_map: Vec<(usize, usize)> = Vec::new();
    for dim in (k + 1)..=gd {
        for pos in g.maximal(dim) {
            node_map.push((dim, pos));
        }
    }
    let n = node_map.len();
    let mut graph = DiGraph::new(n);

    let out_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Output, k, dim, pos)
    }).collect();
    let in_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Input, k, dim, pos)
    }).collect();

    for (xi, out) in out_k.iter().enumerate() {
        for (yi, incoming) in in_k.iter().enumerate() {
            if xi == yi { continue; }
            if !intset::is_disjoint(out, incoming) {
                graph.add_edge(xi, yi);
            }
        }
    }

    (graph, node_map)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::flow_graph;
    use crate::core::ogposet::Ogposet;

    #[test]
    fn flow_graph_two_arrow_paste() {
        let u = Arc::new(Ogposet::make(
            1,
            vec![
                vec![vec![], vec![], vec![]],
                vec![vec![0], vec![1]],
            ],
            vec![
                vec![vec![], vec![], vec![]],
                vec![vec![1], vec![2]],
            ],
            vec![
                vec![vec![0], vec![1], vec![]],
                vec![vec![], vec![]],
            ],
            vec![
                vec![vec![], vec![0], vec![1]],
                vec![vec![], vec![]],
            ],
        ));
        let (fg, node_map) = flow_graph(&u, 0);
        assert_eq!(fg.node_count(), 2, "one node per 1-cell");
        assert_eq!(node_map.len(), 2);
        assert!(fg.successors[0].contains(&1), "f → g edge must exist (shared midpoint m)");
        assert!(!fg.successors[1].contains(&0), "no g → f edge (endpoints a and b are disjoint)");
    }
}
