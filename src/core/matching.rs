//! Subdiagram matching algorithms for regular molecules.
//!
//! Implements the three algorithms from Hadzihasanovic–Kessler (2304.09216, LICS 2023):
//!
//! - [`molecule_inclusions`] — find all shape-level inclusions ι: V → U (Algorithm 68)
//! - [`is_rewritable_submolecule`] — decide whether V ⊑ U (Algorithm 95)
//! - [`subdiagram_matches`] — full subdiagram matching combining both (Definition 59)
//!
//! The public entry point is [`Diagram::find_subdiagrams`].
//!
//! # Overview
//!
//! A *subdiagram match* of a pattern diagram `s: V → 𝕍` inside a target diagram
//! `t: U → 𝕍` (Definition 59) is an inclusion ι: V → U such that:
//! 1. Labels are preserved: `s = t ∘ ι` (checked in [`subdiagram_matches`]).
//! 2. V is a *rewritable submolecule* of U: V ⊑ U (checked by [`is_rewritable_submolecule`]).
//!
//! Finding such inclusions is split into two subproblems (Section II of the paper):
//!
//! **Molecule matching (Algorithm 68).**  Both V and U are regular molecules of the same
//! dimension n, and V is round.  The algorithm matches the top-dimensional atoms of V to
//! atoms of U one by one, guided by the (n-1)-flow graph **F**_{n-1}(V) (Definition 61),
//! which is connected by Proposition 66 (since V is round).  Once the first atom is fixed,
//! each subsequent atom has a unique candidate in U because any two flow-adjacent atoms in V
//! share a coface of a boundary (n-1)-cell whose image in U uniquely determines the next
//! match.  The implementation uses backtracking search instead of the paper's deterministic
//! extension, which is correct (and sound — the uniqueness argument means there is at most
//! one valid match at each step) but slightly less efficient.
//!
//! **Rewritable submolecule (Algorithm 95).**  Given ι: V ↪ U with dim V = dim U = n and
//! V round, decides V ⊑ U.  The algorithm builds the contracted flow graph
//! **G** := **F**_{n-1}(U) / **F**_{n-1}(ι(V)), enumerates its topological sorts, and for
//! each sort checks the layering boundary condition of Theorem 94.  For dim ≤ 2 the check
//! is trivially true (Corollary 120 / Theorem 121: every round submolecule of a ≤2-dimensional
//! molecule is rewritable).  For dim > 3 the recursive sub-problem is an open complexity
//! question (Section 125) and returns an error.

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
    /// Returns one [`Embedding`] (pattern shape → target shape) per match, filtered so that:
    /// 1. Labels are compatible: `pattern.labels[d][i] == target.labels[d][ι(i)]` for all cells.
    /// 2. The matched subshape is a rewritable submolecule (Definition 54).
    ///
    /// # Preconditions
    /// - `pattern` must be round (its shape satisfies [`Ogposet::is_round`]).
    /// - Both diagrams must have the same dimension.
    ///
    /// Returns `Err` if `pattern` is empty, dimensions differ, or the rewritability check
    /// cannot be completed (dim > 3; see [`is_rewritable_submolecule`]).
    pub fn find_subdiagrams(
        pattern: &Diagram,
        target: &Diagram,
    ) -> Result<Vec<Embedding>, Error> {
        subdiagram_matches(pattern, target)
    }
}

// ---- Subdiagram matching (Definition 59) ----

/// Combine molecule inclusion search with label and rewritability filtering.
fn subdiagram_matches(
    pattern: &Diagram,
    target: &Diagram,
) -> Result<Vec<Embedding>, Error> {
    // Step 1: shape-level inclusions V → U.
    let inclusions = molecule_inclusions(&pattern.shape, &target.shape)?;

    // Step 2: filter by label compatibility, then by rewritability.
    let mut result = Vec::new();
    for emb in inclusions {
        // Labels must match: pattern.labels[d][i] == target.labels[d][emb.map[d][i]].
        let labels_ok = emb.map.iter().enumerate().all(|(dim, row)| {
            let Some(pat_row) = pattern.labels.get(dim) else { return true; };
            let Some(tgt_row) = target.labels.get(dim) else { return false; };
            row.iter().enumerate().all(|(i, &j)| {
                pat_row.get(i).zip(tgt_row.get(j)).map_or(false, |(a, b)| a == b)
            })
        });
        if !labels_ok { continue; }

        // Rewritability check (Algorithm 95).
        if is_rewritable_submolecule(&emb)? {
            result.push(emb);
        }
    }
    Ok(result)
}

// ---- Molecule matching (Algorithm 68) ----

/// Precomputed data for a single top-dimensional atom (= the closure of one top-dim cell).
struct Atom {
    /// The atom as a sub-ogposet of the parent, with its embedding.
    shape: Arc<Ogposet>,
    /// Embedding cl{x} → parent.
    emb: Embedding,
    /// Normal form of `shape`, used for fast isomorphism grouping.
    normal: Arc<Ogposet>,
}

/// Precompute the atom for every top-dimensional cell of `g`.
///
/// For a pure regular molecule, these atoms are exactly the submolecules cl{x} for each
/// top-dimensional x (Proposition 43).
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

/// Find all inclusions ι: V → U of the molecule shape V into U (Algorithm 68).
///
/// # Algorithm sketch
/// 1. Compute the atoms (top-dim closures) of V and U.
/// 2. Build the (n-1)-flow graph of V and determine a traversal order for V's atoms
///    in which each atom (after the first) is flow-adjacent to some earlier one.
///    Such an order exists because **F**_{n-1}(V) is connected (Proposition 66).
/// 3. Seed the search by matching the first V-atom to every U-atom of the same isomorphism
///    type, then extend via backtracking: at each step, try every unused U-atom whose
///    normal form matches the current V-atom and whose image is consistent with the
///    partial map built so far.
///
/// # Preconditions
/// - `v.dim == u.dim`
/// - `v` is round ([`Ogposet::is_round`])
/// - `v.dim >= 0`
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
    // The unique 0-cell of V maps to each 0-cell of U.
    if n == 0 {
        let v_size = v.faces_in.get(0).map_or(0, |r| r.len());
        let u_size = u.faces_in.get(0).map_or(0, |r| r.len());
        if v_size != 1 { return Ok(vec![]); }
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

    // Build the (n-1)-flow graph of V and derive a traversal order.
    let (v_flow, _) = graph::flow_graph(v, n - 1);
    let v_order = flow_traversal_order(&v_flow, v_n);

    let v_sizes = v.sizes();
    let u_sizes = u.sizes();

    // `partial[dim][v_pos]` — the U-position assigned to V-cell (dim, v_pos), if any.
    let mut partial: Vec<Vec<Option<usize>>> = v_sizes.iter()
        .map(|&sz| vec![None; sz])
        .collect();
    // `used[ui]` — whether U-atom ui has already been matched.
    let mut used: Vec<bool> = vec![false; u_n];

    let mut results: Vec<Embedding> = Vec::new();
    let first_v = v_order[0];

    // Seed: match the first V-atom to each compatible U-atom.
    for first_u in 0..u_n {
        if !Ogposet::equal(&v_atoms[first_v].normal, &u_atoms[first_u].normal) {
            continue;
        }
        let Some(iso) = ogposet::find_isomorphism(
            &v_atoms[first_v].shape, &u_atoms[first_u].shape
        ).ok() else { continue; };

        // Consistency is trivially true for the first atom (partial map is empty).
        if !atom_iso_consistent(&iso, &v_atoms[first_v].emb, &u_atoms[first_u].emb, &partial) {
            continue;
        }

        let newly = apply_iso_to_partial(
            &iso, &v_atoms[first_v].emb, &u_atoms[first_u].emb, &mut partial,
        );
        used[first_u] = true;

        backtrack_match(
            v, u, &v_atoms, &u_atoms, &v_order, 1,
            &mut partial, &mut used, u_n,
            &v_sizes, &u_sizes, &mut results,
        );

        undo_iso_from_partial(&newly, &mut partial);
        used[first_u] = false;
    }

    Ok(results)
}

/// Recursive backtracking for the molecule matching loop (steps 2..m of Algorithm 68).
///
/// At each level `step`, selects the next V-atom `v_order[step]` and tries all compatible
/// unused U-atoms.  A U-atom is compatible if its isomorphism type matches and the induced
/// cell assignments are consistent with the current partial map.
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
    if step == v_order.len() {
        // All V top-cells are matched; materialise the embedding.
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

/// Determine a traversal order for V's top-dimensional atoms such that each atom
/// (after the first) shares a flow edge with some earlier atom in the order.
///
/// This is a BFS over the (n-1)-flow graph starting from atom 0.  Since V is round,
/// **F**_{n-1}(V) is connected (Proposition 66), so all atoms are reached.
/// Any remaining atoms (disconnected components, which should not arise for round V)
/// are appended at the end.
fn flow_traversal_order(flow: &DiGraph, n_top: usize) -> Vec<usize> {
    let mut visited = vec![false; n_top];
    let mut order = Vec::with_capacity(n_top);
    if n_top == 0 { return order; }

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

    // Safety net for disconnected components (should not occur for round V).
    for v in 0..n_top {
        if !visited[v] { order.push(v); }
    }
    order
}

/// Check whether an atom isomorphism `iso: cl{x} → cl{a}` is consistent with the
/// existing partial map.
///
/// For each cell `c` in cl{x}, the partial map may already have an assignment for the
/// corresponding V-cell (via a previously-matched atom that shared lower-dimensional
/// boundary with cl{x}).  The isomorphism is *consistent* iff every such existing
/// assignment agrees with the U-cell that `iso` would assign.
///
/// - `atom_emb_v`: the embedding cl{x} → V
/// - `atom_emb_u`: the embedding cl{a} → U
fn atom_iso_consistent(
    iso: &Embedding,
    atom_emb_v: &Embedding,
    atom_emb_u: &Embedding,
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

/// Apply an atom isomorphism to the partial map and return the newly-assigned positions.
///
/// For each cell `c` in the atom, computes the V-position and U-position and, if the
/// V-position is not yet assigned in `partial`, writes the assignment.  Only cells
/// that were `None` before this call are written; cells already assigned (shared with
/// a previously-matched atom) are left untouched.
///
/// Returns the list of `(dim, v_pos)` pairs that were **newly assigned**.  The caller
/// must pass this list to [`undo_iso_from_partial`] to correctly undo on backtrack.
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

/// Undo the effect of [`apply_iso_to_partial`] by clearing exactly the positions that
/// were newly assigned.
///
/// Only the positions in `newly_assigned` are cleared; cells that were already set before
/// `apply_iso_to_partial` was called (shared boundary with an earlier-matched atom) are
/// left untouched.
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

/// Convert a fully-populated partial map to an [`Embedding`] V → U.
///
/// Returns `None` if any V-cell is still unmapped (which should not happen when called
/// from [`backtrack_match`] after all atoms have been matched).
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

/// Decide whether the inclusion `emb: V ↪ U` makes V a rewritable submolecule of U
/// (V ⊑ U, Definition 54).
///
/// Implements Algorithm 95 from Hadzihasanovic–Kessler (2304.09216).
///
/// # Correctness range
/// - For dim V ≤ 2: always returns `Ok(true)` by Corollary 120 / Theorem 121
///   (all round molecules are stably frame-acyclic up to dimension 3, so every
///   inclusion of round molecules of dimension ≤ 2 is a submolecule inclusion).
/// - For dim V = 3 (the algorithm runs with k = 2): fully implemented.
/// - For dim V > 3: returns `Err`, since a polynomial algorithm is an open problem
///   (Section 125 of the paper).
///
/// # Preconditions
/// - `emb.dom` (= V) must be round.
/// - `dim(emb.dom) == dim(emb.cod)`.
pub(super) fn is_rewritable_submolecule(emb: &Embedding) -> Result<bool, Error> {
    let v = &emb.dom;

    if v.dim < 0 { return Ok(true); }
    let n = v.dim as usize;

    // Corollary 120 / Theorem 121: every inclusion of round ≤2-dimensional molecules
    // is automatically a submolecule inclusion (stable frame-acyclicity holds for all
    // regular molecules of dimension ≤ 3).
    if n <= 2 { return Ok(true); }

    is_rewritable_inner(&emb.dom, &emb.cod, emb, n)
}

/// Inner recursive implementation of Algorithm 95 for n = dim V = dim U > 2.
fn is_rewritable_inner(
    v: &Arc<Ogposet>,
    u: &Arc<Ogposet>,
    emb: &Embedding,
    n: usize,
) -> Result<bool, Error> {
    if n <= 2 { return Ok(true); }

    let k = n - 1; // working dimension for the flow graph and layering

    // Build F_k(U).
    let (u_flow, u_node_map) = graph::flow_graph(u, k);

    // Identify which nodes of u_flow correspond to cells in ι(V).
    let v_image_nodes = image_nodes_in_flow(&u_node_map, emb, n);

    // Construct G := F_k(U) / F_k(ι(V)).
    let (g_contracted, old_to_new) = graph::contract(&u_flow, &v_image_nodes);

    // The contracted-image node index in the quotient (usize::MAX if V's image is empty).
    let v_contracted_node = if v_image_nodes.is_empty() { usize::MAX }
    else { old_to_new[v_image_nodes[0]] };

    // Enumerate topological sorts of G (with a safety limit).
    // By Lemma 89, a cycle in G means F_k(ι(V)) is not path-induced in F_k(U),
    // which means V is not a submolecule of U.
    let sorts = match graph::all_topological_sorts(&g_contracted, Some(10_000)) {
        TopoSortResult::Sorts(s) => s,
        TopoSortResult::HasCycle => return Ok(false),
        TopoSortResult::LimitExceeded => return Err(Error::new(
            "rewritable submolecule: too many topological sorts (limit exceeded)"
        )),
    };

    // For each topological sort, check the layering boundary condition (Theorem 94).
    for sort in &sorts {
        if check_sort_condition(u, v, emb, &u_node_map, &old_to_new,
            v_contracted_node, sort, k)? {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Collect the node indices in the flow graph that correspond to cells in the image of V.
///
/// Because the flow graph is built with k = n-1, all its nodes are at dimension n
/// (there is only one dimension > k = n-1).  The function maps V's top-dim cells
/// through `emb` to U's top-dim positions and finds their corresponding graph nodes.
fn image_nodes_in_flow(
    node_map: &[(usize, usize)],
    emb: &Embedding,
    n: usize,
) -> Vec<usize> {
    let v_top_count = emb.dom.faces_in.get(n).map_or(0, |r| r.len());
    let u_top_positions: Vec<usize> = (0..v_top_count)
        .filter_map(|i| emb.map.get(n).and_then(|row| row.get(i)).copied())
        .collect();

    node_map.iter().enumerate()
        .filter(|&(_, &(dim, pos))| dim == n && u_top_positions.contains(&pos))
        .map(|(idx, _)| idx)
        .collect()
}

/// Check whether a given topological sort of the contracted flow graph witnesses
/// that V is a rewritable submolecule of U.
///
/// Implements the condition of Theorem 94 of Hadzihasanovic–Kessler:
/// Given a topological sort `((x^(i))_{i<q}, x_V, (x^(i))_{i>q})` of **G**,
/// build the layering `U^(0) = ∂⁻U`, `U^(i) = ∂⁺_k U^(i-1) ∪ cl{x^(i)}` for i ≠ q,
/// and check:
/// - `∂⁻_k x^(i) ⊆ ∂⁻_k U^(i)` for all i ≠ q, and
/// - `ι(∂⁻_k V) ⊆ ∂⁻_k U^(q)` for i = q.
///
/// The containment `⊆` at the k-cell level suffices (rather than full `⊑`) because
/// Corollary 120 guarantees rewritability at dimension ≤ 2, which covers k ≤ 2
/// (the only dimensions for which this function is fully implemented).
///
/// Returns `Ok(true)` if the condition holds, `Ok(false)` if it fails,
/// and `Err` if the recursive sub-check for k > 2 is required.
fn check_sort_condition(
    u: &Arc<Ogposet>,
    v: &Arc<Ogposet>,
    emb: &Embedding,
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

    let n = u.dim as usize;

    // Track ∂⁻_k U^(i): initially ∂⁻_k U (k-cells with no output cofaces).
    let mut current_in_bd: IntSet = u.extremal(Sign::Input, k);

    for (i, &contracted_node) in sort.iter().enumerate() {
        // The U top-cells belonging to this contracted node.
        let top_cells_here: Vec<usize> = u_node_map.iter().enumerate()
            .filter(|&(fi, &(dim, _pos))| dim == n && old_to_new[fi] == contracted_node)
            .map(|(_fi, &(_dim, pos))| pos)
            .collect();

        if i == q {
            // V's layer: check ι(∂⁻_k V) ⊆ current_in_bd.
            let v_input_k: IntSet = v.extremal(Sign::Input, k);
            let v_input_k_in_u: IntSet = intset::collect_sorted(
                v_input_k.iter().filter_map(|&vk| {
                    emb.map.get(k).and_then(|row| row.get(vk)).copied()
                })
            );
            if !intset::difference(&v_input_k_in_u, &current_in_bd).is_empty() {
                return Ok(false);
            }

            // Advance: ∂⁻_k U^(q+1) = (current_in_bd \ ι(∂⁻_k V)) ∪ ι(∂⁺_k V).
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
            // Regular layer for top-dim cell x^(i): check ∂⁻_k x^(i) ⊆ current_in_bd.
            for &x_pos in &top_cells_here {
                let x_input_k = ogposet::signed_k_boundary_of_cell(u, Sign::Input, k, n, x_pos);
                if !intset::difference(&x_input_k, &current_in_bd).is_empty() {
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

    // For k ≤ 2, the containment check above suffices by Corollary 120: every inclusion
    // of round molecules at dimension ≤ 2 is a rewritable submolecule inclusion, so the
    // boundary-level check (⊆) automatically upgrades to the submolecule relation (⊑).
    if k <= 2 { return Ok(true); }

    // For k > 2, a recursive submolecule check at dimension k is required to verify
    // ι(∂⁻V) ⊑ ∂⁻U^(q).  This is not yet implemented; a polynomial algorithm for
    // dim > 3 is an open problem (Section 125 of the paper).
    Err(Error::new(
        "rewritable submolecule: recursive check for dim > 3 not yet implemented"
    ))
}
