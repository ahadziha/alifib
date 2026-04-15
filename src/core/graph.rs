//! Lightweight directed graphs for flow graphs and topological sort enumeration.
//!
//! - [`DiGraph`] — a directed graph on nodes `0..n`, with sorted adjacency lists
//! - [`flow_graph`] — constructs the k-flow graph **F**_k(U)
//! - [`maximal_flow_graph`] — constructs the maximal k-flow graph **M**_k(U)
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
pub(crate) struct DiGraph {
    n: usize,
    /// `successors[u]` — sorted list of nodes that `u` points to.
    pub(crate) successors: Vec<IntSet>,
    /// `predecessors[v]` — sorted list of nodes that point to `v`.
    pub(crate) predecessors: Vec<IntSet>,
}

impl DiGraph {
    /// Create an empty graph with `n` nodes and no edges.
    pub(crate) fn new(n: usize) -> Self {
        Self {
            n,
            successors: vec![vec![]; n],
            predecessors: vec![vec![]; n],
        }
    }

    /// Number of nodes.
    pub(crate) fn node_count(&self) -> usize { self.n }

    /// Add a directed edge from `u` to `v`.  Idempotent (adding a duplicate edge is a no-op).
    pub(crate) fn add_edge(&mut self, u: usize, v: usize) {
        intset::insert(&mut self.successors[u], v);
        intset::insert(&mut self.predecessors[v], u);
    }

    /// In-degree of node `v`: the number of nodes with an edge to `v`.
    pub(crate) fn indegree(&self, v: usize) -> usize {
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

// ---- Topological sort enumeration ----

/// Compute a single topological sort of the graph using Kahn's algorithm.
///
/// Returns `Ok(order)` or `Err` if the graph has a cycle.
pub(crate) fn topological_sort(graph: &DiGraph) -> Result<Vec<usize>, ()> {
    let n = graph.node_count();
    let mut indeg: Vec<usize> = (0..n).map(|v| graph.indegree(v)).collect();
    let mut queue: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(v) = queue.pop() {
        order.push(v);
        for &s in &graph.successors[v] {
            indeg[s] -= 1;
            if indeg[s] == 0 { queue.push(s); }
        }
    }
    if order.len() == n { Ok(order) } else { Err(()) }
}

/// Try topological sorts one at a time, calling `f` for each. Stops when `f` returns
/// `Ok(value)` (success) or when all sorts have been tried. Returns `Err(())` if the
/// graph has a cycle, or `Err(())` if no sort satisfies `f`.
pub(crate) fn try_topological_sorts<T>(
    graph: &DiGraph,
    limit: usize,
    mut f: impl FnMut(&[usize]) -> Option<T>,
) -> Result<T, &'static str> {
    let n = graph.node_count();
    let mut indegrees: Vec<usize> = (0..n).map(|v| graph.indegree(v)).collect();
    let mut current: Vec<usize> = Vec::with_capacity(n);
    let mut found: Option<T> = None;
    let mut count = 0usize;

    let outcome = topo_try_backtrack(graph, &mut indegrees, &mut current, n, &mut f, &mut found, &mut count, limit);
    match outcome {
        TryBacktrackOutcome::Found => Ok(found.unwrap()),
        TryBacktrackOutcome::Cycle => Err("cycle"),
        TryBacktrackOutcome::Exhausted | TryBacktrackOutcome::LimitExceeded => Err("exhausted"),
        TryBacktrackOutcome::Continue => Err("exhausted"),
    }
}

enum TryBacktrackOutcome { Continue, Found, Cycle, LimitExceeded, Exhausted }

fn topo_try_backtrack<T>(
    graph: &DiGraph,
    indegrees: &mut Vec<usize>,
    current: &mut Vec<usize>,
    total: usize,
    f: &mut impl FnMut(&[usize]) -> Option<T>,
    found: &mut Option<T>,
    count: &mut usize,
    limit: usize,
) -> TryBacktrackOutcome {
    if current.len() == total {
        *count += 1;
        if *count > limit { return TryBacktrackOutcome::LimitExceeded; }
        if let Some(val) = f(current) {
            *found = Some(val);
            return TryBacktrackOutcome::Found;
        }
        return TryBacktrackOutcome::Continue;
    }

    let ready: Vec<usize> = (0..total).filter(|&v| indegrees[v] == 0).collect();
    if ready.is_empty() { return TryBacktrackOutcome::Cycle; }

    for v in ready {
        current.push(v);
        indegrees[v] = usize::MAX;
        let succs: Vec<usize> = graph.successors[v].clone();
        for &s in &succs {
            if indegrees[s] != usize::MAX { indegrees[s] -= 1; }
        }

        match topo_try_backtrack(graph, indegrees, current, total, f, found, count, limit) {
            TryBacktrackOutcome::Found => return TryBacktrackOutcome::Found,
            TryBacktrackOutcome::Cycle => {
                for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
                indegrees[v] = 0;
                current.pop();
                return TryBacktrackOutcome::Cycle;
            }
            TryBacktrackOutcome::LimitExceeded => {
                for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
                indegrees[v] = 0;
                current.pop();
                return TryBacktrackOutcome::LimitExceeded;
            }
            TryBacktrackOutcome::Continue | TryBacktrackOutcome::Exhausted => {}
        }

        for &s in &succs { if indegrees[s] != usize::MAX { indegrees[s] += 1; } }
        indegrees[v] = 0;
        current.pop();
    }

    TryBacktrackOutcome::Exhausted
}

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::{DiGraph, TopoSortResult, all_topological_sorts, flow_graph};
    use super::super::ogposet::Ogposet;

    fn chain_graph(n: usize) -> DiGraph {
        let mut g = DiGraph::new(n);
        for i in 0..n.saturating_sub(1) { g.add_edge(i, i + 1); }
        g
    }

    // ---- all_topological_sorts ----

    #[test]
    fn topo_sorts_empty_graph() {
        // 0-node graph has exactly one (empty) topological sort.
        let g = DiGraph::new(0);
        let TopoSortResult::Sorts(sorts) = all_topological_sorts(&g, None) else { panic!("expected Sorts"); };
        assert_eq!(sorts, vec![Vec::<usize>::new()]);
    }

    #[test]
    fn topo_sorts_chain() {
        // 0 → 1 → 2 has exactly one topological sort.
        let TopoSortResult::Sorts(sorts) = all_topological_sorts(&chain_graph(3), None) else { panic!(); };
        assert_eq!(sorts, vec![vec![0usize, 1, 2]]);
    }

    #[test]
    fn topo_sorts_diamond() {
        // 0 → {1, 2} → 3 has two topological sorts.
        let mut g = DiGraph::new(4);
        g.add_edge(0, 1); g.add_edge(0, 2);
        g.add_edge(1, 3); g.add_edge(2, 3);
        let TopoSortResult::Sorts(sorts) = all_topological_sorts(&g, None) else { panic!(); };
        assert_eq!(sorts.len(), 2);
        assert!(sorts.contains(&vec![0usize, 1, 2, 3]));
        assert!(sorts.contains(&vec![0usize, 2, 1, 3]));
    }

    #[test]
    fn topo_sorts_cycle() {
        // 0 → 1 → 0 has no topological sort.
        let mut g = DiGraph::new(2);
        g.add_edge(0, 1); g.add_edge(1, 0);
        assert!(matches!(all_topological_sorts(&g, None), TopoSortResult::HasCycle));
    }

    // ---- flow_graph ----

    #[test]
    fn flow_graph_two_arrow_paste() {
        // Build the 2-arrow paste directly as an Ogposet:
        //   0-cells: a(0), m(1), b(2)
        //   1-cells: f(0): a → m,  g(1): m → b
        //
        // F_0 should have 2 nodes (f and g) and 1 edge (f → g), because
        //   Δ⁺_0(f) = {m}  and  Δ⁻_0(g) = {m}  share the midpoint m.
        let u = Arc::new(Ogposet::make(
            1,
            vec![
                vec![vec![], vec![], vec![]],   // dim 0: 0-cells have no faces
                vec![vec![0], vec![1]],          // dim 1: f's in-face = {a(0)}, g's = {m(1)}
            ],
            vec![
                vec![vec![], vec![], vec![]],
                vec![vec![1], vec![2]],          // f's out-face = {m(1)}, g's = {b(2)}
            ],
            vec![
                vec![vec![0], vec![1], vec![]],  // dim 0 in-cofaces: a→{f}, m→{g}, b→{}
                vec![vec![], vec![]],
            ],
            vec![
                vec![vec![], vec![0], vec![1]],  // dim 0 out-cofaces: a→{}, m→{f}, b→{g}
                vec![vec![], vec![]],
            ],
        ));
        let (fg, node_map) = flow_graph(&u, 0);
        assert_eq!(fg.node_count(), 2, "one node per 1-cell");
        assert_eq!(node_map.len(), 2);
        // Node 0 = f, node 1 = g (in order of node_map construction).
        assert!(fg.successors[0].contains(&1), "f → g edge must exist (shared midpoint m)");
        assert!(!fg.successors[1].contains(&0), "no g → f edge (endpoints a and b are disjoint)");
    }
}
