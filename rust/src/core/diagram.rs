use super::embeddings::{Embedding, NO_PREIMAGE, Pushout};
use super::ogposet::{self, Ogposet, Sign as OgSign};
use crate::aux::{Error, Tag};
use std::sync::Arc;

/// Sign in the diagram sense (no `Both` variant)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Source,
    Target,
}

impl Sign {
    fn as_ogposet_sign(self) -> OgSign {
        match self {
            Self::Source => OgSign::Input,
            Self::Target => OgSign::Output,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Dim(usize);

/// Records how a diagram was built up from paste operations.
///
/// - `Leaf(tag)` — a single generating cell, identified by its tag.
/// - `Node { dim, left, right }` — the result of pasting `left` and `right`
///   at dimension `dim`.
#[derive(Debug, Clone)]
pub(super) enum PasteTree {
    Leaf(Tag),
    Node {
        dim: usize,
        left: Arc<PasteTree>,
        right: Arc<PasteTree>,
    },
}

/// Records which generators appear in the source and target boundaries of a
/// diagram at one particular dimension.  One `BoundaryHistory` is stored per
/// dimension in `Diagram::paste_history`.
#[derive(Debug, Clone)]
pub(super) struct BoundaryHistory {
    /// Paste-tree for the source (input) boundary at this dimension.
    pub(super) source: PasteTree,
    /// Paste-tree for the target (output) boundary at this dimension.
    pub(super) target: PasteTree,
}

impl BoundaryHistory {
    fn get(&self, sign: Sign) -> &PasteTree {
        match sign {
            Sign::Source => &self.source,
            Sign::Target => &self.target,
        }
    }

    pub(super) fn from_pair(source: PasteTree, target: PasteTree) -> Self {
        Self { source, target }
    }
}

/// The boundary specification of a cell.
#[derive(Debug, Clone)]
pub enum CellData {
    /// A 0-dimensional cell (a point); has no boundaries.
    Zero,
    /// An n-cell (n > 0) with explicit source and target boundaries.
    Boundary {
        /// The source (input) boundary: an (n−1)-diagram.
        boundary_in: Arc<Diagram>,
        /// The target (output) boundary: an (n−1)-diagram.
        boundary_out: Arc<Diagram>,
    },
}

/// Witness that two diagrams share matching boundaries.
///
/// Produced by `parallelism` and `pastability`; the two embeddings map the
/// shared boundary into each diagram respectively and are used to compute
/// the pushout that merges the pair.
#[derive(Debug, Clone)]
struct BoundaryMatch {
    left_embedding: Embedding,
    right_embedding: Embedding,
}

/// A diagram: a labelled, oriented graded poset with paste structure.
///
/// Representation invariants:
/// - `labels[d][i]` labels the `i`-th cell of `shape` in dimension `d`
/// - `labels[d].len() == shape.sizes()[d]` for every `d`
/// - `paste_history[d]` stores only the two boundary histories (source/target)
///   at that dimension; it is not indexed by cell position
/// - for a genuine generating cell, the top source history is a `Leaf(tag)`
///
/// `paste_history[d]` stores source/target paste history at dimension `d`.
#[derive(Debug, Clone)]
pub struct Diagram {
    pub(super) shape: Arc<Ogposet>,
    pub(super) labels: Vec<Vec<Tag>>,               // labels[dim][pos]
    pub(super) paste_history: Vec<BoundaryHistory>, // paste_history[dim]
}

// ---- Public interface ----

impl Diagram {
    /// Create a cell from a tag and cell data.
    pub fn cell(tag: Tag, data: &CellData) -> Result<Diagram, Error> {
        match data {
            CellData::Zero => Diagram::cell0(tag),
            CellData::Boundary {
                boundary_in,
                boundary_out,
            } => Diagram::cell_n(tag, boundary_in, boundary_out),
        }
    }

    /// Paste u and v at level k.
    pub fn paste(k: usize, u: &Diagram, v: &Diagram) -> Result<Diagram, Error> {
        let m = Diagram::pastability(k, u, v)?;
        let Pushout {
            tip: shape_uv,
            inl,
            inr,
        } = super::pushout::pushout(&m.left_embedding, &m.right_embedding);
        let sizes_uv = shape_uv.sizes();
        let num_dims = sizes_uv.len();

        let labels_uv = merge_pushout_labels(
            &sizes_uv,
            &inl.map,
            &inr.map,
            &u.labels,
            &v.labels,
            "all cells should be labelled",
        );
        let history_uv = paste_histories(&u.paste_history, &v.paste_history, k, num_dims);

        Ok(Diagram::make(shape_uv, labels_uv, history_uv))
    }

    /// Return the (sign, k)-boundary as a new diagram.
    pub fn boundary(sign: Sign, k: usize, d: &Diagram) -> Result<Diagram, Error> {
        let (_, emb) = ogposet::boundary(sign.as_ogposet_sign(), k, &d.shape);
        let pulled_labels = pullback_labels(d, &emb);
        let new_history = boundary_history(&d.paste_history, sign, k);
        Ok(Diagram::make(
            Arc::clone(&emb.dom),
            pulled_labels,
            new_history,
        ))
    }

    /// Return the normalised (sign, k)-boundary.
    pub fn boundary_normal(sign: Sign, k: usize, d: &Diagram) -> Result<Diagram, Error> {
        let og_sign = sign.as_ogposet_sign();
        let effective_k = if d.shape.dim < 0 {
            0
        } else {
            k.min(d.shape.dim as usize)
        };
        let (shape_norm, emb) = ogposet::boundary_traverse(og_sign, effective_k, &d.shape);
        let pulled_labels = pullback_labels(d, &emb);
        let new_history = boundary_history(&d.paste_history, sign, k);
        Ok(Diagram::make(shape_norm, pulled_labels, new_history))
    }

    /// Return the normalised version of this diagram (reorder cells canonically).
    pub fn normal(d: &Diagram) -> Diagram {
        if d.is_normal() {
            return d.clone();
        }
        let (shape_norm, emb) = ogposet::normalisation(&d.shape);
        let pulled = pullback_labels(d, &emb);
        Diagram::make(shape_norm, pulled, d.paste_history.clone())
    }

    /// Returns the labels at dimension `dim`, or `None` if out of range.
    pub fn labels_at(&self, dim: usize) -> Option<&[Tag]> {
        self.labels.get(dim).map(|v| v.as_slice())
    }

    /// Returns the first label at the top dimension, or `None` if absent.
    pub fn top_label(&self) -> Option<&Tag> {
        self.labels_at(self.top_dim()).and_then(|row| row.first())
    }

    /// Iterates over every label in the diagram, across all dimensions.
    pub fn all_labels(&self) -> impl Iterator<Item = &Tag> {
        self.labels.iter().flat_map(|row| row.iter())
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

    fn history(&self, dim: Dim) -> Option<&BoundaryHistory> {
        self.paste_history.get(dim.0)
    }

    pub(super) fn tree(&self, sign: Sign, dim: usize) -> Option<&PasteTree> {
        self.history(Dim(dim)).map(|h| h.get(sign))
    }

    /// Returns the top dimension as a `usize`, clamped to 0 for empty diagrams.
    pub fn top_dim(&self) -> usize {
        self.dim().max(0) as usize
    }

    /// True if the top-level paste tree is just a single leaf (a genuine cell).
    pub fn is_cell(&self) -> bool {
        if self.shape.dim < 0 {
            return false;
        }
        let d = self.shape.dim as usize;
        matches!(self.tree(Sign::Source, d), Some(PasteTree::Leaf(_)))
    }

    pub fn has_local_labels(&self) -> bool {
        self.labels
            .iter()
            .any(|level| level.iter().any(|t| t.is_local()))
    }

    pub fn equal(lhs: &Diagram, rhs: &Diagram) -> bool {
        Ogposet::equal(&lhs.shape, &rhs.shape) && labels_equal(&lhs.labels, &rhs.labels)
    }

    pub fn isomorphic(lhs: &Diagram, rhs: &Diagram) -> bool {
        if Diagram::equal(lhs, rhs) {
            return true;
        }
        match ogposet::isomorphism_of(&lhs.shape, &rhs.shape) {
            Err(_) => false,
            Ok(iso) => {
                let pulled_labels = pullback_labels(rhs, &iso);
                labels_equal(&lhs.labels, &pulled_labels)
            }
        }
    }

    /// Given two diagrams whose top-level shapes are isomorphic, find the image
    /// of `focus` (a label in `source`) in `target` under that shape isomorphism.
    pub fn map_tag_via_shape_iso(source: &Diagram, target: &Diagram, focus: &Tag) -> Result<Tag, Error> {
        let iso = ogposet::isomorphism_of(&source.shape, &target.shape)
            .map_err(|_| Error::new("boundary shapes don't match"))?;
        let dim = source.top_dim();
        let (Some(source_row), Some(map_row), Some(target_row)) = (
            source.labels.get(dim),
            iso.map.get(dim),
            target.labels.get(dim),
        ) else {
            return Err(Error::new("no labels at top dimension"));
        };

        let mut image: Option<Tag> = None;
        for (i, tag) in source_row.iter().enumerate() {
            if tag != focus { continue; }
            let Some(&j) = map_row.get(i) else { continue; };
            let Some(mapped) = target_row.get(j) else { continue; };
            match &image {
                None => image = Some(mapped.clone()),
                Some(existing) if existing != mapped =>
                    return Err(Error::new("generator maps to multiple targets")),
                _ => {}
            }
        }

        image.ok_or_else(|| Error::new("tag not found in source diagram"))
    }
}

// ---- Internal constructors and helpers ----

impl Diagram {
    pub(super) fn make(
        shape: Arc<Ogposet>,
        labels: Vec<Vec<Tag>>,
        paste_history: Vec<BoundaryHistory>,
    ) -> Self {
        debug_assert!(Self::well_formed(&shape, &labels, &paste_history));
        Self { shape, labels, paste_history }
    }

    fn well_formed(
        shape: &Ogposet,
        labels: &[Vec<Tag>],
        paste_history: &[BoundaryHistory],
    ) -> bool {
        let sizes = shape.sizes();
        if labels.len() != sizes.len() || paste_history.len() != sizes.len() {
            return false;
        }
        if !labels
            .iter()
            .zip(sizes.iter())
            .all(|(level_labels, &expected_len)| level_labels.len() == expected_len)
        {
            return false;
        }

        if shape.dim >= 0 {
            let d = shape.dim as usize;
            // A non-empty top-dimensional label list is required for classifier lookup.
            if labels.get(d).is_none_or(|row| row.is_empty()) {
                return false;
            }
        }

        true
    }

    /// Check whether u and v have parallel boundaries (same boundary shape and labels).
    fn parallelism(u: &Diagram, v: &Diagram) -> Result<BoundaryMatch, Error> {
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

        let k = u.top_dim();
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

        Ok(BoundaryMatch {
            left_embedding: e_u,
            right_embedding: e_v,
        })
    }

    /// Check whether u and v can be pasted at level k.
    fn pastability(k: usize, u: &Diagram, v: &Diagram) -> Result<BoundaryMatch, Error> {
        let (out_u, e_u) = ogposet::boundary_traverse(OgSign::Output, k.min(u.top_dim()), &u.shape);
        let (in_v, e_v) = ogposet::boundary_traverse(OgSign::Input, k.min(v.top_dim()), &v.shape);

        if !Ogposet::equal(&out_u, &in_v) {
            return Err(Error::new("shapes of boundaries do not match"));
        }

        let pb_u = pullback_labels(u, &e_u);
        let pb_v = pullback_labels(v, &e_v);
        if !labels_equal(&pb_u, &pb_v) {
            return Err(Error::new("boundaries do not match"));
        }

        Ok(BoundaryMatch {
            left_embedding: e_u,
            right_embedding: e_v,
        })
    }

    fn cell0(tag: Tag) -> Result<Diagram, Error> {
        let shape = Arc::new(Ogposet::point());
        let labels = vec![vec![tag.clone()]];
        let paste_history = vec![BoundaryHistory::from_pair(
            PasteTree::Leaf(tag.clone()),
            PasteTree::Leaf(tag),
        )];
        Ok(Diagram::make(shape, labels, paste_history))
    }

    fn cell_n(tag: Tag, source: &Diagram, target: &Diagram) -> Result<Diagram, Error> {
        let m = Diagram::parallelism(source, target)?;

        let d = source.top_dim();
        let Pushout {
            tip: bd_uv,
            inl,
            inr,
        } = super::pushout::pushout(&m.left_embedding, &m.right_embedding);
        let shape_uv = build_cell_shape(d, &bd_uv, &inl, &inr);

        let sizes_bd = bd_uv.sizes();
        let mut labels_uv = merge_pushout_labels(
            &sizes_bd,
            &inl.map,
            &inr.map,
            &source.labels,
            &target.labels,
            "all boundary cells should be labelled",
        );
        labels_uv.push(vec![tag.clone()]);

        let history_uv =
            build_cell_paste_history(d, &tag, &source.paste_history, &target.paste_history);

        Ok(Diagram::make(shape_uv, labels_uv, history_uv))
    }
}

// ---- Helpers ----

/// Sentinel paste tree used when history data is absent.
fn missing_tree() -> PasteTree {
    PasteTree::Leaf(Tag::Local("?".into()))
}

/// Get a paste tree from a history slice at position `k`, falling back to `fallback()`.
fn history_tree(hist: &[BoundaryHistory], sign: Sign, k: usize, fallback: impl FnOnce() -> PasteTree) -> PasteTree {
    hist.get(k).map(|h| h.get(sign).clone()).unwrap_or_else(fallback)
}

fn labels_equal(a: &[Vec<Tag>], b: &[Vec<Tag>]) -> bool {
    a == b
}

fn pullback_labels(d: &Diagram, emb: &Embedding) -> Vec<Vec<Tag>> {
    emb.map
        .iter()
        .enumerate()
        .map(|(dim, level_map)| {
            level_map
                .iter()
                .map(|&idx| d.labels[dim][idx].clone())
                .collect()
        })
        .collect()
}

fn merge_pushout_labels(
    sizes: &[usize],
    inl_map: &[Vec<usize>],
    inr_map: &[Vec<usize>],
    left_labels: &[Vec<Tag>],
    right_labels: &[Vec<Tag>],
    missing_label_msg: &str,
) -> Vec<Vec<Tag>> {
    let mut base_labels: Vec<Vec<Option<Tag>>> = sizes.iter().map(|&n| vec![None; n]).collect();

    for (dim, mapping) in inl_map.iter().enumerate() {
        for (idx, &target) in mapping.iter().enumerate() {
            base_labels[dim][target] = Some(left_labels[dim][idx].clone());
        }
    }

    for (dim, mapping) in inr_map.iter().enumerate() {
        for (idx, &target) in mapping.iter().enumerate() {
            base_labels[dim][target] = Some(right_labels[dim][idx].clone());
        }
    }

    base_labels
        .into_iter()
        .map(|level| {
            level
                .into_iter()
                .map(|opt| opt.expect(missing_label_msg))
                .collect()
        })
        .collect()
}

/// Histories for a boundary: histories[k'] for k'<k keep original,
/// histories[k][both] = histories[k][sign].
fn boundary_history(histories: &[BoundaryHistory], sign: Sign, k: usize) -> Vec<BoundaryHistory> {
    (0..=k)
        .map(|k2| {
            if k2 < k {
                histories[k2].clone()
            } else {
                let t = histories
                    .get(k)
                    .map(|h| h.get(sign).clone())
                    .unwrap_or_else(missing_tree);
                BoundaryHistory::from_pair(t.clone(), t)
            }
        })
        .collect()
}

/// Histories for a paste: result has `num_dims` dimensions.
fn paste_histories(
    u_hist: &[BoundaryHistory],
    v_hist: &[BoundaryHistory],
    n: usize,
    num_dims: usize,
) -> Vec<BoundaryHistory> {
    let dummy = |k: usize| {
        u_hist
            .get(k)
            .or(v_hist.get(k))
            .map(|h| h.source.clone())
            .unwrap_or_else(missing_tree)
    };

    (0..num_dims)
        .map(|k| {
            if k < n {
                BoundaryHistory::from_pair(
                    history_tree(u_hist, Sign::Source, k, || dummy(k)),
                    history_tree(u_hist, Sign::Target, k, || dummy(k)),
                )
            } else if k == n {
                BoundaryHistory::from_pair(
                    history_tree(u_hist, Sign::Source, n, || dummy(n)),
                    history_tree(v_hist, Sign::Target, n, || dummy(n)),
                )
            } else {
                BoundaryHistory::from_pair(
                    PasteTree::Node {
                        dim: n,
                        left: Arc::new(history_tree(u_hist, Sign::Source, k, || dummy(k))),
                        right: Arc::new(history_tree(v_hist, Sign::Source, k, || dummy(k))),
                    },
                    PasteTree::Node {
                        dim: n,
                        left: Arc::new(history_tree(u_hist, Sign::Target, k, || dummy(k))),
                        right: Arc::new(history_tree(v_hist, Sign::Target, k, || dummy(k))),
                    },
                )
            }
        })
        .collect()
}

fn build_cell_shape(
    d: usize,
    bd_uv: &Arc<Ogposet>,
    inl: &Embedding,
    inr: &Embedding,
) -> Arc<Ogposet> {
    let sizes_bd = bd_uv.sizes();

    let mut faces_in: Vec<Vec<super::intset::IntSet>> = Vec::new();
    let mut faces_out: Vec<Vec<super::intset::IntSet>> = Vec::new();
    let mut cofaces_in: Vec<Vec<super::intset::IntSet>> = Vec::new();
    let mut cofaces_out: Vec<Vec<super::intset::IntSet>> = Vec::new();

    // Dims 0..d-1: interior boundary cells — copy faces and cofaces directly from bd_uv.
    for dim in 0..d {
        let n = sizes_bd.get(dim).copied().unwrap_or(0);
        faces_in.push((0..n).map(|pos| bd_uv.faces_of(OgSign::Input, dim, pos)).collect());
        faces_out.push((0..n).map(|pos| bd_uv.faces_of(OgSign::Output, dim, pos)).collect());
        cofaces_in.push((0..n).map(|pos| bd_uv.cofaces_of(OgSign::Input, dim, pos)).collect());
        cofaces_out.push((0..n).map(|pos| bd_uv.cofaces_of(OgSign::Output, dim, pos)).collect());
    }

    // Dim d: top boundary cells — copy faces from bd_uv; cofaces point to the new
    // top cell (index 0 at dim d+1) iff the cell appears in the source (inl) or
    // target (inr) embedding respectively.
    {
        let n = sizes_bd.get(d).copied().unwrap_or(0);
        faces_in.push((0..n).map(|pos| bd_uv.faces_of(OgSign::Input, d, pos)).collect());
        faces_out.push((0..n).map(|pos| bd_uv.faces_of(OgSign::Output, d, pos)).collect());
        let inl_inv_d = &inl.inv[d];
        let inr_inv_d = &inr.inv[d];
        cofaces_in.push(
            (0..n)
                .map(|idx| {
                    if inl_inv_d.get(idx).copied().unwrap_or(NO_PREIMAGE) != NO_PREIMAGE {
                        vec![0usize]
                    } else {
                        vec![]
                    }
                })
                .collect(),
        );
        cofaces_out.push(
            (0..n)
                .map(|idx| {
                    if inr_inv_d.get(idx).copied().unwrap_or(NO_PREIMAGE) != NO_PREIMAGE {
                        vec![0usize]
                    } else {
                        vec![]
                    }
                })
                .collect(),
        );
    }

    // Dim d+1: the single new top cell — its source face is the inl image and
    // its target face is the inr image; it has no cofaces.
    {
        let faces_source = super::intset::collect_sorted(inl.map[d].iter().copied());
        let faces_target = super::intset::collect_sorted(inr.map[d].iter().copied());
        faces_in.push(vec![faces_source]);
        faces_out.push(vec![faces_target]);
        cofaces_in.push(vec![vec![]]);
        cofaces_out.push(vec![vec![]]);
    }

    Arc::new(Ogposet::make(
        (d + 1) as isize,
        faces_in,
        faces_out,
        cofaces_in,
        cofaces_out,
    ))
}

fn build_cell_paste_history(
    d: usize,
    tag: &Tag,
    source: &[BoundaryHistory],
    target: &[BoundaryHistory],
) -> Vec<BoundaryHistory> {
    (0..=(d + 1))
        .map(|dim| {
            if dim < d {
                BoundaryHistory::from_pair(
                    history_tree(source, Sign::Source, dim, || PasteTree::Leaf(tag.clone())),
                    history_tree(source, Sign::Target, dim, || PasteTree::Leaf(tag.clone())),
                )
            } else if dim == d {
                BoundaryHistory::from_pair(
                    history_tree(source, Sign::Source, d, || PasteTree::Leaf(tag.clone())),
                    history_tree(target, Sign::Target, d, || PasteTree::Leaf(tag.clone())),
                )
            } else {
                BoundaryHistory::from_pair(
                    PasteTree::Leaf(tag.clone()),
                    PasteTree::Leaf(tag.clone()),
                )
            }
        })
        .collect()
}
