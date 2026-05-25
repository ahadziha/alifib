//! Lightweight directed graphs and topological sort enumeration.
//!
//! - [`DiGraph`] — a directed graph on nodes `0..n`, with sorted adjacency lists
//! - [`topological_sort`] — Kahn's algorithm for a single topological ordering
//! - [`try_topological_sorts`] — backtracking enumeration of topological orderings
//!
//! All graph nodes are identified by small non-negative integers.  Adjacency lists are
//! [`IntSet`](super::intset::IntSet) values — sorted, deduplicated `Vec<usize>` — which
//! support O(n+m) set operations and binary-search membership queries.

use super::intset::{self, IntSet};

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

    pub(crate) fn has_any_edge(&self) -> bool {
        self.successors.iter().any(|s| !s.is_empty())
    }
}

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
