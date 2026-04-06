//! Embeddings between oriented graded posets.
//!
//! An [`Embedding`] is an injective, dimension-preserving map of ogposets: it
//! records both the forward map (`map`) and its partial inverse (`inv`), using
//! [`NO_PREIMAGE`] as a sentinel for cells that have no preimage.

use std::sync::Arc;
use super::ogposet::Ogposet;

/// Sentinel value stored in `inv` when a cell has no preimage under the embedding.
pub const NO_PREIMAGE: usize = usize::MAX;

/// An injective, dimension-preserving map between two oriented graded posets.
///
/// Stores both directions so callers can look up images and preimages in O(1):
/// - `map[d][i]` — image of cell `i` at dimension `d` in the codomain
/// - `inv[d][j]` — preimage of cell `j` at dimension `d`, or [`NO_PREIMAGE`] if none
#[derive(Debug, Clone)]
pub struct Embedding {
    pub dom: Arc<Ogposet>,
    pub cod: Arc<Ogposet>,
    pub map: Vec<Vec<usize>>,
    pub inv: Vec<Vec<usize>>,
}

impl Embedding {
    /// Construct an embedding directly from precomputed `map` and `inv` tables.
    pub fn make(dom: Arc<Ogposet>, cod: Arc<Ogposet>, map: Vec<Vec<usize>>, inv: Vec<Vec<usize>>) -> Self {
        Self { dom, cod, map, inv }
    }

    /// The identity embedding of `x` into itself.
    pub fn id(x: Arc<Ogposet>) -> Self {
        let sizes = x.sizes();
        let map: Vec<Vec<usize>> = sizes.iter().map(|&n| (0..n).collect()).collect();
        let inv = map.clone();
        Self { dom: Arc::clone(&x), cod: x, map, inv }
    }

    /// The unique embedding from the empty ogposet into `cod`.
    pub fn empty(cod: Arc<Ogposet>) -> Self {
        let sizes = cod.sizes();
        let inv: Vec<Vec<usize>> = sizes.iter().map(|&n| vec![NO_PREIMAGE; n]).collect();
        Self { dom: Arc::new(Ogposet::empty()), cod, map: vec![], inv }
    }
}
