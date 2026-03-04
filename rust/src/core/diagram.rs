use std::sync::Arc;
use crate::aux::{Error, Tag};
use super::ogposet::{self, Ogposet, Sign as OgSign};
pub(crate) use super::ogposet::isomorphism_of;
pub use super::embeddings::{Embedding, Pushout, NO_PREIMAGE};

/// Sign in the diagram sense (no `Both` variant)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Input,
    Output,
}

impl Sign {
    pub fn idx(self) -> usize {
        match self { Self::Input => 0, Self::Output => 1 }
    }
    pub fn as_ogposet_sign(self) -> OgSign {
        match self { Self::Input => OgSign::Input, Self::Output => OgSign::Output }
    }
}

/// The paste-tree records how a diagram was built from paste operations.
/// Each diagram stores one tree per (sign, dim) pair.
#[derive(Debug, Clone)]
pub enum PasteTree {
    Leaf(Tag),
    Node { dim: usize, left: Arc<PasteTree>, right: Arc<PasteTree> },
}

/// Cell data: either a 0-cell (no boundaries) or an n-cell with specified boundaries.
#[derive(Debug, Clone)]
pub enum CellData {
    Zero,
    Boundary { boundary_in: Arc<Diagram>, boundary_out: Arc<Diagram> },
}

/// A diagram: a labelled, oriented graded poset with paste structure.
///
/// `trees[d][sign_idx]` is the paste tree at dimension d for the given sign (0=Input, 1=Output).
#[derive(Debug, Clone)]
pub struct Diagram {
    pub shape: Arc<Ogposet>,
    pub labels: Vec<Vec<Tag>>,       // labels[dim][pos]
    pub trees: Vec<[PasteTree; 2]>,  // trees[dim][sign_idx]
}

impl Diagram {
    pub fn new(shape: Arc<Ogposet>, labels: Vec<Vec<Tag>>, trees: Vec<[PasteTree; 2]>) -> Self {
        Self { shape, labels, trees }
    }

    pub fn dim(&self) -> isize {
        self.shape.dim
    }

    pub fn is_round(&self) -> bool {
        self.shape.is_round()
    }

    pub fn is_normal(&self) -> bool {
        self.shape.is_normal()
    }

    pub fn tree(&self, sign: Sign, dim: usize) -> Option<&PasteTree> {
        self.trees.get(dim).map(|pair| &pair[sign.idx()])
    }

    /// True if the top-level paste tree is just a single leaf (a genuine cell).
    pub fn is_cell(&self) -> bool {
        if self.shape.dim < 0 {
            return false;
        }
        let d = self.shape.dim as usize;
        match self.tree(Sign::Input, d) {
            Some(PasteTree::Leaf(_)) => true,
            _ => false,
        }
    }

    pub fn has_local_labels(&self) -> bool {
        self.labels.iter().any(|level| level.iter().any(|t| t.is_local()))
    }

    pub fn equal(u: &Diagram, v: &Diagram) -> bool {
        Ogposet::equal(&u.shape, &v.shape) && labels_equal(&u.labels, &v.labels)
    }

    pub fn isomorphic(u: &Diagram, v: &Diagram) -> bool {
        if Diagram::equal(u, v) {
            return true;
        }
        match ogposet::isomorphism_of(&u.shape, &v.shape) {
            Err(_) => false,
            Ok(iso) => {
                let pulled = pullback_labels(v, &iso);
                labels_equal(&u.labels, &pulled)
            }
        }
    }

    /// Return the (sign, k)-boundary as a new diagram.
    pub fn boundary(sign: Sign, k: usize, d: &Diagram) -> Result<Diagram, Error> {
        let (_, emb) = ogposet::boundary(sign.as_ogposet_sign(), k, &d.shape);
        let pulled_labels = pullback_labels(d, &emb);
        let new_trees = boundary_trees(&d.trees, sign, k);
        Ok(Diagram::new(Arc::clone(&emb.dom), pulled_labels, new_trees))
    }

    /// Return the normalised (sign, k)-boundary.
    pub fn boundary_normal(sign: Sign, k: usize, d: &Diagram) -> Result<Diagram, Error> {
        let og_sign = sign.as_ogposet_sign();
        let effective_k = if d.shape.dim < 0 { 0 } else { k.min(d.shape.dim as usize) };
        let (shape_norm, emb) = ogposet::boundary_traverse(og_sign, effective_k, &d.shape);
        let pulled_labels = pullback_labels(d, &emb);
        let new_trees = boundary_trees(&d.trees, sign, k);
        Ok(Diagram::new(shape_norm, pulled_labels, new_trees))
    }

    /// Return the normalised version of this diagram (reorder cells canonically).
    pub fn normal(d: &Diagram) -> Diagram {
        if d.is_normal() {
            return d.clone();
        }
        let (shape_norm, emb) = ogposet::normalisation(&d.shape);
        let pulled = pullback_labels(d, &emb);
        Diagram::new(shape_norm, pulled, d.trees.clone())
    }

    /// Check whether u and v have parallel boundaries (same boundary shape and labels).
    pub fn parallelism(u: &Diagram, v: &Diagram) -> Result<(Arc<Ogposet>, Embedding, Embedding), Error> {
        let dim_u = u.shape.dim;
        let dim_v = v.shape.dim;
        if dim_u != dim_v {
            return Err(Error::new("dimensions do not match"));
        }
        if !u.is_round() {
            return Err(Error::new("first argument is not round"));
        }
        if !v.is_round() {
            return Err(Error::new("second argument is not round"));
        }
        let k = if dim_u < 0 { 0 } else { dim_u as usize };
        let (bd_u, e_u) = ogposet::boundary_traverse(OgSign::Both, k, &u.shape);
        let (bd_v, e_v) = ogposet::boundary_traverse(OgSign::Both, k, &v.shape);
        if !Ogposet::equal(&bd_u, &bd_v) {
            return Err(Error::new("shapes of boundaries do not match"));
        }
        let pb_u = pullback_labels(u, &e_u);
        let pb_v = pullback_labels(v, &e_v);
        if !labels_equal(&pb_u, &pb_v) {
            return Err(Error::new("boundaries do not match"));
        }
        Ok((bd_u, e_u, e_v))
    }

    /// Check whether u and v can be pasted at level k.
    pub fn pastability(k: usize, u: &Diagram, v: &Diagram) -> Result<(Arc<Ogposet>, Embedding, Embedding), Error> {
        let dim_u = if u.shape.dim < 0 { 0 } else { u.shape.dim as usize };
        let dim_v = if v.shape.dim < 0 { 0 } else { v.shape.dim as usize };
        let (out_u, e_u) = ogposet::boundary_traverse(OgSign::Output, k.min(dim_u), &u.shape);
        let (in_v, e_v) = ogposet::boundary_traverse(OgSign::Input, k.min(dim_v), &v.shape);
        if !Ogposet::equal(&out_u, &in_v) {
            return Err(Error::new("shapes of boundaries do not match"));
        }
        let pb_u = pullback_labels(u, &e_u);
        let pb_v = pullback_labels(v, &e_v);
        if !labels_equal(&pb_u, &pb_v) {
            return Err(Error::new("boundaries do not match"));
        }
        Ok((out_u, e_u, e_v))
    }

    /// Paste u and v at level k.
    pub fn paste(k: usize, u: &Diagram, v: &Diagram) -> Result<Diagram, Error> {
        let (_, e_u, e_v) = Diagram::pastability(k, u, v)?;
        let Pushout { tip: shape_uv, inl, inr } = super::pushout::pushout(&e_u, &e_v);
        let sizes_uv = shape_uv.sizes();
        let num_dims = sizes_uv.len();

        let mut base_labels: Vec<Vec<Option<Tag>>> = sizes_uv.iter().map(|&n| vec![None; n]).collect();
        for (d, mapping) in inl.map.iter().enumerate() {
            for (idx, &target) in mapping.iter().enumerate() {
                base_labels[d][target] = Some(u.labels[d][idx].clone());
            }
        }
        for (d, mapping) in inr.map.iter().enumerate() {
            for (idx, &target) in mapping.iter().enumerate() {
                base_labels[d][target] = Some(v.labels[d][idx].clone());
            }
        }
        let labels_uv: Vec<Vec<Tag>> = base_labels.into_iter().map(|level| {
            level.into_iter().map(|opt| opt.expect("all cells should be labelled")).collect()
        }).collect();

        let trees_uv = paste_trees(&u.trees, &v.trees, k, num_dims);

        Ok(Diagram::new(shape_uv, labels_uv, trees_uv))
    }

    /// Create a cell from a tag and cell data.
    pub fn cell(tag: Tag, data: &CellData) -> Result<Diagram, Error> {
        match data {
            CellData::Zero => Diagram::cell0(tag),
            CellData::Boundary { boundary_in, boundary_out } => {
                Diagram::cell_n(tag, boundary_in, boundary_out)
            }
        }
    }

    fn cell0(tag: Tag) -> Result<Diagram, Error> {
        let shape = Arc::new(Ogposet::point());
        let labels = vec![vec![tag.clone()]];
        let trees = vec![[PasteTree::Leaf(tag.clone()), PasteTree::Leaf(tag)]];
        Ok(Diagram::new(shape, labels, trees))
    }

    fn cell_n(tag: Tag, u: &Diagram, v: &Diagram) -> Result<Diagram, Error> {
        let (_, e_u, e_v) = Diagram::parallelism(u, v)?;

        let d = if u.shape.dim < 0 { 0 } else { u.shape.dim as usize };
        let Pushout { tip: bd_uv, inl, inr } = super::pushout::pushout(&e_u, &e_v);
        let sizes_bd = bd_uv.sizes();

        let mut faces_in: Vec<Vec<super::intset::IntSet>> = Vec::new();
        let mut faces_out: Vec<Vec<super::intset::IntSet>> = Vec::new();
        let mut cofaces_in: Vec<Vec<super::intset::IntSet>> = Vec::new();
        let mut cofaces_out: Vec<Vec<super::intset::IntSet>> = Vec::new();

        for dim in 0..=(d + 1) {
            if dim <= d {
                let n = sizes_bd.get(dim).copied().unwrap_or(0);
                faces_in.push((0..n).map(|pos| bd_uv.faces_of(OgSign::Input, dim, pos)).collect());
                faces_out.push((0..n).map(|pos| bd_uv.faces_of(OgSign::Output, dim, pos)).collect());

                if dim < d {
                    cofaces_in.push((0..n).map(|pos| bd_uv.cofaces_of(OgSign::Input, dim, pos)).collect());
                    cofaces_out.push((0..n).map(|pos| bd_uv.cofaces_of(OgSign::Output, dim, pos)).collect());
                } else {
                    let inl_inv_d = &inl.inv[d];
                    let inr_inv_d = &inr.inv[d];
                    cofaces_in.push((0..n).map(|idx| {
                        if inl_inv_d.get(idx).copied().unwrap_or(NO_PREIMAGE) != NO_PREIMAGE {
                            vec![0usize]
                        } else {
                            vec![]
                        }
                    }).collect());
                    cofaces_out.push((0..n).map(|idx| {
                        if inr_inv_d.get(idx).copied().unwrap_or(NO_PREIMAGE) != NO_PREIMAGE {
                            vec![0usize]
                        } else {
                            vec![]
                        }
                    }).collect());
                }
            } else {
                let inl_map_d = &inl.map[d];
                let inr_map_d = &inr.map[d];
                let faces_in_set = super::intset::collect_sorted(inl_map_d.iter().copied());
                let faces_out_set = super::intset::collect_sorted(inr_map_d.iter().copied());
                faces_in.push(vec![faces_in_set]);
                faces_out.push(vec![faces_out_set]);
                cofaces_in.push(vec![vec![]]);
                cofaces_out.push(vec![vec![]]);
            }
        }

        let shape_uv = Arc::new(Ogposet::make((d + 1) as isize, faces_in, faces_out, cofaces_in, cofaces_out));

        let mut base_labels: Vec<Vec<Option<Tag>>> = sizes_bd.iter().map(|&n| vec![None; n]).collect();
        for (dim, mapping) in inl.map.iter().enumerate() {
            for (idx, &target) in mapping.iter().enumerate() {
                base_labels[dim][target] = Some(u.labels[dim][idx].clone());
            }
        }
        for (dim, mapping) in inr.map.iter().enumerate() {
            for (idx, &target) in mapping.iter().enumerate() {
                base_labels[dim][target] = Some(v.labels[dim][idx].clone());
            }
        }
        let labels_bd: Vec<Vec<Tag>> = base_labels.into_iter().map(|level| {
            level.into_iter().map(|opt| opt.unwrap()).collect()
        }).collect();
        let mut labels_uv: Vec<Vec<Tag>> = labels_bd;
        labels_uv.push(vec![tag.clone()]);

        let mut trees_uv: Vec<[PasteTree; 2]> = Vec::new();
        for dim in 0..=(d + 1) {
            if dim < d {
                let input_tree = u.trees.get(dim).map(|p| p[0].clone())
                    .unwrap_or(PasteTree::Leaf(tag.clone()));
                let output_tree = u.trees.get(dim).map(|p| p[1].clone())
                    .unwrap_or(PasteTree::Leaf(tag.clone()));
                trees_uv.push([input_tree, output_tree]);
            } else if dim == d {
                let input_tree = u.trees.get(d).map(|p| p[0].clone())
                    .unwrap_or(PasteTree::Leaf(tag.clone()));
                let output_tree = v.trees.get(d).map(|p| p[1].clone())
                    .unwrap_or(PasteTree::Leaf(tag.clone()));
                trees_uv.push([input_tree, output_tree]);
            } else {
                trees_uv.push([PasteTree::Leaf(tag.clone()), PasteTree::Leaf(tag.clone())]);
            }
        }

        Ok(Diagram::new(shape_uv, labels_uv, trees_uv))
    }
}

// ---- Helpers ----

fn labels_equal(a: &[Vec<Tag>], b: &[Vec<Tag>]) -> bool {
    if a.len() != b.len() { return false; }
    a.iter().zip(b.iter()).all(|(ra, rb)| ra.len() == rb.len() && ra.iter().zip(rb.iter()).all(|(x, y)| x == y))
}

fn pullback_labels(d: &Diagram, emb: &Embedding) -> Vec<Vec<Tag>> {
    emb.map.iter().enumerate().map(|(dim, level_map)| {
        level_map.iter().map(|&idx| d.labels[dim][idx].clone()).collect()
    }).collect()
}

/// Trees for a boundary: trees[k'] for k'<k keep original, trees[k][both] = trees[k][sign].
fn boundary_trees(trees: &[[PasteTree; 2]], sign: Sign, k: usize) -> Vec<[PasteTree; 2]> {
    let sign_idx = sign.idx();
    (0..=k).map(|k2| {
        if k2 < k {
            [trees[k2][0].clone(), trees[k2][1].clone()]
        } else {
            let t = trees.get(k).map(|p| p[sign_idx].clone()).unwrap_or(PasteTree::Leaf(Tag::Local("?".into())));
            [t.clone(), t]
        }
    }).collect()
}

/// Trees for a paste: result has max(len_u, len_v) dimensions.
fn paste_trees(u_trees: &[[PasteTree; 2]], v_trees: &[[PasteTree; 2]], n: usize, num_dims: usize) -> Vec<[PasteTree; 2]> {
    let dummy = |idx: usize| u_trees.get(idx).or(v_trees.get(idx))
        .map(|p| p[0].clone())
        .unwrap_or(PasteTree::Leaf(Tag::Local("?".into())));

    (0..num_dims).map(|k| {
        if k < n {
            let it = u_trees.get(k).map(|p| p[0].clone()).unwrap_or_else(|| dummy(k));
            let ot = u_trees.get(k).map(|p| p[1].clone()).unwrap_or_else(|| dummy(k));
            [it, ot]
        } else if k == n {
            let it = u_trees.get(n).map(|p| p[0].clone()).unwrap_or_else(|| dummy(n));
            let ot = v_trees.get(n).map(|p| p[1].clone()).unwrap_or_else(|| dummy(n));
            [it, ot]
        } else {
            let u_in = u_trees.get(k).map(|p| p[0].clone()).unwrap_or_else(|| dummy(k));
            let u_out = u_trees.get(k).map(|p| p[1].clone()).unwrap_or_else(|| dummy(k));
            let v_in = v_trees.get(k).map(|p| p[0].clone()).unwrap_or_else(|| dummy(k));
            let v_out = v_trees.get(k).map(|p| p[1].clone()).unwrap_or_else(|| dummy(k));
            [
                PasteTree::Node { dim: n, left: Arc::new(u_in), right: Arc::new(v_in) },
                PasteTree::Node { dim: n, left: Arc::new(u_out), right: Arc::new(v_out) },
            ]
        }
    }).collect()
}
