//! Pushout of embeddings between oriented graded posets.
//!
//! The core operation is [`multi_pushout`], which computes the colimit of a base
//! ogposet with any number of extensions attached along shared subobjects.
//! [`pushout`] is a convenience wrapper for the single-extension case.

use std::sync::Arc;
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::intset::{self, IntSet};
use super::ogposet::Ogposet;

/// The result of a binary pushout: the colimit ogposet and the two
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

/// A span for multi-attachment: two embeddings sharing a common domain.
///
/// Given `into_base: A → B` and `into_ext: A → C`, the span identifies the
/// image of A in B with the image of A in C.
pub(super) struct Span<'a> {
    pub(super) into_base: &'a Embedding,
    pub(super) into_ext: &'a Embedding,
}

/// The result of a multi-pushout: the colimit ogposet, the base injection, and
/// one injection per extension.
pub(super) struct MultiPushout {
    pub(super) tip: Arc<Ogposet>,
    pub(super) inl: Embedding,
    pub(super) inrs: Vec<Embedding>,
}

/// Compute the pushout of `f: A → B` and `g: A → C` along their common domain.
///
/// Uses the larger codomain as the base to minimise the number of new cells.
pub(super) fn pushout(f: &Embedding, g: &Embedding) -> Pushout {
    let size_sum = |x: &Ogposet| x.sizes().iter().sum::<usize>();
    let (base_emb, ext_emb, swapped) = if size_sum(&f.cod) >= size_sum(&g.cod) {
        (f, g, false)
    } else {
        (g, f, true)
    };
    let mp = multi_pushout(
        &base_emb.cod,
        &[Span { into_base: base_emb, into_ext: ext_emb }],
    );
    let inr = mp.inrs.into_iter().next().unwrap();
    if swapped {
        Pushout { tip: mp.tip, inl: inr, inr: mp.inl }
    } else {
        Pushout { tip: mp.tip, inl: mp.inl, inr }
    }
}

/// Compute the colimit of a base ogposet with multiple extensions attached
/// along shared subobjects.
///
/// Each [`Span`] contributes an extension `C_i` glued to the base `B` along
/// their common subobject `A_i`.  Returns the tip together with injections
/// `inl: B → tip` and `inr_i: C_i → tip`.
pub(super) fn multi_pushout(base: &Arc<Ogposet>, spans: &[Span]) -> MultiPushout {
    let b_sizes = base.sizes();

    let tip_dim_isize = spans.iter()
        .map(|s| s.into_ext.cod.dim)
        .fold(base.dim, |a, b| a.max(b));
    let tip_dim = if tip_dim_isize < 0 { 0 } else { tip_dim_isize as usize };
    let levels = tip_dim + 1;

    let base_sizes: Vec<usize> = (0..levels).map(|d| {
        b_sizes.get(d).copied().unwrap_or(0)
    }).collect();

    let mut extra_counts: Vec<usize> = vec![0; levels];
    for span in spans {
        let c = &span.into_ext.cod;
        let c_sizes = c.sizes();
        let g_inv = &span.into_ext.inv;
        for d in 0..c.faces_in.len().min(levels) {
            let len = c_sizes.get(d).copied().unwrap_or(0);
            let g_inv_d = g_inv.get(d).map(|v| v.as_slice()).unwrap_or(&[]);
            for p in 0..len {
                if g_inv_d.get(p).copied().unwrap_or(NO_PREIMAGE) == NO_PREIMAGE {
                    extra_counts[d] += 1;
                }
            }
        }
    }

    let total_sizes: Vec<usize> = (0..levels).map(|d| base_sizes[d] + extra_counts[d]).collect();

    let mut tip_faces_in    = alloc_face_arrays(&base.faces_in,    &total_sizes);
    let mut tip_faces_out   = alloc_face_arrays(&base.faces_out,   &total_sizes);
    let mut tip_cofaces_in  = alloc_face_arrays(&base.cofaces_in,  &total_sizes);
    let mut tip_cofaces_out = alloc_face_arrays(&base.cofaces_out, &total_sizes);

    let mut counters = base_sizes.clone();

    let mut all_inr_data: Vec<(Vec<Vec<usize>>, Vec<Vec<usize>>)> =
        Vec::with_capacity(spans.len());

    for span in spans {
        let c = &span.into_ext.cod;
        let c_sizes = c.sizes();
        let c_levels = c.faces_in.len();
        let f_map = &span.into_base.map;
        let g_inv = &span.into_ext.inv;

        let mut inr_map: Vec<Vec<usize>> = (0..c_levels).map(|d| {
            vec![0usize; c_sizes.get(d).copied().unwrap_or(0)]
        }).collect();
        let mut inr_inv: Vec<Vec<usize>> = (0..levels).map(|d| {
            vec![NO_PREIMAGE; total_sizes[d]]
        }).collect();

        for d in 0..c_levels.min(levels) {
            let len = c_sizes.get(d).copied().unwrap_or(0);
            let g_inv_d = g_inv.get(d).map(|v| v.as_slice()).unwrap_or(&[]);

            for p in 0..len {
                let preimage = g_inv_d.get(p).copied().unwrap_or(NO_PREIMAGE);
                if preimage != NO_PREIMAGE {
                    let target_idx = f_map.get(d)
                        .and_then(|row| row.get(preimage))
                        .copied().unwrap_or(0);
                    inr_map[d][p] = target_idx;
                    inr_inv[d][target_idx] = p;
                } else {
                    let idx = counters[d];
                    inr_map[d][p] = idx;
                    inr_inv[d][idx] = p;

                    if d > 0 {
                        let fi = intset::collect_sorted(
                            c.faces_in[d][p].iter().map(|&q| inr_map[d - 1][q]));
                        let fo = intset::collect_sorted(
                            c.faces_out[d][p].iter().map(|&q| inr_map[d - 1][q]));
                        for &q in &fi { intset::insert(&mut tip_cofaces_in[d - 1][q], idx); }
                        for &q in &fo { intset::insert(&mut tip_cofaces_out[d - 1][q], idx); }
                        tip_faces_in[d][idx] = fi;
                        tip_faces_out[d][idx] = fo;
                    }

                    counters[d] += 1;
                }
            }
        }

        all_inr_data.push((inr_map, inr_inv));
    }

    let tip = Arc::new(Ogposet::make(
        tip_dim_isize,
        tip_faces_in,
        tip_faces_out,
        tip_cofaces_in,
        tip_cofaces_out,
    ));

    let b_dim = if base.dim < 0 { 0 } else { base.dim as usize };
    let inl_map: Vec<Vec<usize>> = (0..=b_dim)
        .map(|d| (0..b_sizes.get(d).copied().unwrap_or(0)).collect())
        .collect();
    let inl_inv: Vec<Vec<usize>> = (0..levels).map(|d| {
        let mut arr = vec![NO_PREIMAGE; total_sizes[d]];
        for i in 0..base_sizes[d].min(total_sizes[d]) {
            arr[i] = i;
        }
        arr
    }).collect();
    let inl = Embedding::make(Arc::clone(base), Arc::clone(&tip), inl_map, inl_inv);

    let inrs: Vec<Embedding> = spans.iter().zip(all_inr_data)
        .map(|(span, (inr_map, inr_inv))| {
            Embedding::make(Arc::clone(&span.into_ext.cod), Arc::clone(&tip), inr_map, inr_inv)
        })
        .collect();

    MultiPushout { tip, inl, inrs }
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
