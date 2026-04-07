//! Lightweight directed graphs for flow graphs, contractions, and topological sort enumeration.
//!
//! This module provides the combinatorial graph machinery used by the subdiagram matching
//! algorithms of Hadzihasanovic–Kessler (2304.09216):
//!
//! - [`DiGraph`] — a directed graph on nodes `0..n`, with sorted adjacency lists
//! - [`flow_graph`] — constructs the k-flow graph **F**_k(U) (Definition 61)
//! - [`maximal_flow_graph`] — constructs the maximal k-flow graph **M**_k(U) (Definition 63)
//! - [`contract`] — computes the quotient **G**/**G**|_W (Definition 88)
//! - [`all_topological_sorts`] — enumerates all topological orderings of a DAG
//!
//! All graph nodes are identified by small non-negative integers.  Adjacency lists are
//! [`IntSet`](super::intset::IntSet) values — sorted, deduplicated `Vec<usize>` — which
//! support O(n+m) set operations and binary-search membership queries.

use std::sync::Arc;
use super::intset::{self, IntSet};
use super::ogposet::{self, Ogposet, Sign};

// ---- DiGraph ----

/// A directed graph on nodes `0..n`, represented as sorted adjacency lists.
///
/// Both the successor list (`u → v`) and the predecessor list (`v → u`) are
/// stored explicitly so that callers can traverse edges in either direction
/// in O(degree) time.
#[derive(Debug, Clone)]
pub(super) struct DiGraph {
    n: usize,
    /// `successors[u]` — sorted list of nodes that `u` points to.
    pub(super) successors: Vec<IntSet>,
    /// `predecessors[v]` — sorted list of nodes that point to `v`.
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

    /// Add a directed edge from `u` to `v`.  Idempotent (adding a duplicate edge is a no-op).
    pub(super) fn add_edge(&mut self, u: usize, v: usize) {
        intset::insert(&mut self.successors[u], v);
        intset::insert(&mut self.predecessors[v], u);
    }

    /// True if there is a directed edge from `u` to `v`.
    #[allow(dead_code)]
    pub(super) fn has_edge(&self, u: usize, v: usize) -> bool {
        self.successors[u].binary_search(&v).is_ok()
    }

    /// In-degree of node `v`: the number of nodes with an edge to `v`.
    pub(super) fn indegree(&self, v: usize) -> usize {
        self.predecessors[v].len()
    }
}

// ---- Flow graph construction ----

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

    // Precompute Δ⁺_k and Δ⁻_k for every node.
    let out_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Output, k, dim, pos)
    }).collect();
    let in_k: Vec<IntSet> = node_map.iter().map(|&(dim, pos)| {
        ogposet::signed_k_boundary_of_cell(g, Sign::Input, k, dim, pos)
    }).collect();

    // Add edges: x → y iff Δ⁺_k(x) ∩ Δ⁻_k(y) ≠ ∅.
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

/// Constructs the maximal k-flow graph **M**_k(U) (Definition 63 of Hadzihasanovic–Kessler).
///
/// This is the induced subgraph of **F**_k(U) restricted to cells that are *maximal* —
/// i.e. cells that have no cofaces in either direction within `g`.  For a pure regular
/// molecule, every top-dimensional cell is maximal, so **M**_{n-1}(U) and **F**_{n-1}(U)
/// coincide.  For lower k values, intermediate-dimensional cells without cofaces are
/// also included.
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

    // Precompute boundaries and add edges as in flow_graph.
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

/// Contract the induced subgraph on `subset` to a single node (Definition 88).
///
/// Computes the connected components of the *undirected* version of the induced subgraph
/// `graph|_subset`, collapses each component to one representative node, and returns the
/// quotient graph **G**/**G**|_W together with a mapping from old node indices to new ones.
///
/// Nodes outside `subset` retain their individual identities in the quotient.
///
/// **Returns** `(quotient_graph, mapping)` where `mapping[old_node] = new_node_index`.
///
/// This corresponds to the contraction used in Algorithm 95 to build
/// **F**_{n-1}(U) / **F**_{n-1}(ι(V)).
pub(super) fn contract(graph: &DiGraph, subset: &[usize]) -> (DiGraph, Vec<usize>) {
    let n = graph.node_count();

    // Union-Find on subset nodes to identify connected components,
    // treating directed edges as undirected (Lemma 89 requires path-induced subgraphs,
    // so we check connectivity undirectedly).
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

    let subset_set: Vec<bool> = {
        let mut s = vec![false; n];
        for &v in subset { s[v] = true; }
        s
    };

    // Union connected nodes within the subset (both directions to handle all paths).
    for &u in subset {
        for &v in &graph.successors[u] {
            if subset_set[v] { union(&mut parent, u, v); }
        }
        for &v in &graph.predecessors[u] {
            if subset_set[v] { union(&mut parent, u, v); }
        }
    }

    // Assign new node indices: one per connected component of the subset,
    // then one per node outside the subset.
    let mut new_idx: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    let mut mapping = vec![0usize; n];
    let mut next_id = 0usize;

    for &v in subset {
        let root = find(&mut parent, v);
        let id = *new_idx.entry(root).or_insert_with(|| {
            let id = next_id;
            next_id += 1;
            id
        });
        mapping[v] = id;
    }

    for v in 0..n {
        if !subset_set[v] {
            mapping[v] = next_id;
            next_id += 1;
        }
    }

    // Build the quotient graph: add an edge nu → nv for each original edge u → v
    // where the endpoints map to distinct new nodes (self-loops are dropped).
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

/// Outcome of enumerating topological orderings (returned by [`all_topological_sorts`]).
pub(super) enum TopoSortResult {
    /// All orderings were successfully collected (at most `limit` of them).
    Sorts(Vec<Vec<usize>>),
    /// The graph contains a directed cycle; no topological ordering exists.
    HasCycle,
    /// The number of distinct orderings exceeded the requested limit.
    LimitExceeded,
}

/// Enumerate all topological orderings of a DAG using Kahn's algorithm with backtracking.
///
/// At each backtracking step, every node currently at in-degree 0 is a valid next choice;
/// the algorithm tries each in turn and recurses.  State is restored on backtrack.
///
/// **Returns**:
/// - `TopoSortResult::Sorts(v)` — all orderings found (up to `limit`, or all if `None`)
/// - `TopoSortResult::HasCycle` — the graph is not a DAG
/// - `TopoSortResult::LimitExceeded` — more than `limit` orderings exist
///
/// Used by Algorithm 95 to iterate over all topological sorts of the contracted flow graph.
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

    // `usize::MAX` is the sentinel for "already scheduled" (avoids a separate `visited` array).
    let ready: Vec<usize> = (0..total).filter(|&v| indegrees[v] == 0).collect();

    if ready.is_empty() {
        // Nodes remain but none have in-degree 0: the graph has a cycle.
        return BacktrackOutcome::Cycle;
    }

    for v in ready {
        current.push(v);
        indegrees[v] = usize::MAX; // mark as scheduled

        let succs: Vec<usize> = graph.successors[v].clone();
        for &s in &succs {
            if indegrees[s] != usize::MAX { indegrees[s] -= 1; }
        }

        match topo_backtrack(graph, indegrees, current, total, result, limit) {
            BacktrackOutcome::Cycle => {
                for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
                indegrees[v] = 0;
                current.pop();
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

        // Restore and try the next candidate.
        for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
        indegrees[v] = 0;
        current.pop();

        if result.len() >= limit {
            return BacktrackOutcome::LimitExceeded;
        }
    }

    BacktrackOutcome::Done
}
