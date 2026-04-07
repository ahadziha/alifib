//! Lightweight directed graphs for flow graphs, contractions, and topological sort enumeration.
//!
//! This module provides:
//! - [`DiGraph`] — a directed graph on nodes 0..n
//! - [`flow_graph`] — constructs F_k(U) (Definition 61 of Hadzihasanovic–Kessler 2304.09216)
//! - [`maximal_flow_graph`] — constructs M_k(U) (Definition 63)
//! - [`contract`] — graph contraction G/(G|_W) (Definition 88)
//! - [`all_topological_sorts`] — enumerate all topological orderings of a DAG

use std::sync::Arc;
use super::intset::{self, IntSet};
use super::ogposet::{self, Ogposet, Sign};

// ---- DiGraph ----

/// A directed graph on nodes `0..n`, represented as sorted adjacency lists.
#[derive(Debug, Clone)]
pub(super) struct DiGraph {
    n: usize,
    /// `successors[i]` — sorted list of nodes that i points to.
    pub(super) successors: Vec<IntSet>,
    /// `predecessors[i]` — sorted list of nodes pointing to i.
    pub(super) predecessors: Vec<IntSet>,
}

impl DiGraph {
    /// Create an empty graph with `n` nodes and no edges.
    pub(super) fn new(n: usize) -> Self {
        Self {
            n,
            successors: vec![vec![]; n],
            predecessors: vec![vec![]; n],
        }
    }

    /// Number of nodes.
    pub(super) fn node_count(&self) -> usize { self.n }

    /// Add a directed edge from `u` to `v`.  Idempotent.
    pub(super) fn add_edge(&mut self, u: usize, v: usize) {
        intset::insert(&mut self.successors[u], v);
        intset::insert(&mut self.predecessors[v], u);
    }

    /// True if there is a directed edge from `u` to `v`.
    #[allow(dead_code)]
    pub(super) fn has_edge(&self, u: usize, v: usize) -> bool {
        self.successors[u].binary_search(&v).is_ok()
    }

    /// Indegree of node `v`.
    pub(super) fn indegree(&self, v: usize) -> usize {
        self.predecessors[v].len()
    }
}

// ---- Flow graph construction ----

/// Constructs the k-flow graph F_k(U) of an ogposet (Definition 61).
///
/// Nodes are all cells at dimensions > k, labelled by `(dim, pos)`.
/// There is a directed edge x → y iff Δ⁺_k(x) ∩ Δ⁻_k(y) ≠ ∅.
///
/// Returns `(graph, node_map)` where `node_map[i] = (dim, pos)`.
pub(super) fn flow_graph(g: &Arc<Ogposet>, k: usize) -> (DiGraph, Vec<(usize, usize)>) {
    if g.dim < 0 { return (DiGraph::new(0), vec![]); }
    let gd = g.dim as usize;
    if k >= gd { return (DiGraph::new(0), vec![]); }

    // Enumerate nodes: all cells at dimensions k+1 ..= gd.
    let mut node_map: Vec<(usize, usize)> = Vec::new();
    for dim in (k + 1)..=gd {
        let n_cells = g.faces_in[dim].len();
        for pos in 0..n_cells {
            node_map.push((dim, pos));
        }
    }
    let n = node_map.len();
    let mut graph = DiGraph::new(n);

    // Build a lookup: (dim, pos) -> node index.
    // Use a flat offset: node index = sum_{d=k+1}^{dim-1} |cells at d| + pos.
    let sizes = g.sizes();
    let mut dim_offset = vec![0usize; gd + 2];
    let mut acc = 0usize;
    for dim in 0..=gd {
        dim_offset[dim] = acc;
        acc += sizes.get(dim).copied().unwrap_or(0);
    }
    let node_idx = |(dim, pos): (usize, usize)| -> usize {
        dim_offset[dim] - dim_offset[k + 1] + pos
    };

    // Precompute Δ⁺_k and Δ⁻_k for every node.
    // For dim = k+1: Δ⁺_k(x) = faces_out[k+1][pos], Δ⁻_k(x) = faces_in[k+1][pos].
    // For dim > k+1: use signed_k_boundary_of_cell.
    let out_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Output, k, dim, pos)
    }).collect();
    let in_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Input, k, dim, pos)
    }).collect();

    // Add edges: x -> y iff out_k[x] ∩ in_k[y] ≠ ∅.
    for xi in 0..n {
        for yi in 0..n {
            if xi == yi { continue; }
            if !intset::is_disjoint(&out_k[xi], &in_k[yi]) {
                graph.add_edge(xi, yi);
            }
        }
    }

    // Suppress unused warning for node_idx in cases where g.dim == k+1
    let _ = node_idx;

    (graph, node_map)
}

/// Constructs the maximal k-flow graph M_k(U) of an ogposet (Definition 63).
///
/// This is the induced subgraph of F_k(U) restricted to cells that are maximal
/// at their dimension (i.e. have no cofaces in any direction).
///
/// Returns `(graph, node_map)` where `node_map[i] = (dim, pos)`.
#[allow(dead_code)]
pub(super) fn maximal_flow_graph(g: &Arc<Ogposet>, k: usize) -> (DiGraph, Vec<(usize, usize)>) {
    if g.dim < 0 { return (DiGraph::new(0), vec![]); }
    let gd = g.dim as usize;
    if k >= gd { return (DiGraph::new(0), vec![]); }

    // Nodes: maximal cells at dimensions k+1 ..= gd.
    let mut node_map: Vec<(usize, usize)> = Vec::new();
    for dim in (k + 1)..=gd {
        for pos in g.maximal(dim) {
            node_map.push((dim, pos));
        }
    }
    let n = node_map.len();
    let mut graph = DiGraph::new(n);

    // Precompute boundaries.
    let out_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Output, k, dim, pos)
    }).collect();
    let in_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Input, k, dim, pos)
    }).collect();

    for xi in 0..n {
        for yi in 0..n {
            if xi == yi { continue; }
            if !intset::is_disjoint(&out_k[xi], &in_k[yi]) {
                graph.add_edge(xi, yi);
            }
        }
    }

    (graph, node_map)
}

// ---- Graph contraction ----

/// Contract the graph `graph` by the subgraph induced on `subset` (Definition 88).
///
/// Computes connected components of the undirected version of the induced
/// subgraph on `subset`, collapses each component to a single node, and builds
/// the quotient graph.  Nodes outside `subset` retain individual identities.
///
/// Returns `(quotient_graph, mapping)` where `mapping[old_node] = new_node_index`.
pub(super) fn contract(graph: &DiGraph, subset: &[usize]) -> (DiGraph, Vec<usize>) {
    let n = graph.node_count();
    let mut component = vec![usize::MAX; n];

    // Union-Find on subset nodes to find connected components
    // (treating edges as undirected).
    let mut parent: Vec<usize> = (0..n).collect();

    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        if parent[x] != x {
            parent[x] = find(parent, parent[x]);
        }
        parent[x]
    }

    fn union(parent: &mut Vec<usize>, x: usize, y: usize) {
        let rx = find(parent, x);
        let ry = find(parent, y);
        if rx != ry { parent[rx] = ry; }
    }

    // Union connected nodes within the subset.
    let subset_set: Vec<bool> = {
        let mut s = vec![false; n];
        for &v in subset { s[v] = true; }
        s
    };

    for &u in subset {
        for &v in &graph.successors[u] {
            if subset_set[v] {
                union(&mut parent, u, v);
            }
        }
        for &v in &graph.predecessors[u] {
            if subset_set[v] {
                union(&mut parent, u, v);
            }
        }
    }

    // Assign new node indices.
    // Each connected component of the subset gets one node.
    // Nodes outside the subset get individual nodes.
    let mut new_idx: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut mapping = vec![0usize; n];
    let mut next_id = 0usize;

    // First pass: nodes in subset, grouped by root.
    for &v in subset {
        let root = find(&mut parent, v);
        let id = *new_idx.entry(root).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        component[v] = id;
        mapping[v] = id;
    }

    // Second pass: nodes outside subset get fresh ids.
    for v in 0..n {
        if !subset_set[v] {
            mapping[v] = next_id;
            next_id += 1;
        }
    }

    let new_n = next_id;
    let mut quotient = DiGraph::new(new_n);

    for u in 0..n {
        for &v in &graph.successors[u] {
            let nu = mapping[u];
            let nv = mapping[v];
            if nu != nv {
                quotient.add_edge(nu, nv);
            }
        }
    }

    (quotient, mapping)
}

// ---- Topological sort enumeration ----

/// Outcome of enumerating topological orderings.
pub(super) enum TopoSortResult {
    /// All orderings collected (up to the optional limit).
    Sorts(Vec<Vec<usize>>),
    /// The graph contains a cycle; no ordering exists.
    HasCycle,
    /// The number of orderings exceeded the requested limit.
    LimitExceeded,
}

/// Enumerate all topological orderings of a DAG using Kahn's algorithm with backtracking.
///
/// Returns `TopoSortResult::Sorts` (up to `limit` orderings, or all if `None`),
/// `TopoSortResult::HasCycle` if the graph is not a DAG, or
/// `TopoSortResult::LimitExceeded` if the number of orderings exceeds the limit.
pub(super) fn all_topological_sorts(graph: &DiGraph, limit: Option<usize>) -> TopoSortResult {
    let n = graph.node_count();
    let lim = limit.unwrap_or(usize::MAX);

    let mut indegrees: Vec<usize> = (0..n).map(|v| graph.indegree(v)).collect();
    let mut result: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::with_capacity(n);

    match topo_backtrack(graph, &mut indegrees, &mut current, n, &mut result, lim) {
        BacktrackOutcome::Done => TopoSortResult::Sorts(result),
        BacktrackOutcome::Cycle => TopoSortResult::HasCycle,
        BacktrackOutcome::LimitExceeded => TopoSortResult::LimitExceeded,
    }
}

enum BacktrackOutcome { Done, Cycle, LimitExceeded }

fn topo_backtrack(
    graph: &DiGraph,
    indegrees: &mut Vec<usize>,
    current: &mut Vec<usize>,
    total: usize,
    result: &mut Vec<Vec<usize>>,
    limit: usize,
) -> BacktrackOutcome {
    if current.len() == total {
        result.push(current.clone());
        return BacktrackOutcome::Done;
    }

    // usize::MAX is the sentinel for "already scheduled".
    let ready: Vec<usize> = (0..total).filter(|&v| indegrees[v] == 0).collect();

    if ready.is_empty() {
        // Nodes remain but none have in-degree 0: the graph has a cycle.
        return BacktrackOutcome::Cycle;
    }

    for v in ready {
        current.push(v);
        indegrees[v] = usize::MAX;

        let succs: Vec<usize> = graph.successors[v].clone();
        for &s in &succs {
            if indegrees[s] != usize::MAX { indegrees[s] -= 1; }
        }

        match topo_backtrack(graph, indegrees, current, total, result, limit) {
            BacktrackOutcome::Cycle => {
                // Restore before propagating.
                for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
                indegrees[v] = 0;
                current.pop();
                // A cycle is a global property; propagate immediately.
                return BacktrackOutcome::Cycle;
            }
            BacktrackOutcome::LimitExceeded => {
                for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
                indegrees[v] = 0;
                current.pop();
                return BacktrackOutcome::LimitExceeded;
            }
            BacktrackOutcome::Done => {}
        }

        for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
        indegrees[v] = 0;
        current.pop();

        if result.len() >= limit {
            return BacktrackOutcome::LimitExceeded;
        }
    }

    BacktrackOutcome::Done
}
