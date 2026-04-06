use std::sync::Arc;
use super::ogposet::Ogposet;

pub const NO_PREIMAGE: usize = usize::MAX;
/// An embedding (injective map) between two ogposets.
#[derive(Debug, Clone)]
pub struct Embedding {
    pub dom: Arc<Ogposet>,
    pub cod: Arc<Ogposet>,
    /// `map[d][i]` = image of cell i at dimension d in the codomain
    pub map: Vec<Vec<usize>>,
    /// `inv[d][j]` = preimage of cell j at dimension d in domain, or usize::MAX if none
    pub inv: Vec<Vec<usize>>,
}

impl Embedding {
    pub fn make(dom: Arc<Ogposet>, cod: Arc<Ogposet>, map: Vec<Vec<usize>>, inv: Vec<Vec<usize>>) -> Self {
        Self { dom, cod, map, inv }
    }

    pub fn id(x: Arc<Ogposet>) -> Self {
        let sizes = x.sizes();
        let map: Vec<Vec<usize>> = sizes.iter().map(|&n| (0..n).collect()).collect();
        let inv = map.clone();
        Self { dom: Arc::clone(&x), cod: x, map, inv }
    }

    pub fn empty(cod: Arc<Ogposet>) -> Self {
        let sizes = cod.sizes();
        let inv: Vec<Vec<usize>> = sizes.iter().map(|&n| vec![NO_PREIMAGE; n]).collect();
        Self { dom: Arc::new(Ogposet::empty()), cod, map: vec![], inv }
    }
}

