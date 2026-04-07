//! Oriented graded posets (ogposets): the combinatorial shapes underlying diagrams.
//!
//! An [`Ogposet`] records cells at each dimension together with their signed
//! face and coface adjacency.  The module provides the key operations used to
//! build new ogposets from old ones:
//!
//! - [`boundary`] — extract the sub-ogposet on the sign-side k-boundary
//! - [`boundary_traverse`] — normalised boundary (memoised)
//! - [`normalisation`] — canonical cell ordering (memoised)
//! - [`find_isomorphism`] — decide shape isomorphism via canonical forms
//! - [`traverse`] — general sub-ogposet traversal (drives all of the above)

use std::sync::Arc;
use std::cell::RefCell;
use std::collections::HashMap;
use crate::aux::Error;
use super::bitset::BitSet;
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::intset::{self, IntSet};

fn set_map(f: impl Fn(usize) -> usize, s: &IntSet) -> IntSet {
    intset::collect_sorted(s.iter().map(|&x| f(x)))
}

fn set_filter_map(f: impl Fn(usize) -> Option<usize>, s: &IntSet) -> IntSet {
    intset::collect_sorted(s.iter().filter_map(|&x| f(x)))
}

// ---- Sign ----

/// Face sign in an oriented graded poset.
///
/// Every face relation carries a sign indicating whether it is a source (`Input`,
/// written δ⁻) or target (`Output`, written δ⁺) face.  `Both` is a convenience
/// variant meaning "either sign".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Sign {
    Input,
    Output,
    Both,
}

// ---- Ogposet ----

/// An oriented graded poset (ogposet): the combinatorial shape of a diagram.
///
/// Cells are indexed per dimension, 0..=`dim`.  For a cell `p` at dimension `d`:
/// - `faces_in[d][p]`    — input (source, δ⁻) faces at dimension d-1
/// - `faces_out[d][p]`   — output (target, δ⁺) faces at dimension d-1
/// - `cofaces_in[d][p]`  — cells at dimension d+1 that have `p` as an input face
/// - `cofaces_out[d][p]` — cells at dimension d+1 that have `p` as an output face
///
/// `dim = -1` denotes the empty ogposet.  The `normal` flag records whether
/// the cells are in canonical traversal order (see [`normalisation`]).
#[derive(Debug, Clone)]
pub struct Ogposet {
    pub(super) dim: isize,
    pub(super) faces_in:    Vec<Vec<IntSet>>,
    pub(super) faces_out:   Vec<Vec<IntSet>>,
    pub(super) cofaces_in:  Vec<Vec<IntSet>>,
    pub(super) cofaces_out: Vec<Vec<IntSet>>,
    pub(super) normal: bool,
}

impl Ogposet {
    pub(super) fn make(
        dim: isize,
        faces_in:   Vec<Vec<IntSet>>,
        faces_out:  Vec<Vec<IntSet>>,
        cofaces_in: Vec<Vec<IntSet>>,
        cofaces_out: Vec<Vec<IntSet>>,
    ) -> Self {
        Self { dim, faces_in, faces_out, cofaces_in, cofaces_out, normal: false }
    }

    /// The empty ogposet (dim = -1, no cells at any dimension).
    pub fn empty() -> Self {
        Self {
            dim: -1,
            faces_in:   vec![],
            faces_out:  vec![],
            cofaces_in: vec![],
            cofaces_out: vec![],
            normal: true,
        }
    }

    /// A single point (dim = 0, one 0-cell with no faces or cofaces).
    pub fn point() -> Self {
        Self {
            dim: 0,
            faces_in:   vec![vec![vec![]]],
            faces_out:  vec![vec![vec![]]],
            cofaces_in: vec![vec![vec![]]],
            cofaces_out: vec![vec![vec![]]],
            normal: true,
        }
    }

    pub fn is_normal(&self) -> bool { self.normal }

    /// Number of cells at each dimension, as a `Vec` of length `dim+1` (empty for dim < 0).
    pub fn sizes(&self) -> Vec<usize> {
        if self.dim < 0 { return vec![]; }
        (0..=(self.dim as usize)).map(|d| self.faces_in[d].len()).collect()
    }

    pub(super) fn faces_of(&self, sign: Sign, dim: usize, pos: usize) -> IntSet {
        match sign {
            Sign::Input  => self.faces_in[dim][pos].clone(),
            Sign::Output => self.faces_out[dim][pos].clone(),
            Sign::Both   => intset::union(&self.faces_in[dim][pos], &self.faces_out[dim][pos]),
        }
    }

    pub(super) fn cofaces_of(&self, sign: Sign, dim: usize, pos: usize) -> IntSet {
        match sign {
            Sign::Input  => self.cofaces_in[dim][pos].clone(),
            Sign::Output => self.cofaces_out[dim][pos].clone(),
            Sign::Both   => intset::union(&self.cofaces_in[dim][pos], &self.cofaces_out[dim][pos]),
        }
    }

    /// Structural equality: two ogposets are equal iff their face tables coincide.
    pub fn equal(a: &Ogposet, b: &Ogposet) -> bool {
        a.faces_in == b.faces_in && a.faces_out == b.faces_out
    }

    /// Cells at dimension `k` that are extremal in the given direction:
    /// - `Input` extremal: no output cofaces (source boundary of the diagram)
    /// - `Output` extremal: no input cofaces (target boundary)
    pub(super) fn extremal(&self, sign: Sign, k: usize) -> IntSet {
        if self.dim < 0 || k > self.dim as usize {
            return vec![];
        }
        let n = self.faces_in[k].len();
        match sign {
            // A source-boundary cell is one not yet "consumed" by any output coface.
            Sign::Input  => (0..n).filter(|&i| self.cofaces_out[k][i].is_empty()).collect(),
            // A target-boundary cell is one not yet "consumed" by any input coface.
            Sign::Output => (0..n).filter(|&i| self.cofaces_in[k][i].is_empty()).collect(),
            Sign::Both   => (0..n).filter(|&i| {
                self.cofaces_in[k][i].is_empty() || self.cofaces_out[k][i].is_empty()
            }).collect(),
        }
    }

    /// Cells at dimension `k` that have no cofaces in either direction.
    pub(super) fn maximal(&self, k: usize) -> IntSet {
        if self.dim < 0 || k > self.dim as usize {
            return vec![];
        }
        let n = self.faces_in[k].len();
        (0..n).filter(|&i| {
            self.cofaces_in[k][i].is_empty() && self.cofaces_out[k][i].is_empty()
        }).collect()
    }

    /// True if every cell below the top dimension has at least one coface.
    pub(super) fn is_pure(&self) -> bool {
        if self.dim <= 0 { return true; }
        let n = self.dim as usize;
        (0..n).all(|k| self.maximal(k).is_empty())
    }

    /// True if the ogposet is "round": the input and output interiors are disjoint
    /// at every dimension, as required for a well-formed diagram boundary.
    pub fn is_round(&self) -> bool {
        if self.dim <= 0 { return true; }
        if !self.is_pure() { return false; }
        let n = self.dim as usize;
        if self.faces_in[n].len() == 1 { return true; }
        let mut accum_in:  Vec<IntSet> = vec![vec![]; n];
        let mut accum_out: Vec<IntSet> = vec![vec![]; n];

        for j in 0..n {
            let layer_in  = self.build_layer(j, Sign::Input,  &accum_in, &accum_out);
            let layer_out = self.build_layer(j, Sign::Output, &accum_in, &accum_out);

            for i in 0..=j {
                if !intset::is_disjoint(&layer_in[i], &layer_out[i]) {
                    return false;
                }
            }
            for i in 0..=j {
                accum_in[i]  = intset::union(&accum_in[i],  &layer_in[i]);
                accum_out[i] = intset::union(&accum_out[i], &layer_out[i]);
            }
        }
        true
    }

    fn build_layer(
        &self, j: usize, sign: Sign,
        accum_in: &[IntSet], accum_out: &[IntSet],
    ) -> Vec<IntSet> {
        let mut layer: Vec<IntSet> = vec![vec![]; j + 1];
        layer[j] = self.extremal(sign, j);
        for i in (0..j).rev() {
            let upper: Vec<usize> = layer[i + 1].clone();
            layer[i] = intset::collect_sorted(
                upper.iter().flat_map(|&p| self.faces_of(Sign::Both, i + 1, p))
            );
            let prev = intset::union(&accum_in[i], &accum_out[i]);
            layer[i] = intset::difference(&layer[i], &prev);
        }
        layer
    }
}

// ---- Operations ----

/// Remap a face/coface adjacency table onto a subset of cells given by an embedding.
///
/// `forward[d][i]` is the old index of new cell `i` at dimension `d`.
/// `inv_dom[d][old]` maps an old index to its new index, or [`NO_PREIMAGE`].
/// `shift = -1` remaps faces (neighbours at dimension d-1); `shift = +1` remaps
/// cofaces (neighbours at dimension d+1).  With shift -1 every neighbour is
/// guaranteed to have a preimage; with shift +1 neighbours outside the subset
/// are silently dropped.
fn remap_adjacency(
    levels:   usize,
    forward:  &[Vec<usize>],
    inv_dom:  &[Vec<usize>],
    shift:    isize,
    adj:      &[Vec<IntSet>],
) -> Vec<Vec<IntSet>> {
    (0..levels).map(|j| {
        let nj = forward[j].len();
        let boundary = (shift == -1 && j == 0) || (shift == 1 && j + 1 == levels);
        if boundary {
            vec![vec![]; nj]
        } else {
            (0..nj).map(|i| {
                let old = forward[j][i];
                let target_dim = (j as isize + shift) as usize;
                if shift == -1 {
                    set_map(|x| inv_dom[target_dim][x], &adj[j][old])
                } else {
                    set_filter_map(|x| {
                        let y = inv_dom[target_dim][x];
                        (y != NO_PREIMAGE).then_some(y)
                    }, &adj[j][old])
                }
            }).collect()
        }
    }).collect()
}

/// Extract the sub-ogposet on the sign-side boundary of `g` up to dimension `k`.
///
/// Returns the boundary sub-ogposet together with its embedding into `g`.
/// If `k >= g.dim` the result is `g` itself with an identity embedding.
pub(super) fn boundary(sign: Sign, k: usize, g: &Arc<Ogposet>) -> (Arc<Ogposet>, Embedding) {
    if g.dim < 0 {
        return (Arc::new(Ogposet::empty()), Embedding::empty(Arc::clone(g)));
    }
    let gd = g.dim as usize;
    if k >= gd {
        return (Arc::clone(g), Embedding::id(Arc::clone(g)));
    }

    let dims_b = k + 1;
    let sizes_g = g.sizes();
    let mut forward:  Vec<Vec<usize>> = vec![vec![]; dims_b];
    let mut inv_dom:  Vec<Vec<usize>> = (0..dims_b).map(|d| vec![NO_PREIMAGE; sizes_g[d]]).collect();
    let mut next_idx: Vec<usize>      = vec![0; dims_b];

    let insert_f = |j: usize, old: usize,
                        forward: &mut Vec<Vec<usize>>,
                        inv_dom: &mut Vec<Vec<usize>>,
                        next_idx: &mut Vec<usize>| {
        let i = next_idx[j];
        inv_dom[j][old] = i;
        forward[j].push(old);
        next_idx[j] += 1;
    };

    let extremal_k = g.extremal(sign, k);
    for i in extremal_k {
        insert_f(k, i, &mut forward, &mut inv_dom, &mut next_idx);
    }

    for j in (0..k).rev() {
        let parents: Vec<usize> = forward[j + 1].clone();
        for parent_old in parents {
            let faces = g.faces_of(Sign::Both, j + 1, parent_old);
            for f in faces {
                if inv_dom[j][f] == NO_PREIMAGE {
                    insert_f(j, f, &mut forward, &mut inv_dom, &mut next_idx);
                }
            }
        }
        let maximal_j = g.maximal(j);
        for m in maximal_j {
            if inv_dom[j][m] == NO_PREIMAGE {
                insert_f(j, m, &mut forward, &mut inv_dom, &mut next_idx);
            }
        }
    }

    let faces_in   = remap_adjacency(dims_b, &forward, &inv_dom, -1, &g.faces_in);
    let faces_out  = remap_adjacency(dims_b, &forward, &inv_dom, -1, &g.faces_out);
    let cofaces_in  = remap_adjacency(dims_b, &forward, &inv_dom,  1, &g.cofaces_in);
    let cofaces_out = remap_adjacency(dims_b, &forward, &inv_dom,  1, &g.cofaces_out);

    let sub = Arc::new(Ogposet::make(k as isize, faces_in, faces_out, cofaces_in, cofaces_out));

    let full_levels = sizes_g.len();
    let cod_inv: Vec<Vec<usize>> = (0..full_levels).map(|d| {
        if d < dims_b { inv_dom[d].clone() } else { vec![NO_PREIMAGE; sizes_g[d]] }
    }).collect();

    let emb = Embedding::make(Arc::clone(&sub), Arc::clone(g), forward, cod_inv);
    (sub, emb)
}

/// Traverse the sub-ogposet of `g` induced by the downward closure of `initial_stack`,
/// enumerating cells in a canonical input-first order.
///
/// `initial_stack` is a list of `(dim, cell_set)` pairs, typically ordered from
/// highest to lowest dimension, naming the seed cells whose entire downward closure
/// is to be included.  Cells are emitted in the order the traversal marks them:
/// roughly highest dimension first, and within each dimension in the order their
/// input faces were finalised.  This ordering is the key invariant exploited by
/// [`normalisation`] and [`boundary_traverse`].
///
/// Set `mark_normal = true` when the resulting cell ordering is already canonical
/// so that downstream code can skip re-normalising it.
pub(super) fn traverse(g: &Arc<Ogposet>, initial_stack: Vec<(usize, IntSet)>, mark_normal: bool) -> (Arc<Ogposet>, Embedding) {
    if initial_stack.is_empty() {
        return (Arc::new(Ogposet::empty()), Embedding::empty(Arc::clone(g)));
    }
    let gd = if g.dim < 0 {
        return (Arc::new(Ogposet::empty()), Embedding::empty(Arc::clone(g)));
    } else {
        g.dim as usize
    };
    let sizes_g = g.sizes();

    let max_dim = initial_stack.iter().map(|(d, _)| *d).max().unwrap_or(0);

    // Build downward closure as bitvectors
    let mut dc: Vec<BitSet> = (0..=max_dim)
        .map(|d| BitSet::new(sizes_g.get(d).copied().unwrap_or(0)))
        .collect();
    for (d, cells) in &initial_stack {
        for &c in cells { dc[*d].insert(c); }
    }
    for d in (1..=max_dim).rev() {
        if d > gd { continue; }
        let (lower, upper) = dc.split_at_mut(d);
        let dc_d = &upper[0];
        let dc_d_minus_1 = &mut lower[d - 1];
        for cell in dc_d.iter() {
            for &f in &g.faces_in[d][cell]  { dc_d_minus_1.insert(f); }
            for &f in &g.faces_out[d][cell] { dc_d_minus_1.insert(f); }
        }
    }

    let map_levels = max_dim + 1;
    let map_sizes: Vec<usize>  = (0..map_levels).map(|d| dc[d].len()).collect();
    let mut map: Vec<Vec<usize>> = map_sizes.iter().map(|&n| vec![0usize; n]).collect();
    let mut next_idx = vec![0usize; map_levels];
    let mut inv: Vec<Vec<usize>> = sizes_g.iter().map(|&n| vec![NO_PREIMAGE; n]).collect();

    fn do_mark(
        dim: usize, cell: usize,
        map: &mut [Vec<usize>],
        inv: &mut [Vec<usize>],
        next_idx: &mut [usize],
    ) {
        let idx = next_idx[dim];
        map[dim][idx] = cell;
        inv[dim][cell] = idx;
        next_idx[dim] += 1;
    }

    // Convert initial_stack to BitSet stack
    let mut stack: Vec<(usize, BitSet)> = initial_stack.into_iter().map(|(d, cells)| {
        let univ = sizes_g.get(d).copied().unwrap_or(0);
        let mut bs = BitSet::new(univ);
        for &c in &cells { bs.insert(c); }
        (d, bs)
    }).collect();

    // Pre-allocate scratch BitSets to avoid per-iteration allocations
    let mut scratch_in = BitSet::new(0);
    let mut scratch_out = BitSet::new(0);
    let mut scratch_input = BitSet::new(0);
    let mut scratch_outputs = BitSet::new(0);
    let mut scratch_singleton = BitSet::new(0);

    while !stack.is_empty() {
        let dim = stack.last().unwrap().0;

        if stack.last().unwrap().1.is_empty() {
            stack.pop();
            continue;
        }
        if stack.last().unwrap().1.iter().all(|p| inv[dim][p] != NO_PREIMAGE) {
            stack.pop();
            continue;
        }
        if dim == 0 {
            let to_mark: Vec<usize> = stack.last().unwrap().1.iter()
                .filter(|&p| inv[0][p] == NO_PREIMAGE)
                .collect();
            for p in to_mark {
                do_mark(0, p, &mut map, &mut inv, &mut next_idx);
            }
            stack.pop();
            continue;
        }

        let univ_lower = sizes_g.get(dim - 1).copied().unwrap_or(0);

        scratch_in.reset(univ_lower);
        for p in stack.last().unwrap().1.iter() {
            for &f in &g.faces_in[dim][p]  { scratch_in.insert(f); }
        }
        scratch_out.reset(univ_lower);
        for p in stack.last().unwrap().1.iter() {
            for &f in &g.faces_out[dim][p] { scratch_out.insert(f); }
        }

        scratch_input.copy_from(&scratch_in);
        scratch_input.difference_inplace(&scratch_out);

        if scratch_input.iter().any(|p| inv[dim - 1][p] == NO_PREIMAGE) {
            let focus_input = scratch_input.clone();
            stack.push((dim - 1, focus_input));
            continue;
        }

        if stack.last().unwrap().1.len() == 1 {
            let q = stack.last().unwrap().1.iter().next().unwrap();
            do_mark(dim, q, &mut map, &mut inv, &mut next_idx);
            scratch_outputs.reset(univ_lower);
            for &f in &g.faces_out[dim][q] { scratch_outputs.insert(f); }
            if scratch_outputs.iter().any(|p| inv[dim - 1][p] == NO_PREIMAGE) {
                let outputs = scratch_outputs.clone();
                stack.pop();
                stack.push((dim - 1, outputs));
            } else {
                stack.pop();
            }
            continue;
        }

        // Find best candidate: the unmarked cell whose earliest-marked input face
        // has the lowest new index, breaking ties by old cell index.
        let mut best: Option<(usize, usize)> = None;
        {
            let focus = &stack.last().unwrap().1;
            for x in scratch_in.iter() {
                let order = inv[dim - 1][x];
                if order == NO_PREIMAGE { continue; }
                if let Some(q) = g.cofaces_in[dim - 1][x].iter()
                    .copied()
                    .find(|&q| focus.contains(q) && inv[dim][q] == NO_PREIMAGE)
                {
                    let candidate = (order, q);
                    best = Some(match best {
                        None => candidate,
                        Some((bo, bq)) => {
                            if order < bo || (order == bo && q < bq) { candidate }
                            else { (bo, bq) }
                        }
                    });
                }
            }
        }

        if let Some((_, q)) = best {
            let univ = sizes_g.get(dim).copied().unwrap_or(0);
            scratch_singleton.reset(univ);
            scratch_singleton.insert(q);
            let singleton = scratch_singleton.clone();
            stack.push((dim, singleton));
        } else {
            let q_opt = stack.last().unwrap().1.iter()
                .find(|&p| inv[dim][p] == NO_PREIMAGE);
            if let Some(q) = q_opt {
                do_mark(dim, q, &mut map, &mut inv, &mut next_idx);
                stack.last_mut().unwrap().1.remove(q);
            } else {
                stack.pop();
            }
        }
    }

    let faces_in   = remap_adjacency(map_levels, &map, &inv, -1, &g.faces_in);
    let faces_out  = remap_adjacency(map_levels, &map, &inv, -1, &g.faces_out);
    let cofaces_in  = remap_adjacency(map_levels, &map, &inv,  1, &g.cofaces_in);
    let cofaces_out = remap_adjacency(map_levels, &map, &inv,  1, &g.cofaces_out);

    let dom = Arc::new(Ogposet {
        dim: max_dim as isize,
        faces_in, faces_out, cofaces_in, cofaces_out,
        normal: mark_normal,
    });
    let emb = Embedding::make(Arc::clone(&dom), Arc::clone(g), map, inv);
    (dom, emb)
}

/// Compute the normal form of `g`: reorder its cells into the canonical
/// input-first traversal order.  Returns the normalised ogposet and the
/// embedding that maps new indices to old ones.  Memoised by pointer identity.
pub(super) fn normalisation(g: &Arc<Ogposet>) -> (Arc<Ogposet>, Embedding) {
    if g.is_normal() {
        return (Arc::clone(g), Embedding::id(Arc::clone(g)));
    }

    let key = Arc::as_ptr(g) as usize;
    let cached = NORM_CACHE.with(|c| c.borrow().get(&key).cloned());
    if let Some((shape, emb)) = cached {
        return (shape, emb);
    }

    let stack = build_stack_extremal(Sign::Input, g);
    let (dom, emb) = traverse(g, stack, true);

    NORM_CACHE.with(|c| c.borrow_mut().insert(key, (Arc::clone(&dom), emb.clone())));

    (dom, emb)
}

/// Build a traversal stack seeded with the sign-extremal cells of `g` at every
/// dimension, in descending order (highest dimension first).
fn build_stack_extremal(sign: Sign, g: &Ogposet) -> Vec<(usize, IntSet)> {
    if g.dim < 0 { return vec![]; }
    let d = g.dim as usize;
    (0..=d).map(|k| (k, g.extremal(sign, k))).rev().collect()
}

/// Build a traversal stack seeded with sign-extremal cells at levels 0..=`max_dim`,
/// in ascending order.  Used as the initial stack for paste boundary traversal.
fn build_stack_paste(sign: Sign, g: &Ogposet, max_dim: usize) -> Vec<(usize, IntSet)> {
    (0..=max_dim).map(|k| (k, g.extremal(sign, k))).collect()
}

/// Build the traversal stack for the shared boundary of an n-cell:
/// input-extremal cells at levels 0..d-1 plus output-extremal cells at level d-1,
/// reversed so traversal starts from the highest dimension.
fn build_stack_cell_n(g: &Ogposet) -> Vec<(usize, IntSet)> {
    if g.dim < 0 { return vec![]; }
    let d = g.dim as usize;
    let mut inputs: Vec<(usize, IntSet)> =
        (0..d).map(|k| (k, g.extremal(Sign::Input, k))).collect();
    if d > 0 {
        inputs.push((d - 1, g.extremal(Sign::Output, d - 1)));
    }
    inputs.reverse();
    inputs
}

/// Compute the normalised `sign`-boundary of `g` at dimension `k`.  Memoised
/// by (pointer, sign, effective_k).
///
/// - `Input` / `Output`: traverses the sign-extremal cells at every level 0..=k,
///   producing the normalised sign-side boundary sub-ogposet.
/// - `Both`: traverses using [`build_stack_cell_n`], producing the shared boundary
///   needed when forming an n-cell from a pair of parallel (n-1)-diagrams.
pub(super) fn boundary_traverse(sign: Sign, k: usize, g: &Arc<Ogposet>) -> (Arc<Ogposet>, Embedding) {
    let effective_k = if g.dim < 0 { 0 } else { k.min(g.dim as usize) };

    let cache_key = (Arc::as_ptr(g) as usize, sign, effective_k);
    let cached = BT_CACHE.with(|c| c.borrow().get(&cache_key).cloned());
    if let Some((shape, emb)) = cached {
        return (shape, emb);
    }

    let (dom, emb) = match sign {
        Sign::Input | Sign::Output => traverse(g, build_stack_paste(sign, g, effective_k), true),
        Sign::Both => traverse(g, build_stack_cell_n(g), false),
    };

    BT_CACHE.with(|c| c.borrow_mut().insert(cache_key, (Arc::clone(&dom), emb.clone())));

    (dom, emb)
}

/// Find a shape isomorphism from `u` to `v`, or return an error if none exists.
///
/// The algorithm normalises both shapes and checks that their canonical forms
/// are structurally equal.  If so, the isomorphism is recovered by composing
/// the normalisation embedding of `u` with the inverse of `v`'s.
pub(super) fn find_isomorphism(u: &Arc<Ogposet>, v: &Arc<Ogposet>) -> Result<Embedding, Error> {
    let failure = |msg: &str| Err(Error::new(msg));

    if u.dim != v.dim { return failure("dimensions do not match"); }
    let sizes_u = u.sizes();
    let sizes_v = v.sizes();
    if sizes_u != sizes_v { return failure("shapes do not match"); }
    if Ogposet::equal(u, v) { return Ok(Embedding::id(Arc::clone(u))); }

    let (u_norm, e_u) = normalisation(u);
    let (v_norm, e_v) = normalisation(v);
    if !Ogposet::equal(&u_norm, &v_norm) {
        return failure("canonical forms do not match");
    }

    let dims_dom = e_u.inv.len();
    let dims_cod = e_v.inv.len();
    if dims_dom != e_v.map.len() || dims_cod != e_u.map.len() {
        return failure("failed to compose isomorphism data");
    }

    let produce_rows = |inv_levels: &[Vec<usize>], map_levels: &[Vec<usize>]| -> Result<Vec<Vec<usize>>, Error> {
        let dims = inv_levels.len();
        let mut result = vec![vec![]; dims];
        for dim in 0..dims {
            let inv_level = &inv_levels[dim];
            let map_level = &map_levels[dim];
            let len = inv_level.len();
            let mut row = vec![0usize; len];
            for idx in 0..len {
                let mid = inv_level[idx];
                if mid == NO_PREIMAGE || mid >= map_level.len() {
                    return Err(Error::new("failed to compose isomorphism data"));
                }
                row[idx] = map_level[mid];
            }
            result[dim] = row;
        }
        Ok(result)
    };

    let map = produce_rows(&e_u.inv, &e_v.map)?;
    let inv = produce_rows(&e_v.inv, &e_u.map)?;

    Ok(Embedding::make(Arc::clone(u), Arc::clone(v), map, inv))
}

// ---- Thread-local memoisation caches ----
//
// Keyed by the raw pointer of the Arc<Ogposet>.  This is safe because the Arc
// keeps the allocation live for as long as any cache entry that references it.

type ShapeWithEmbedding = (Arc<Ogposet>, Embedding);

thread_local! {
    static NORM_CACHE: RefCell<HashMap<usize, ShapeWithEmbedding>> = RefCell::new(HashMap::new());
    static BT_CACHE: RefCell<HashMap<(usize, Sign, usize), ShapeWithEmbedding>> = RefCell::new(HashMap::new());
}

/// Compute the downward closure of a set of seed cells as BitSets per dimension,
/// without constructing a sub-ogposet or embedding.
///
/// `seeds` is a list of `(dim, cells)` pairs.  The returned `Vec<BitSet>` is
/// indexed by dimension 0..=max_seed_dim; every face of every seed cell is
/// recursively included.  Used by the flow-graph and matching algorithms where
/// only membership queries are needed, not a full sub-ogposet.
#[allow(dead_code)]
pub(super) fn closure(g: &Ogposet, seeds: &[(usize, &[usize])]) -> Vec<BitSet> {
    if g.dim < 0 || seeds.is_empty() {
        return vec![];
    }
    let gd = g.dim as usize;
    let sizes_g = g.sizes();

    let max_dim = seeds.iter().map(|(d, _)| *d).max().unwrap_or(0);

    let mut dc: Vec<BitSet> = (0..=max_dim)
        .map(|d| BitSet::new(sizes_g.get(d).copied().unwrap_or(0)))
        .collect();

    for (d, cells) in seeds {
        for &c in *cells {
            dc[*d].insert(c);
        }
    }

    for d in (1..=max_dim).rev() {
        if d > gd { continue; }
        let (lower, upper) = dc.split_at_mut(d);
        let dc_d = &upper[0];
        let dc_d_minus_1 = &mut lower[d - 1];
        for cell in dc_d.iter() {
            for &f in &g.faces_in[d][cell]  { dc_d_minus_1.insert(f); }
            for &f in &g.faces_out[d][cell] { dc_d_minus_1.insert(f); }
        }
    }

    dc
}

/// Compute Δ^sign_k(x) for a single cell x = (dim, pos): the set of k-dimensional
/// cells in the sign-side k-boundary of the atom cl{x}.
///
/// For the common case `dim == k+1`, this directly reads the face table.
/// For `dim > k+1`, it constructs the atom via `traverse` and calls `extremal`.
pub(super) fn signed_k_boundary_of_cell(
    g: &Arc<Ogposet>,
    sign: Sign,
    k: usize,
    dim: usize,
    pos: usize,
) -> IntSet {
    if dim <= k {
        return vec![];
    }
    if dim == k + 1 {
        return g.faces_of(sign, dim, pos);
    }
    // General case: build the atom and extract the extremal cells at level k.
    let seeds: IntSet = vec![pos];
    let (atom, emb) = traverse(g, vec![(dim, seeds)], false);
    let extremal = atom.extremal(sign, k);
    // Map atom-local indices back to g-indices via the embedding.
    if k < emb.map.len() {
        intset::collect_sorted(extremal.iter().map(|&i| emb.map[k][i]))
    } else {
        vec![]
    }
}

/// Clear all memoisation caches.  Call between independent interpreter runs
/// if long-lived threads are reused.
#[allow(dead_code)]
pub(super) fn clear_caches() {
    NORM_CACHE.with(|c| c.borrow_mut().clear());
    BT_CACHE.with(|c| c.borrow_mut().clear());
}
