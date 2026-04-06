use std::sync::Arc;
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::intset::{self, IntSet};
use super::ogposet::Ogposet;

/// The result of a pushout computation: the colimit ogposet and the two
/// canonical injections into it.
///
/// Given embeddings `f: A → B` and `g: A → C` with common domain, the pushout
/// is an ogposet `tip = B +_A C` together with:
/// - `inl: B → tip` — the left injection
/// - `inr: C → tip` — the right injection
///
/// such that `inl ∘ f = inr ∘ g`.
pub(super) struct Pushout {
    pub(super) tip: Arc<Ogposet>,
    pub(super) inl: Embedding,
    pub(super) inr: Embedding,
}

/// Compute the pushout of `f: A → B` and `g: A → C` along their common domain.
///
/// Routes to [`attach`] with the larger codomain as the base to minimise the
/// number of new cells that need to be allocated.
pub(super) fn pushout(f: &Embedding, g: &Embedding) -> Pushout {
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

/// Core pushout algorithm, assuming `f.cod` is at least as large as `g.cod`.
///
/// Constructs the pushout by starting with `B = f.cod` as the base and merging
/// in the cells of `C = g.cod` one dimension at a time.  For each cell of `C`:
/// - if it has a preimage under `g`, it is identified with the corresponding
///   cell of `B` via `f`;
/// - otherwise it is a new cell and is appended to the tip, with its faces
///   translated into the tip's indexing.
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

    let mut tip_faces_in   = alloc_face_arrays(&b.faces_in,   &total_sizes);
    let mut tip_faces_out  = alloc_face_arrays(&b.faces_out,  &total_sizes);
    let mut tip_cofaces_in  = alloc_face_arrays(&b.cofaces_in,  &total_sizes);
    let mut tip_cofaces_out = alloc_face_arrays(&b.cofaces_out, &total_sizes);

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

                let (fi, fo) = translate_faces(c, i, p, &inr_map);
                tip_faces_in[i][idx]  = fi.clone();
                tip_faces_out[i][idx] = fo.clone();
                inr_inv[i][idx] = p;

                if i > 0 {
                    for &q in &fi { intset::insert(&mut tip_cofaces_in[i - 1][q],  idx); }
                    for &q in &fo { intset::insert(&mut tip_cofaces_out[i - 1][q], idx); }
                }

                counters[i] += 1;
            }
        }
    }

    let tip = Arc::new(Ogposet {
        dim: tip_dim_isize,
        faces_in:   tip_faces_in,
        faces_out:  tip_faces_out,
        cofaces_in:  tip_cofaces_in,
        cofaces_out: tip_cofaces_out,
        normal: false,
    });

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

    let inl = Embedding::make(Arc::clone(b), Arc::clone(&tip), inl_map, inl_inv);
    let inr = Embedding::make(Arc::clone(c), Arc::clone(&tip), inr_map, inr_inv);

    Pushout { tip, inl, inr }
}

/// Allocate a face/coface array with `total_sizes[d]` slots per dimension,
/// pre-filled with the corresponding rows from `base` (B's existing data).
fn alloc_face_arrays(base: &[Vec<IntSet>], total_sizes: &[usize]) -> Vec<Vec<IntSet>> {
    (0..total_sizes.len()).map(|d| {
        let mut arr = vec![vec![]; total_sizes[d]];
        if d < base.len() {
            for (i, s) in base[d].iter().enumerate() {
                arr[i] = s.clone();
            }
        }
        arr
    }).collect()
}

/// Translate the face indices of cell `(i, p)` from `c`'s indexing into the
/// tip's indexing via the partial `inr_map` built so far.  Returns `(faces_in,
/// faces_out)`; both are empty for dimension-0 cells.
fn translate_faces(c: &Ogposet, i: usize, p: usize, inr_map: &[Vec<usize>]) -> (IntSet, IntSet) {
    if i == 0 {
        (vec![], vec![])
    } else {
        let fi = intset::collect_sorted(c.faces_in[i][p].iter().map(|&q| inr_map[i - 1][q]));
        let fo = intset::collect_sorted(c.faces_out[i][p].iter().map(|&q| inr_map[i - 1][q]));
        (fi, fo)
    }
}
