//! Subdiagram matching algorithms for regular molecules.
//!
//! Implements the three algorithms from Hadzihasanovic–Kessler (2304.09216):
//!
//! - [`molecule_inclusions`] — find all shape-level inclusions V → U (Algorithm 68)
//! - [`is_rewritable_submolecule`] — decide whether V ⊑ U (Algorithm 95)
//! - [`subdiagram_matches`] — full subdiagram matching combining both (Definition 59)
//!
//! The public entry point is [`Diagram::find_subdiagrams`].

use std::sync::Arc;
use crate::aux::Error;
use super::diagram::Diagram;
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::graph::{self, DiGraph, TopoSortResult};
use super::intset::{self, IntSet};
use super::ogposet::{self, Ogposet, Sign};

// ---- Public API ----

impl Diagram {
    /// Find all rewritable subdiagram matches of `pattern` inside `target`.
    ///
    /// Returns one [`Embedding`] (pattern shape → target shape) for each match,
    /// filtered so that:
    /// 1. Labels are compatible at every cell.
    /// 2. The matched subshape is a rewritable submolecule.
    ///
    /// `pattern` must be round; both diagrams must have the same dimension.
    pub fn find_subdiagrams(
        pattern: &Diagram,
        target: &Diagram,
    ) -> Result<Vec<Embedding>, Error> {
        subdiagram_matches(pattern, target)
    }
}

// ---- Subdiagram matching (Def 59) ----

fn subdiagram_matches(
    pattern: &Diagram,
    target: &Diagram,
) -> Result<Vec<Embedding>, Error> {
    // Step 1: shape-level inclusions.
    let inclusions = molecule_inclusions(&pattern.shape, &target.shape)?;

    // Step 2: filter by label compatibility, then rewritability.
    let mut result = Vec::new();
    for emb in inclusions {
        // Check labels: pattern.labels[dim][i] == target.labels[dim][emb.map[dim][i]]
        let labels_ok = emb.map.iter().enumerate().all(|(dim, row)| {
            let Some(pat_row) = pattern.labels.get(dim) else { return true; };
            let Some(tgt_row) = target.labels.get(dim) else { return false; };
            row.iter().enumerate().all(|(i, &j)| {
                pat_row.get(i).zip(tgt_row.get(j)).map_or(false, |(a, b)| a == b)
            })
        });
        if !labels_ok { continue; }

        // Step 3: rewritability check.
        if is_rewritable_submolecule(&emb)? {
            result.push(emb);
        }
    }
    Ok(result)
}

// ---- Molecule matching (Algorithm 68) ----

/// Precomputed atom data for a single top-dimensional cell.
struct Atom {
    /// The atom as a sub-ogposet, together with the embedding into the parent.
    shape: Arc<Ogposet>,
    emb: Embedding,
    /// The normal form of the atom's shape (used for fast isomorphism grouping).
    normal: Arc<Ogposet>,
}

/// Precompute the atom for every top-dimensional cell of `g`.
fn compute_atoms(g: &Arc<Ogposet>) -> Vec<Atom> {
    if g.dim < 0 { return vec![]; }
    let n = g.dim as usize;
    let n_top = g.faces_in[n].len();
    (0..n_top).map(|pos| {
        let seeds: IntSet = vec![pos];
        let (shape, emb) = ogposet::traverse(g, vec![(n, seeds)], false);
        let (normal, _) = ogposet::normalisation(&shape);
        Atom { shape, emb, normal }
    }).collect()
}

/// Find all inclusions of the molecule shape V into U (Algorithm 68).
///
/// Preconditions: `v.dim == u.dim`, `v.is_round()`, `v.dim >= 0`.
pub(super) fn molecule_inclusions(
    v: &Arc<Ogposet>,
    u: &Arc<Ogposet>,
) -> Result<Vec<Embedding>, Error> {
    if v.dim < 0 {
        return Err(Error::new("molecule matching: pattern is empty"));
    }
    if v.dim != u.dim {
        return Err(Error::new("molecule matching: dimensions do not match"));
    }
    if !v.is_round() {
        return Err(Error::new("molecule matching: pattern is not round"));
    }

    let n = v.dim as usize;

    // Trivial 0-dimensional case: V and U are single points.
    if n == 0 {
        let v_size = v.faces_in.get(0).map_or(0, |r| r.len());
        let u_size = u.faces_in.get(0).map_or(0, |r| r.len());
        if v_size != 1 { return Ok(vec![]); }
        // Match the single 0-cell of V to each 0-cell of U.
        return Ok((0..u_size).map(|pos| {
            let map = vec![vec![pos]];
            let mut inv = vec![vec![NO_PREIMAGE; u_size]];
            inv[0][pos] = 0;
            Embedding::make(Arc::clone(v), Arc::clone(u), map, inv)
        }).collect());
    }

    let v_atoms = compute_atoms(v);
    let u_atoms = compute_atoms(u);

    if v_atoms.is_empty() { return Ok(vec![]); }

    let v_n = v_atoms.len();
    let u_n = u_atoms.len();

    // Build the (n-1)-flow graph of V to determine matching order.
    let (v_flow, _) = graph::flow_graph(v, n - 1);

    // Compute a valid traversal order for V's top-dim cells: each element
    // (after the first) must be flow-adjacent to some earlier element.
    // V is round so F_{n-1}(V) is connected (Prop 66).
    let v_order = flow_traversal_order(&v_flow, v_n);

    // Initialise partial map: dim -> Vec<Option<usize>> (V-pos -> U-pos).
    let v_sizes = v.sizes();
    let u_sizes = u.sizes();

    let mut partial: Vec<Vec<Option<usize>>> = v_sizes.iter()
        .map(|&sz| vec![None; sz])
        .collect();
    let mut used: Vec<bool> = vec![false; u_n]; // which U top-cells are taken

    let mut results: Vec<Embedding> = Vec::new();
    let first_v = v_order[0];

    // Try seeding with each U atom that has the same normal form as v_atoms[first_v].
    for first_u in 0..u_n {
        if !Ogposet::equal(&v_atoms[first_v].normal, &u_atoms[first_u].normal) {
            continue;
        }
        // Find the isomorphism cl{v_order[0]} -> cl{first_u}.
        let Some(iso) = ogposet::find_isomorphism(
            &v_atoms[first_v].shape, &u_atoms[first_u].shape
        ).ok() else { continue; };

        // Check consistency (trivially true for first atom since map is empty).
        if !atom_iso_consistent(&iso, &v_atoms[first_v].emb, &u_atoms[first_u].emb, &partial) {
            continue;
        }

        // Apply the isomorphism to the partial map; track newly-assigned positions.
        let newly = apply_iso_to_partial(
            &iso, &v_atoms[first_v].emb, &u_atoms[first_u].emb, &mut partial,
        );
        used[first_u] = true;

        // Extend along the flow order.
        backtrack_match(
            v, u, &v_atoms, &u_atoms, &v_order, 1,
            &mut partial, &mut used, u_n,
            &v_sizes, &u_sizes, &mut results,
        );

        // Undo exactly what we assigned.
        undo_iso_from_partial(&newly, &mut partial);
        used[first_u] = false;
    }

    Ok(results)
}

/// Recursive backtracking for molecule matching.
#[allow(clippy::too_many_arguments)]
fn backtrack_match(
    v: &Arc<Ogposet>,
    u: &Arc<Ogposet>,
    v_atoms: &[Atom],
    u_atoms: &[Atom],
    v_order: &[usize],
    step: usize,
    partial: &mut Vec<Vec<Option<usize>>>,
    used: &mut Vec<bool>,
    u_n: usize,
    v_sizes: &[usize],
    u_sizes: &[usize],
    results: &mut Vec<Embedding>,
) {
    let _n = v.dim as usize;

    if step == v_order.len() {
        // All V top-cells matched. Convert partial map to Embedding.
        if let Some(emb) = partial_to_embedding(v, u, partial, v_sizes, u_sizes) {
            results.push(emb);
        }
        return;
    }

    let vi = v_order[step];
    for ui in 0..u_n {
        if used[ui] { continue; }
        if !Ogposet::equal(&v_atoms[vi].normal, &u_atoms[ui].normal) {
            continue;
        }
        let Some(iso) = ogposet::find_isomorphism(
            &v_atoms[vi].shape, &u_atoms[ui].shape
        ).ok() else { continue; };

        if !atom_iso_consistent(&iso, &v_atoms[vi].emb, &u_atoms[ui].emb, partial) {
            continue;
        }

        let newly = apply_iso_to_partial(&iso, &v_atoms[vi].emb, &u_atoms[ui].emb, partial);
        used[ui] = true;

        backtrack_match(v, u, v_atoms, u_atoms, v_order, step + 1,
            partial, used, u_n, v_sizes, u_sizes, results);

        undo_iso_from_partial(&newly, partial);
        used[ui] = false;
    }
}

/// Determine a traversal order for V's top-dim cells such that each cell
/// (after the first) is reachable from the earlier ones in the flow graph.
///
/// This is a BFS/DFS order on the flow graph.  Since V is round, F_{n-1}(V)
/// is connected (Prop 66), so all cells will be reached.
fn flow_traversal_order(flow: &DiGraph, n_top: usize) -> Vec<usize> {
    let mut visited = vec![false; n_top];
    let mut order = Vec::with_capacity(n_top);
    if n_top == 0 { return order; }

    // Start from node 0.
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(0usize);
    visited[0] = true;

    while let Some(v) = queue.pop_front() {
        order.push(v);
        for &s in &flow.successors[v] {
            if !visited[s] { visited[s] = true; queue.push_back(s); }
        }
        for &p in &flow.predecessors[v] {
            if !visited[p] { visited[p] = true; queue.push_back(p); }
        }
    }

    // In case the flow graph is disconnected (shouldn't happen for round V),
    // append any remaining nodes.
    for v in 0..n_top {
        if !visited[v] { order.push(v); }
    }
    order
}

/// Check whether an atom isomorphism `iso: cl{x} -> cl{a}` is consistent
/// with the existing partial map on overlapping cells.
///
/// For each cell c in cl{x}, if the partial map already has an assignment for
/// the corresponding V-cell, that assignment must match the U-cell that `iso`
/// would produce.
fn atom_iso_consistent(
    iso: &Embedding,
    atom_emb_v: &Embedding,  // cl{x} -> V
    atom_emb_u: &Embedding,  // cl{a} -> U
    partial: &[Vec<Option<usize>>],
) -> bool {
    for (dim, iso_row) in iso.map.iter().enumerate() {
        let Some(ev_row) = atom_emb_v.map.get(dim) else { continue; };
        let Some(eu_row) = atom_emb_u.map.get(dim) else { continue; };
        for (atom_pos, &iso_img) in iso_row.iter().enumerate() {
            let Some(&v_pos) = ev_row.get(atom_pos) else { continue; };
            let Some(&u_pos) = eu_row.get(iso_img) else { continue; };
            if let Some(existing) = partial.get(dim).and_then(|r| r.get(v_pos)).copied().flatten() {
                if existing != u_pos { return false; }
            }
        }
    }
    true
}

/// Apply an atom isomorphism to the partial map (write assignments).
///
/// For each atom cell c, assigns `partial[dim][v_pos] = u_pos` if not yet set.
/// Returns the list of `(dim, v_pos)` positions that were **newly assigned**
/// (transitioned from None to Some).  The caller must pass this list to
/// `undo_iso_from_partial` to correctly reverse the effect on backtrack.
fn apply_iso_to_partial(
    iso: &Embedding,
    atom_emb_v: &Embedding,
    atom_emb_u: &Embedding,
    partial: &mut Vec<Vec<Option<usize>>>,
) -> Vec<(usize, usize)> {
    let mut newly_assigned: Vec<(usize, usize)> = Vec::new();
    for (dim, iso_row) in iso.map.iter().enumerate() {
        let Some(ev_row) = atom_emb_v.map.get(dim) else { continue; };
        let Some(eu_row) = atom_emb_u.map.get(dim) else { continue; };
        for (atom_pos, &iso_img) in iso_row.iter().enumerate() {
            let Some(&v_pos) = ev_row.get(atom_pos) else { continue; };
            let Some(&u_pos) = eu_row.get(iso_img) else { continue; };
            if let Some(row) = partial.get_mut(dim) {
                if row[v_pos].is_none() {
                    row[v_pos] = Some(u_pos);
                    newly_assigned.push((dim, v_pos));
                }
            }
        }
    }
    newly_assigned
}

/// Undo the effect of `apply_iso_to_partial` by clearing exactly the positions
/// that were newly assigned.
///
/// Only the positions returned by the corresponding `apply_iso_to_partial` call
/// are cleared; cells that were already set (shared with a previously-matched
/// atom) are left untouched.
fn undo_iso_from_partial(
    newly_assigned: &[(usize, usize)],
    partial: &mut Vec<Vec<Option<usize>>>,
) {
    for &(dim, v_pos) in newly_assigned {
        if let Some(row) = partial.get_mut(dim) {
            row[v_pos] = None;
        }
    }
}

/// Convert the completed partial map to a full `Embedding` V -> U.
///
/// Returns `None` if any V-cell is unmapped (should not happen on success).
fn partial_to_embedding(
    v: &Arc<Ogposet>,
    u: &Arc<Ogposet>,
    partial: &[Vec<Option<usize>>],
    v_sizes: &[usize],
    u_sizes: &[usize],
) -> Option<Embedding> {
    let dims = v_sizes.len();
    let mut map: Vec<Vec<usize>> = Vec::with_capacity(dims);
    let mut inv: Vec<Vec<usize>> = u_sizes.iter().map(|&s| vec![NO_PREIMAGE; s]).collect();

    for (dim, row) in partial.iter().enumerate() {
        let mut map_row = Vec::with_capacity(row.len());
        for (v_pos, &opt) in row.iter().enumerate() {
            let u_pos = opt?;
            map_row.push(u_pos);
            if dim < inv.len() {
                inv[dim][u_pos] = v_pos;
            }
        }
        map.push(map_row);
    }

    Some(Embedding::make(Arc::clone(v), Arc::clone(u), map, inv))
}

// ---- Rewritable submolecule decision (Algorithm 95) ----

/// Decide whether the inclusion `emb: V -> U` makes V a rewritable submolecule
/// of U (i.e., V ⊑ U, Definition 54).
///
/// Implements Algorithm 95 from Hadzihasanovic–Kessler (2304.09216).
pub(super) fn is_rewritable_submolecule(emb: &Embedding) -> Result<bool, Error> {
    let v = &emb.dom;
    let u = &emb.cod;

    // Base cases.
    if v.dim < 0 { return Ok(true); }
    let n = v.dim as usize;

    // Dimensions ≤ 2: every inclusion of round molecules is a submolecule
    // inclusion (Corollary 120 / Theorem 121 of the paper).
    if n <= 2 { return Ok(true); }

    // General case: Algorithm 95.
    is_rewritable_inner(v, u, emb, n)
}

/// Inner recursive implementation of Algorithm 95.
fn is_rewritable_inner(
    v: &Arc<Ogposet>,
    u: &Arc<Ogposet>,
    emb: &Embedding,
    n: usize,
) -> Result<bool, Error> {
    if n <= 2 { return Ok(true); }

    let k = n - 1; // layering dimension

    // Build F_{k}(U).
    let (u_flow, u_node_map) = graph::flow_graph(u, k);

    // Identify which nodes of u_flow correspond to V's image.
    let v_image_nodes = image_nodes_in_flow(&u_flow, &u_node_map, emb, n);

    // Construct G := F_{k}(U) / F_{k}(V's image).
    let (g_contracted, old_to_new) = graph::contract(&u_flow, &v_image_nodes);

    // The contracted V-image node index in the quotient.
    let v_contracted_node = if v_image_nodes.is_empty() { usize::MAX }
    else { old_to_new[v_image_nodes[0]] };

    // Enumerate topological sorts of G (with a limit).
    let sorts = match graph::all_topological_sorts(&g_contracted, Some(10_000)) {
        TopoSortResult::Sorts(s) => s,
        // A cycle means the contracted graph is not a DAG, so by Lemma 89
        // the induced subgraph on V's image is not path-induced → V ⋢ U.
        TopoSortResult::HasCycle => return Ok(false),
        TopoSortResult::LimitExceeded => return Err(Error::new(
            "rewritable submolecule: too many topological sorts (limit exceeded)"
        )),
    };

    // For each topological sort, check the layering boundary condition.
    for sort in &sorts {
        if check_sort_condition(u, v, emb, &u_flow, &u_node_map, &old_to_new,
            v_contracted_node, sort, k)? {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Collect the indices (in the flow graph) of nodes corresponding to cells in
/// the image of V under `emb`.
fn image_nodes_in_flow(
    _flow: &DiGraph,
    node_map: &[(usize, usize)],
    emb: &Embedding,
    n: usize,
) -> Vec<usize> {
    // The image of V's top-dim cells under emb.
    let v_top_count = emb.dom.faces_in.get(n).map_or(0, |r| r.len());
    let u_top_positions: Vec<usize> = (0..v_top_count)
        .filter_map(|i| emb.map.get(n).and_then(|row| row.get(i)).copied())
        .collect();

    node_map.iter().enumerate()
        .filter(|&(_, &(dim, pos))| dim == n && u_top_positions.contains(&pos))
        .map(|(idx, _)| idx)
        .collect()
}

/// Check whether a given topological sort of the contracted flow graph
/// induces a valid layering under which V is a rewritable submolecule.
///
/// Corresponds to the main loop body of Algorithm 95.
fn check_sort_condition(
    u: &Arc<Ogposet>,
    v: &Arc<Ogposet>,
    emb: &Embedding,
    _u_flow: &DiGraph,
    u_node_map: &[(usize, usize)],
    old_to_new: &[usize],
    v_node: usize,
    sort: &[usize],
    k: usize,
) -> Result<bool, Error> {
    if v_node == usize::MAX { return Ok(true); }

    // Find position q of v_node in the sort.
    let q = match sort.iter().position(|&x| x == v_node) {
        Some(p) => p,
        None => return Ok(false),
    };

    // Build layers of U according to the topological sort.
    // Layer i corresponds to sort[i] in the contracted quotient.
    // We need to construct U^(0) = ∂⁻U and U^(i) = ∂⁺_{k} U^(i-1) ∪ cl{x^(i)}.
    // The key check is: ∂⁻x^(i) ⊑ ∂⁻U^(i) for i ≠ q, and ∂⁻V ⊑ ∂⁻U^(q).

    // The sort over the contracted graph; each entry either corresponds to
    // a single U top-cell (if outside V's image) or to V's contracted node.
    let n = u.dim as usize;

    // ∂⁻_k U consists of the k-cells in the input boundary of U.
    let input_extremal_k: IntSet = u.extremal(Sign::Input, k);

    // We track the set of k-cells that are "in the current input boundary"
    // as we step through the layering.
    let mut current_in_bd: IntSet = input_extremal_k;

    for (i, &contracted_node) in sort.iter().enumerate() {
        // Determine which U top-cells belong to this contracted node.
        let top_cells_here: Vec<usize> = u_node_map.iter().enumerate()
            .filter(|&(fi, &(dim, _pos))| {
                dim == n && old_to_new[fi] == contracted_node
            })
            .map(|(_fi, &(_dim, pos))| pos)
            .collect();

        if i == q {
            // This is V's layer. Check ∂⁻V ⊆ current_in_bd.
            let v_input_k: IntSet = v.extremal(Sign::Input, k);
            // Map V's k-cells to U's k-cells via emb.
            let v_input_k_in_u: IntSet = intset::collect_sorted(
                v_input_k.iter().filter_map(|&vk| {
                    emb.map.get(k).and_then(|row| row.get(vk)).copied()
                })
            );
            // Check containment: v_input_k_in_u ⊆ current_in_bd.
            let diff = intset::difference(&v_input_k_in_u, &current_in_bd);
            if !diff.is_empty() {
                return Ok(false);
            }

            // Advance current_in_bd by: remove V's input k-cells, add V's output k-cells.
            let v_output_k: IntSet = v.extremal(Sign::Output, k);
            let v_output_k_in_u: IntSet = intset::collect_sorted(
                v_output_k.iter().filter_map(|&vk| {
                    emb.map.get(k).and_then(|row| row.get(vk)).copied()
                })
            );
            current_in_bd = intset::union(
                &intset::difference(&current_in_bd, &v_input_k_in_u),
                &v_output_k_in_u,
            );
        } else {
            // Regular layer: one top-dim element x^(i).
            // Check ∂⁻x^(i) ⊆ current_in_bd.
            for &x_pos in &top_cells_here {
                let x_input_k = ogposet::signed_k_boundary_of_cell(u, Sign::Input, k, n, x_pos);
                let diff = intset::difference(&x_input_k, &current_in_bd);
                if !diff.is_empty() {
                    return Ok(false);
                }
                // Advance: remove x's input k-boundary, add x's output k-boundary.
                let x_output_k = ogposet::signed_k_boundary_of_cell(u, Sign::Output, k, n, x_pos);
                current_in_bd = intset::union(
                    &intset::difference(&current_in_bd, &x_input_k),
                    &x_output_k,
                );
            }
        }
    }

    // Recursive check: verify that ∂⁻V ⊑ ∂⁻U^(q) at dimension k.
    // For k ≤ 2, the submolecule inclusion at dimension ≤ 2 is trivially true
    // (Corollary 120), so the boundary containment checked above is sufficient.
    if k <= 2 { return Ok(true); }

    // For k > 2 the recursive submolecule check at dimension k is not yet
    // implemented.  The polynomial algorithm for dim > 3 is an open problem
    // (Section 125 of the paper).
    Err(Error::new(
        "rewritable submolecule: recursive check for dim > 3 not yet implemented"
    ))
}
