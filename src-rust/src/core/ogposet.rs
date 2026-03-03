use crate::helper::error::{Checked, Error};

/// Sorted `Vec<usize>` for long-lived, per-cell face/coface sets.
/// These are always small (bounded by local cell connectivity, typically 1–8 elements)
/// so a contiguous sorted Vec is cheaper than BTreeSet: one allocation, no pointer chasing.
pub type IntSet = Vec<usize>;

// ---- IntSet (sorted Vec<usize>) helpers ----

/// Insert x into a sorted, deduplicated Vec, maintaining the invariant.
#[inline]
pub fn intset_insert(v: &mut IntSet, x: usize) {
    match v.binary_search(&x) {
        Ok(_)  => {}
        Err(i) => v.insert(i, x),
    }
}

/// Merge-union of two sorted Vecs.
pub fn intset_union(a: &IntSet, b: &IntSet) -> IntSet {
    use std::cmp::Ordering::*;
    let mut result = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => { result.push(a[i]); i += 1; }
            Greater => { result.push(b[j]); j += 1; }
            Equal   => { result.push(a[i]); i += 1; j += 1; }
        }
    }
    result.extend_from_slice(&a[i..]);
    result.extend_from_slice(&b[j..]);
    result
}

/// Merge-difference: elements in a that are not in b (both sorted).
pub fn intset_difference(a: &IntSet, b: &IntSet) -> IntSet {
    use std::cmp::Ordering::*;
    let mut result = Vec::with_capacity(a.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => { result.push(a[i]); i += 1; }
            Greater => { j += 1; }
            Equal   => { i += 1; j += 1; }
        }
    }
    result.extend_from_slice(&a[i..]);
    result
}

/// True iff the two sorted Vecs share no element.
pub fn intset_is_disjoint(a: &IntSet, b: &IntSet) -> bool {
    use std::cmp::Ordering::*;
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Less    => i += 1,
            Greater => j += 1,
            Equal   => return false,
        }
    }
    true
}

/// Collect an unsorted iterator into a sorted, deduplicated Vec<usize>.
pub fn collect_sorted(iter: impl Iterator<Item = usize>) -> IntSet {
    let mut v: Vec<usize> = iter.collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn set_map(f: impl Fn(usize) -> usize, s: &IntSet) -> IntSet {
    collect_sorted(s.iter().map(|&x| f(x)))
}

fn set_filter_map(f: impl Fn(usize) -> Option<usize>, s: &IntSet) -> IntSet {
    collect_sorted(s.iter().filter_map(|&x| f(x)))
}

// ---- BitSet: dense bitvector for traversal temporaries ----
//
// For the traversal algorithm, the "downward closure" (dc) and "focus" sets can
// be as large as the total number of cells at a given dimension.  BTreeSet would
// cost O(n) heap allocations; sorted Vec would cost O(n) shifts per insert.
// A bitvector costs O(1) for insert/remove/contains, O(n/64) for iteration/clone,
// and makes difference/union into tight word-level loops — friendly for any complex size.

struct BitSet {
    bits:  Vec<u64>,
    count: usize,
}

impl BitSet {
    fn new(universe: usize) -> Self {
        let words = (universe + 63) / 64;
        BitSet { bits: vec![0u64; words], count: 0 }
    }

    #[inline]
    fn insert(&mut self, x: usize) -> bool {
        let (w, b) = (x / 64, 1u64 << (x % 64));
        if self.bits[w] & b == 0 {
            self.bits[w] |= b;
            self.count += 1;
            true
        } else {
            false
        }
    }

    #[inline]
    fn remove(&mut self, x: usize) -> bool {
        let (w, b) = (x / 64, 1u64 << (x % 64));
        if self.bits[w] & b != 0 {
            self.bits[w] &= !b;
            self.count -= 1;
            true
        } else {
            false
        }
    }

    #[inline]
    fn contains(&self, x: usize) -> bool {
        let w = x / 64;
        w < self.bits.len() && self.bits[w] & (1u64 << (x % 64)) != 0
    }

    fn is_empty(&self) -> bool { self.count == 0 }
    fn len(&self) -> usize { self.count }

    fn iter(&self) -> BitSetIter<'_> {
        BitSetIter {
            bits: &self.bits,
            word_idx: 0,
            word: self.bits.first().copied().unwrap_or(0),
        }
    }

    fn clone(&self) -> Self {
        BitSet { bits: self.bits.clone(), count: self.count }
    }

    /// self &= !other  (in-place set-difference using word-level bitops)
    fn difference_inplace(&mut self, other: &BitSet) {
        let n = self.bits.len().min(other.bits.len());
        for i in 0..n {
            let removed = self.bits[i] & other.bits[i];
            self.bits[i] &= !other.bits[i];
            self.count -= removed.count_ones() as usize;
        }
    }
}

struct BitSetIter<'a> {
    bits:     &'a [u64],
    word_idx: usize,
    word:     u64,
}

impl<'a> Iterator for BitSetIter<'a> {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        while self.word == 0 {
            self.word_idx += 1;
            if self.word_idx >= self.bits.len() { return None; }
            self.word = self.bits[self.word_idx];
        }
        let tz = self.word.trailing_zeros() as usize;
        self.word &= self.word - 1; // clear lowest set bit
        Some(self.word_idx * 64 + tz)
    }
}

// ---- Ogposet ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Input,
    Output,
    Both,
}

/// An oriented graded poset.
///
/// Dimensions are indexed 0..=dim.
/// `faces_in[d][p]` = the set of (d-1)-cells that are input faces of cell p at level d.
/// `cofaces_in[d][p]` = the set of (d+1)-cells that have p as an input face.
/// Analogously for `faces_out` / `cofaces_out`.
#[derive(Debug, Clone)]
pub struct Ogposet {
    /// Top dimension (-1 for the empty ogposet)
    pub dim: isize,
    pub faces_in:   Vec<Vec<IntSet>>,
    pub faces_out:  Vec<Vec<IntSet>>,
    pub cofaces_in: Vec<Vec<IntSet>>,
    pub cofaces_out: Vec<Vec<IntSet>>,
    pub normal: bool,
}

impl Ogposet {
    pub fn make(
        dim: isize,
        faces_in:   Vec<Vec<IntSet>>,
        faces_out:  Vec<Vec<IntSet>>,
        cofaces_in: Vec<Vec<IntSet>>,
        cofaces_out: Vec<Vec<IntSet>>,
    ) -> Self {
        Self { dim, faces_in, faces_out, cofaces_in, cofaces_out, normal: false }
    }

    /// The empty ogposet (dim = -1)
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

    /// A single point (dim = 0)
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

    /// Number of cells at each dimension, as a vector of length dim+1 (or 0 for empty)
    pub fn sizes(&self) -> Vec<usize> {
        if self.dim < 0 { return vec![]; }
        (0..=(self.dim as usize)).map(|d| self.faces_in[d].len()).collect()
    }

    pub fn faces_of(&self, sign: Sign, dim: usize, pos: usize) -> IntSet {
        match sign {
            Sign::Input  => self.faces_in[dim][pos].clone(),
            Sign::Output => self.faces_out[dim][pos].clone(),
            Sign::Both   => intset_union(&self.faces_in[dim][pos], &self.faces_out[dim][pos]),
        }
    }

    pub fn cofaces_of(&self, sign: Sign, dim: usize, pos: usize) -> IntSet {
        match sign {
            Sign::Input  => self.cofaces_in[dim][pos].clone(),
            Sign::Output => self.cofaces_out[dim][pos].clone(),
            Sign::Both   => intset_union(&self.cofaces_in[dim][pos], &self.cofaces_out[dim][pos]),
        }
    }

    pub fn equal(a: &Ogposet, b: &Ogposet) -> bool {
        if a.faces_in.len() != b.faces_in.len() { return false; }
        for (la, lb) in a.faces_in.iter().zip(b.faces_in.iter()) {
            if la.len() != lb.len() { return false; }
            for (sa, sb) in la.iter().zip(lb.iter()) {
                if sa != sb { return false; }
            }
        }
        for (la, lb) in a.faces_out.iter().zip(b.faces_out.iter()) {
            if la.len() != lb.len() { return false; }
            for (sa, sb) in la.iter().zip(lb.iter()) {
                if sa != sb { return false; }
            }
        }
        true
    }

    /// Cells at dimension k that are extremal in the given direction.
    /// Input extremal: no output cofaces. Output extremal: no input cofaces.
    pub fn extremal(&self, sign: Sign, k: usize) -> IntSet {
        if self.dim < 0 || k > self.dim as usize {
            return vec![];
        }
        let n = self.faces_in[k].len();
        // 0..n is already sorted so .collect() yields a sorted Vec.
        match sign {
            Sign::Input  => (0..n).filter(|&i| self.cofaces_out[k][i].is_empty()).collect(),
            Sign::Output => (0..n).filter(|&i| self.cofaces_in[k][i].is_empty()).collect(),
            Sign::Both   => (0..n).filter(|&i| {
                self.cofaces_in[k][i].is_empty() || self.cofaces_out[k][i].is_empty()
            }).collect(),
        }
    }

    /// Cells at dimension k that have no cofaces at all.
    pub fn maximal(&self, k: usize) -> IntSet {
        if self.dim < 0 || k > self.dim as usize {
            return vec![];
        }
        let n = self.faces_in[k].len();
        (0..n).filter(|&i| {
            self.cofaces_in[k][i].is_empty() && self.cofaces_out[k][i].is_empty()
        }).collect()
    }

    /// True if all cells below the top dimension have cofaces.
    pub fn is_pure(&self) -> bool {
        if self.dim <= 0 { return true; }
        let n = self.dim as usize;
        (0..n).all(|k| self.maximal(k).is_empty())
    }

    /// True if pure and has exactly one top-dimensional cell.
    pub fn is_atom(&self) -> bool {
        if self.dim < 0 { return false; }
        self.is_pure() && self.faces_in[self.dim as usize].len() == 1
    }

    /// The ogposet is "round": input and output interiors at each dimension are disjoint.
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
                if !intset_is_disjoint(&layer_in[i], &layer_out[i]) {
                    return false;
                }
            }
            for i in 0..=j {
                accum_in[i]  = intset_union(&accum_in[i],  &layer_in[i]);
                accum_out[i] = intset_union(&accum_out[i], &layer_out[i]);
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
            layer[i] = collect_sorted(
                upper.iter().flat_map(|&p| self.faces_of(Sign::Both, i + 1, p))
            );
            let prev = intset_union(&accum_in[i], &accum_out[i]);
            layer[i] = intset_difference(&layer[i], &prev);
        }
        layer
    }
}

// ---- Embedding ----

/// An embedding (injective map) between two ogposets.
#[derive(Debug, Clone)]
pub struct Embedding {
    pub dom: Ogposet,
    pub cod: Ogposet,
    /// `map[d][i]` = image of cell i at dimension d in the codomain
    pub map: Vec<Vec<usize>>,
    /// `inv[d][j]` = preimage of cell j at dimension d in domain, or usize::MAX if none
    pub inv: Vec<Vec<usize>>,
}

pub const NO_PREIMAGE: usize = usize::MAX;

impl Embedding {
    pub fn make(dom: Ogposet, cod: Ogposet, map: Vec<Vec<usize>>, inv: Vec<Vec<usize>>) -> Self {
        Self { dom, cod, map, inv }
    }

    pub fn id(x: Ogposet) -> Self {
        let sizes = x.sizes();
        let map: Vec<Vec<usize>> = sizes.iter().map(|&n| (0..n).collect()).collect();
        let inv = map.clone();
        Self { dom: x.clone(), cod: x, map, inv }
    }

    pub fn empty(cod: Ogposet) -> Self {
        let sizes = cod.sizes();
        let inv: Vec<Vec<usize>> = sizes.iter().map(|&n| vec![NO_PREIMAGE; n]).collect();
        Self { dom: Ogposet::empty(), cod, map: vec![], inv }
    }

    pub fn compose(f: &Embedding, g: &Embedding) -> Embedding {
        let map: Vec<Vec<usize>> = f.map.iter().enumerate().map(|(d, level)| {
            level.iter().map(|&mid| g.map[d][mid]).collect()
        }).collect();

        let inv: Vec<Vec<usize>> = g.inv.iter().enumerate().map(|(d, g_inv_level)| {
            g_inv_level.iter().map(|&mid| {
                if mid == NO_PREIMAGE { return NO_PREIMAGE; }
                if d >= f.inv.len() { return NO_PREIMAGE; }
                let f_inv_level = &f.inv[d];
                if mid >= f_inv_level.len() { NO_PREIMAGE } else { f_inv_level[mid] }
            }).collect()
        }).collect();

        Embedding::make(f.dom.clone(), g.cod.clone(), map, inv)
    }
}

// ---- Internal helpers ----

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
                        if y == NO_PREIMAGE { None } else { Some(y) }
                    }, &adj[j][old])
                }
            }).collect()
        }
    }).collect()
}

struct EmbedData {
    forward: Vec<Vec<usize>>,
    inv_dom: Vec<Vec<usize>>,
}

/// Compute the boundary (sign-side, up to dimension k) of g.
pub fn boundary(sign: Sign, k: usize, g: &Ogposet) -> (Ogposet, Embedding) {
    if g.dim < 0 {
        return (Ogposet::empty(), Embedding::empty(g.clone()));
    }
    let gd = g.dim as usize;
    if k >= gd {
        return (g.clone(), Embedding::id(g.clone()));
    }

    let dims_b = k + 1;
    let sizes_g = g.sizes();
    let mut acc:      Vec<Vec<usize>> = vec![vec![]; dims_b];
    let mut inv_dom:  Vec<Vec<usize>> = (0..dims_b).map(|d| vec![NO_PREIMAGE; sizes_g[d]]).collect();
    let mut next_idx: Vec<usize>      = vec![0; dims_b];

    let mut insert_f = |j: usize, old: usize,
                        acc: &mut Vec<Vec<usize>>,
                        inv_dom: &mut Vec<Vec<usize>>,
                        next_idx: &mut Vec<usize>| {
        let i = next_idx[j];
        inv_dom[j][old] = i;
        acc[j].push(old);
        next_idx[j] += 1;
    };

    let extremal_k = g.extremal(sign, k);
    for i in extremal_k {
        insert_f(k, i, &mut acc, &mut inv_dom, &mut next_idx);
    }

    for j in (0..k).rev() {
        let parents: Vec<usize> = acc[j + 1].clone();
        for parent_old in parents {
            let faces = g.faces_of(Sign::Both, j + 1, parent_old);
            for f in faces {
                if inv_dom[j][f] == NO_PREIMAGE {
                    insert_f(j, f, &mut acc, &mut inv_dom, &mut next_idx);
                }
            }
        }
        let maximal_j = g.maximal(j);
        for m in maximal_j {
            if inv_dom[j][m] == NO_PREIMAGE {
                insert_f(j, m, &mut acc, &mut inv_dom, &mut next_idx);
            }
        }
    }

    let forward = acc;
    let ed = EmbedData { forward, inv_dom };

    let faces_in   = remap_adjacency(dims_b, &ed.forward, &ed.inv_dom, -1, &g.faces_in);
    let faces_out  = remap_adjacency(dims_b, &ed.forward, &ed.inv_dom, -1, &g.faces_out);
    let cofaces_in  = remap_adjacency(dims_b, &ed.forward, &ed.inv_dom,  1, &g.cofaces_in);
    let cofaces_out = remap_adjacency(dims_b, &ed.forward, &ed.inv_dom,  1, &g.cofaces_out);

    let sub = Ogposet { dim: k as isize, faces_in, faces_out, cofaces_in, cofaces_out, normal: false };

    let full_levels = sizes_g.len();
    let cod_inv: Vec<Vec<usize>> = (0..full_levels).map(|d| {
        if d < dims_b { ed.inv_dom[d].clone() } else { vec![NO_PREIMAGE; sizes_g[d]] }
    }).collect();

    let emb = Embedding::make(sub.clone(), g.clone(), ed.forward, cod_inv);
    (sub, emb)
}

/// Traverse a subset of cells in g (specified by initial_stack: list of (dim, set_of_cells))
/// and return the sub-ogposet induced by the downward closure of those cells.
///
/// Internally uses `BitSet` for downward-closure and focus sets so that insert/contains/remove
/// are all O(1) and set operations (difference, union) are tight word-level bitwise loops.
/// This scales to large complexes without per-element heap allocations.
pub fn traverse(g: &Ogposet, initial_stack: Vec<(usize, IntSet)>) -> (Ogposet, Embedding) {
    if initial_stack.is_empty() {
        return (Ogposet::empty(), Embedding::empty(g.clone()));
    }
    let gd = if g.dim < 0 {
        return (Ogposet::empty(), Embedding::empty(g.clone()));
    } else {
        g.dim as usize
    };
    let sizes_g = g.sizes();

    let max_dim = initial_stack.iter().map(|(d, _)| *d).max().unwrap_or(0);

    // Build downward closure as bitvectors (O(1) insert, O(n/64) iteration).
    let mut dc: Vec<BitSet> = (0..=max_dim)
        .map(|d| BitSet::new(sizes_g.get(d).copied().unwrap_or(0)))
        .collect();
    for (d, cells) in &initial_stack {
        for &c in cells { dc[*d].insert(c); }
    }
    for d in (1..=max_dim).rev() {
        if d > gd { continue; }
        let cells: Vec<usize> = dc[d].iter().collect();
        for cell in cells {
            for &f in &g.faces_in[d][cell]  { dc[d - 1].insert(f); }
            for &f in &g.faces_out[d][cell] { dc[d - 1].insert(f); }
        }
    }

    let map_levels = max_dim + 1;
    let map_sizes: Vec<usize>  = (0..map_levels).map(|d| dc[d].len()).collect();
    let mut map: Vec<Vec<usize>> = map_sizes.iter().map(|&n| vec![0usize; n]).collect();
    let mut next_idx = vec![0usize; map_levels];
    let mut inv: Vec<Vec<usize>> = sizes_g.iter().map(|&n| vec![NO_PREIMAGE; n]).collect();

    fn do_mark(
        dim: usize, cell: usize,
        map: &mut Vec<Vec<usize>>,
        inv: &mut Vec<Vec<usize>>,
        next_idx: &mut Vec<usize>,
    ) {
        let idx = next_idx[dim];
        map[dim][idx] = cell;
        inv[dim][cell] = idx;
        next_idx[dim] += 1;
    }

    // Convert initial_stack (IntSet = Vec<usize>) to BitSet stack.
    let mut stack: Vec<(usize, BitSet)> = initial_stack.into_iter().map(|(d, cells)| {
        let univ = sizes_g.get(d).copied().unwrap_or(0);
        let mut bs = BitSet::new(univ);
        for &c in &cells { bs.insert(c); }
        (d, bs)
    }).collect();

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

        // Build focus_in and focus_out without cloning the focus BitSet.
        let focus_in = {
            let mut bs = BitSet::new(univ_lower);
            for p in stack.last().unwrap().1.iter() {
                for &f in &g.faces_in[dim][p]  { bs.insert(f); }
            }
            bs
        };
        let focus_out = {
            let mut bs = BitSet::new(univ_lower);
            for p in stack.last().unwrap().1.iter() {
                for &f in &g.faces_out[dim][p] { bs.insert(f); }
            }
            bs
        };

        // focus_input = focus_in \ focus_out  (word-level bitwise ops: O(n/64))
        let mut focus_input = focus_in.clone();
        focus_input.difference_inplace(&focus_out);

        if focus_input.iter().any(|p| inv[dim - 1][p] == NO_PREIMAGE) {
            stack.push((dim - 1, focus_input));
            continue;
        }

        if stack.last().unwrap().1.len() == 1 {
            let q = stack.last().unwrap().1.iter().next().unwrap();
            do_mark(dim, q, &mut map, &mut inv, &mut next_idx);
            let mut outputs = BitSet::new(univ_lower);
            for &f in &g.faces_out[dim][q] { outputs.insert(f); }
            if outputs.iter().any(|p| inv[dim - 1][p] == NO_PREIMAGE) {
                stack.pop();
                stack.push((dim - 1, outputs));
            } else {
                stack.pop();
            }
            continue;
        }

        // Find best candidate: the coface of the earliest-marked focus_in cell.
        let mut best: Option<(usize, usize)> = None;
        {
            let focus = &stack.last().unwrap().1;
            for x in focus_in.iter() {
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
        } // immutable borrow of stack released

        if let Some((_, q)) = best {
            let univ = sizes_g.get(dim).copied().unwrap_or(0);
            let mut singleton = BitSet::new(univ);
            singleton.insert(q);
            stack.push((dim, singleton));
        } else {
            // Fallback: find and mark one unmarked element.
            // iter().find() returns Option<usize> (Copy), releasing the borrow immediately.
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

    let ed = EmbedData {
        forward: map.clone(),
        inv_dom: (0..map_levels).map(|d| inv[d].clone()).collect(),
    };

    let faces_in   = remap_adjacency(map_levels, &ed.forward, &ed.inv_dom, -1, &g.faces_in);
    let faces_out  = remap_adjacency(map_levels, &ed.forward, &ed.inv_dom, -1, &g.faces_out);
    let cofaces_in  = remap_adjacency(map_levels, &ed.forward, &ed.inv_dom,  1, &g.cofaces_in);
    let cofaces_out = remap_adjacency(map_levels, &ed.forward, &ed.inv_dom,  1, &g.cofaces_out);

    let dom = Ogposet {
        dim: max_dim as isize,
        faces_in, faces_out, cofaces_in, cofaces_out,
        normal: false,
    };
    let emb = Embedding::make(dom.clone(), g.clone(), map, inv);
    (dom, emb)
}

/// Compute the normal form of g (traverse from input extremals)
pub fn normalisation(g: &Ogposet) -> (Ogposet, Embedding) {
    if g.is_normal() {
        return (g.clone(), Embedding::id(g.clone()));
    }
    let stack = build_stack_extremal(Sign::Input, g);
    let (dom, emb) = traverse(g, stack);
    (Ogposet { normal: true, ..dom }, emb)
}

fn build_stack_extremal(sign: Sign, g: &Ogposet) -> Vec<(usize, IntSet)> {
    if g.dim < 0 { return vec![]; }
    let d = g.dim as usize;
    (0..=d).map(|k| (k, g.extremal(sign, k))).rev().collect()
}

fn build_stack_paste(sign: Sign, g: &Ogposet, max_dim: usize) -> Vec<(usize, IntSet)> {
    (0..=max_dim).map(|k| (k, g.extremal(sign, k))).collect()
}

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

/// Compute boundary traversal: normalised boundary at level k with a given sign.
pub fn boundary_traverse(sign: Sign, k: usize, g: &Ogposet) -> (Ogposet, Embedding) {
    let effective_k = if g.dim < 0 { 0 } else { k.min(g.dim as usize) };
    match sign {
        Sign::Input => {
            let stack = build_stack_paste(Sign::Input, g, effective_k);
            let (dom, emb) = traverse(g, stack);
            (Ogposet { normal: true, ..dom }, emb)
        }
        Sign::Output => {
            let stack = build_stack_paste(Sign::Output, g, effective_k);
            let (dom, emb) = traverse(g, stack);
            (Ogposet { normal: true, ..dom }, emb)
        }
        Sign::Both => {
            let stack = build_stack_cell_n(g);
            traverse(g, stack)
        }
    }
}

/// Try to find an isomorphism from u to v.
pub fn isomorphism_of(u: &Ogposet, v: &Ogposet) -> Checked<Embedding> {
    let failure = |msg: &str| Err(Error::new(msg));

    if u.dim != v.dim { return failure("dimensions do not match"); }
    let sizes_u = u.sizes();
    let sizes_v = v.sizes();
    if sizes_u != sizes_v { return failure("shapes do not match"); }
    if Ogposet::equal(u, v) { return Ok(Embedding::id(u.clone())); }

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

    let produce_rows = |inv_levels: &[Vec<usize>], map_levels: &[Vec<usize>]| -> Checked<Vec<Vec<usize>>> {
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

    Ok(Embedding::make(u.clone(), v.clone(), map, inv))
}

pub fn isomorphic(u: &Ogposet, v: &Ogposet) -> bool {
    if u.dim != v.dim { return false; }
    if u.sizes() != v.sizes() { return false; }
    if Ogposet::equal(u, v) { return true; }
    let (u_norm, _) = normalisation(u);
    let (v_norm, _) = normalisation(v);
    Ogposet::equal(&u_norm, &v_norm)
}

pub struct Pushout {
    pub tip: Ogposet,
    pub inl: Embedding,
    pub inr: Embedding,
}

/// Pushout of f and g along their common domain.
pub fn pushout(f: &Embedding, g: &Embedding) -> Pushout {
    let b = &f.cod;
    let c = &g.cod;
    let size_sum = |x: &Ogposet| x.sizes().iter().sum::<usize>();
    if size_sum(b) >= size_sum(c) {
        attach(f, g)
    } else {
        let res = attach(g, f);
        Pushout { tip: res.tip, inl: res.inr, inr: res.inl }
    }
}

fn attach(f: &Embedding, g: &Embedding) -> Pushout {
    let b = &f.cod;
    let c = &g.cod;
    let f_map = &f.map;
    let g_inv = &g.inv;

    let tip_dim_isize = b.dim.max(c.dim);
    let tip_dim = if tip_dim_isize < 0 { 0 } else { tip_dim_isize as usize };
    let levels = tip_dim + 1;

    let b_sizes = b.sizes();
    let c_sizes = c.sizes();

    let base_sizes: Vec<usize> = (0..levels).map(|d| {
        if d < b_sizes.len() { b_sizes[d] } else { 0 }
    }).collect();

    let mut extra_counts: Vec<usize> = vec![0; levels];
    let c_dim = if c.dim < 0 { 0 } else { c.dim as usize };
    for i in 0..=c_dim.min(c.faces_in.len().saturating_sub(1)) {
        let len = c_sizes.get(i).copied().unwrap_or(0);
        let g_inv_i = g_inv.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
        for p in 0..len {
            if g_inv_i.get(p).copied().unwrap_or(NO_PREIMAGE) == NO_PREIMAGE {
                extra_counts[i] += 1;
            }
        }
    }

    let total_sizes: Vec<usize> = (0..levels).map(|d| base_sizes[d] + extra_counts[d]).collect();

    // Allocate adjacency arrays, seeding from b's data.
    let alloc_faces = |base: &Vec<Vec<IntSet>>| -> Vec<Vec<IntSet>> {
        (0..levels).map(|d| {
            let total = total_sizes[d];
            let mut arr: Vec<IntSet> = vec![vec![]; total];
            if d < base.len() {
                for (i, s) in base[d].iter().enumerate() {
                    arr[i] = s.clone();
                }
            }
            arr
        }).collect()
    };

    let mut tip_faces_in   = alloc_faces(&b.faces_in);
    let mut tip_faces_out  = alloc_faces(&b.faces_out);
    let mut tip_cofaces_in  = alloc_faces(&b.cofaces_in);
    let mut tip_cofaces_out = alloc_faces(&b.cofaces_out);

    let mut inr_inv: Vec<Vec<usize>> = (0..levels).map(|d| vec![NO_PREIMAGE; total_sizes[d]]).collect();
    let c_len = if c.dim < 0 { 0 } else { c.dim as usize + 1 };
    let mut inr_map: Vec<Vec<usize>> = (0..c_len).map(|d| {
        vec![0usize; c_sizes.get(d).copied().unwrap_or(0)]
    }).collect();

    let mut counters: Vec<usize> = base_sizes.clone();

    for i in 0..c_len.min(c.faces_in.len()) {
        let len = c_sizes.get(i).copied().unwrap_or(0);
        let g_inv_i = g_inv.get(i).map(|v| v.as_slice()).unwrap_or(&[]);

        for p in 0..len {
            let preimage = g_inv_i.get(p).copied().unwrap_or(NO_PREIMAGE);
            if preimage != NO_PREIMAGE {
                let target = f_map.get(i).and_then(|row| row.get(preimage)).copied().unwrap_or(0);
                inr_map[i][p] = target;
                if i < inr_inv.len() { inr_inv[i][target] = p; }
            } else {
                let idx = counters[i];
                inr_map[i][p] = idx;

                // Map face sets through the running inr_map (injective so no duplicates, but
                // the indices may not be sorted after remapping, so use collect_sorted).
                let new_faces_in: IntSet = if i == 0 {
                    vec![]
                } else {
                    collect_sorted(c.faces_in[i][p].iter().map(|&q| inr_map[i - 1][q]))
                };
                let new_faces_out: IntSet = if i == 0 {
                    vec![]
                } else {
                    collect_sorted(c.faces_out[i][p].iter().map(|&q| inr_map[i - 1][q]))
                };

                tip_faces_in[i][idx]  = new_faces_in.clone();
                tip_faces_out[i][idx] = new_faces_out.clone();
                inr_inv[i][idx] = p;

                if i > 0 {
                    for &q in &new_faces_in  { intset_insert(&mut tip_cofaces_in[i - 1][q],  idx); }
                    for &q in &new_faces_out { intset_insert(&mut tip_cofaces_out[i - 1][q], idx); }
                }

                counters[i] += 1;
            }
        }
    }

    let tip = Ogposet {
        dim: tip_dim_isize,
        faces_in:   tip_faces_in,
        faces_out:  tip_faces_out,
        cofaces_in:  tip_cofaces_in,
        cofaces_out: tip_cofaces_out,
        normal: false,
    };

    let tip_sizes = tip.sizes();
    let b_dim = if b.dim < 0 { 0 } else { b.dim as usize };
    let inl_map: Vec<Vec<usize>> = (0..=b_dim)
        .map(|d| (0..b_sizes.get(d).copied().unwrap_or(0)).collect())
        .collect();
    let inl_inv: Vec<Vec<usize>> = (0..levels).map(|d| {
        let size_tip = tip_sizes.get(d).copied().unwrap_or(0);
        let mut arr = vec![NO_PREIMAGE; size_tip];
        let size_b = b_sizes.get(d).copied().unwrap_or(0);
        for i in 0..size_b.min(size_tip) { arr[i] = i; }
        arr
    }).collect();

    let inl = Embedding::make(b.clone(), tip.clone(), inl_map, inl_inv);
    let inr = Embedding::make(c.clone(), tip.clone(), inr_map, inr_inv);

    Pushout { tip, inl, inr }
}
