use std::sync::Arc;
use super::intset::{self, IntSet};
use super::embeddings::{Embedding, Pushout, NO_PREIMAGE};
use super::ogposet::Ogposet;

/// Pushout of f and g along their common domain.
pub(crate) fn pushout(f: &Embedding, g: &Embedding) -> Pushout {
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

                let new_faces_in: IntSet = if i == 0 {
                    vec![]
                } else {
                    intset::collect_sorted(c.faces_in[i][p].iter().map(|&q| inr_map[i - 1][q]))
                };
                let new_faces_out: IntSet = if i == 0 {
                    vec![]
                } else {
                    intset::collect_sorted(c.faces_out[i][p].iter().map(|&q| inr_map[i - 1][q]))
                };

                tip_faces_in[i][idx]  = new_faces_in.clone();
                tip_faces_out[i][idx] = new_faces_out.clone();
                inr_inv[i][idx] = p;

                if i > 0 {
                    for &q in &new_faces_in  { intset::insert(&mut tip_cofaces_in[i - 1][q],  idx); }
                    for &q in &new_faces_out { intset::insert(&mut tip_cofaces_out[i - 1][q], idx); }
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
