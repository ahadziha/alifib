//! Diagrams: labelled oriented graded posets with paste structure.
//!
//! A [`Diagram`] pairs an [`Ogposet`] shape with a label at each cell and a
//! [`PasteTree`] history recording how it was assembled from generators.  The
//! two central operations are [`Diagram::cell`] (introduce a generating cell)
//! and [`Diagram::paste`] (compose two diagrams along matching boundaries).

use super::embeddings::{Embedding, NO_PREIMAGE};
use super::pushout::Pushout;
use super::ogposet::{self, Ogposet, Sign as OgSign};
use crate::aux::{Error, Tag};
use std::sync::Arc;

/// Source/target polarity for diagram boundaries.
///
/// Unlike [`OgSign`], which also has a `Both` variant used by traversal
/// queries, diagram operations always act on exactly one boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

/// Records how a diagram was built up from paste operations.
///
/// - `Leaf(tag)` — a single generating cell, identified by its tag.
/// - `Node { dim, left, right }` — the result of pasting `left` and `right`
///   at dimension `dim`.
#[derive(Debug, Clone)]
pub(crate) enum PasteTree {
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
pub(crate) struct BoundaryHistory {
    /// Paste-tree for the source (input) boundary at this dimension.
    pub(crate) source: PasteTree,
    /// Paste-tree for the target (output) boundary at this dimension.
    pub(crate) target: PasteTree,
}

impl BoundaryHistory {
    fn get(&self, sign: Sign) -> &PasteTree {
        match sign {
            Sign::Source => &self.source,
            Sign::Target => &self.target,
        }
    }

    pub(crate) fn from_pair(source: PasteTree, target: PasteTree) -> Self {
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
    pub(crate) labels: Vec<Vec<Tag>>,               // labels[dim][pos]
    pub(crate) paste_history: Vec<BoundaryHistory>, // paste_history[dim]
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
        let effective_k = if d.shape.dim < 0 {
            0
        } else {
            k.min(d.shape.dim as usize)
        };
        let (_, emb) = ogposet::boundary(sign.as_ogposet_sign(), effective_k, &d.shape);
        let pulled_labels = pullback_labels(d, &emb);
        // Clamp history to the same effective boundary dimension as the shape.
        // If we keep the caller's larger `k`, we manufacture extra history levels and
        // break the core invariant that shape/labels/history all have matching lengths.
        let new_history = boundary_history(&d.paste_history, sign, effective_k);
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
        // `boundary_traverse` clamps to `effective_k`; history must do the same or the
        // returned diagram is malformed in release builds when callers pass k > top_dim.
        let new_history = boundary_history(&d.paste_history, sign, effective_k);
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

    /// The ogposet dimension of the underlying shape; negative means the empty diagram.
    pub fn dim(&self) -> isize {
        self.shape.dim
    }

    /// True if the diagram's source and target boundaries are equal (prerequisite for pasting).
    pub fn is_round(&self) -> bool {
        self.shape.is_round()
    }

    /// True if the diagram's underlying shape is in canonical (normal) form.
    pub fn is_normal(&self) -> bool {
        self.shape.is_normal()
    }

    pub(crate) fn tree(&self, sign: Sign, dim: usize) -> Option<&PasteTree> {
        self.paste_history.get(dim).map(|h| h.get(sign))
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

    /// True if any cell in the diagram carries a local (non-global) tag.
    pub fn has_local_labels(&self) -> bool {
        self.labels
            .iter()
            .any(|level| level.iter().any(|t| t.is_local()))
    }

    /// Structural equality: same shape and identical labels at every cell position.
    pub fn equal(lhs: &Diagram, rhs: &Diagram) -> bool {
        Ogposet::equal(&lhs.shape, &rhs.shape) && labels_equal(&lhs.labels, &rhs.labels)
    }

    /// Equality up to shape isomorphism: same labels after relabelling by the canonical
    /// shape isomorphism (falls back to [`Diagram::equal`] first for efficiency).
    pub fn isomorphic(lhs: &Diagram, rhs: &Diagram) -> bool {
        if Diagram::equal(lhs, rhs) {
            return true;
        }
        match ogposet::find_isomorphism(&lhs.shape, &rhs.shape) {
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
        let iso = ogposet::find_isomorphism(&source.shape, &target.shape)
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
    /// Construct a diagram directly from precomputed components.
    ///
    /// Panics in debug builds if the components violate the well-formedness
    /// invariants (label counts must match shape sizes at every dimension).
    pub(super) fn make(
        shape: Arc<Ogposet>,
        labels: Vec<Vec<Tag>>,
        paste_history: Vec<BoundaryHistory>,
    ) -> Self {
        debug_assert!(Self::well_formed(&shape, &labels, &paste_history));
        Self { shape, labels, paste_history }
    }

    /// Return `true` if `shape`, `labels`, and `paste_history` satisfy all
    /// representation invariants (used only in `debug_assert`).
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

    /// Construct a 0-dimensional cell diagram (a point labelled `tag`).
    fn cell0(tag: Tag) -> Result<Diagram, Error> {
        let shape = Arc::new(Ogposet::point());
        let labels = vec![vec![tag.clone()]];
        let paste_history = vec![BoundaryHistory::from_pair(
            PasteTree::Leaf(tag.clone()),
            PasteTree::Leaf(tag),
        )];
        Ok(Diagram::make(shape, labels, paste_history))
    }

    /// Construct an n-dimensional cell with the given tag and parallel boundary diagrams.
    fn cell_n(tag: Tag, source: &Diagram, target: &Diagram) -> Result<Diagram, Error> {
        let (diagram, _) = Diagram::cell_with_source_embedding(tag, source, target)?;
        Ok(diagram)
    }

    /// Like `cell_n` but also returns the embedding of the source boundary into the new cell.
    ///
    /// The embedding maps `source.shape` into the cell's shape, identifying source cells
    /// with their positions in the merged boundary ogposet (dims 0..=n) of the cell.
    pub(super) fn cell_with_source_embedding(
        tag: Tag,
        source: &Diagram,
        target: &Diagram,
    ) -> Result<(Diagram, Embedding), Error> {
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

        let diagram = Diagram::make(Arc::clone(&shape_uv), labels_uv, history_uv);

        // The source embedding: source.shape → shape_uv.
        // `inl` maps source.shape → bd_uv. In shape_uv, dims 0..=d are exactly bd_uv's
        // cells (same indices). Extend inl's inverse to cover dim d+1 (the new top cell,
        // which has no preimage in the source).
        let source_map = inl.map.clone();
        let mut source_inv = inl.inv.clone();
        source_inv.push(vec![NO_PREIMAGE]); // dim d+1: one cell, no preimage from source
        let source_emb = Embedding::make(
            Arc::clone(&source.shape),
            Arc::clone(&shape_uv),
            source_map,
            source_inv,
        );

        Ok((diagram, source_emb))
    }

    /// Construct the (n+1)-dimensional whiskered rewrite step S = U ∪_V R.
    ///
    /// Given:
    /// - `current` (U): the n-dimensional current diagram
    /// - `match_emb` (ι): embedding V.shape → U.shape from subdiagram matching
    /// - `rule_tag`: the tag of the (n+1)-generator being applied
    /// - `source` (V): the rule's source boundary (the matched pattern)
    /// - `target` (T): the rule's target boundary (the replacement)
    ///
    /// Returns an (n+1)-dimensional diagram S with:
    /// - Source n-boundary = U
    /// - Target n-boundary = U[V → T]
    /// - One interior (n+1)-cell (the whiskered rule application)
    ///
    /// Works uniformly for all n ≥ 0.
    pub fn whisker_rewrite(
        current: &Diagram,
        match_emb: &Embedding,
        rule_tag: &Tag,
        source: &Diagram,
        target: &Diagram,
    ) -> Result<Diagram, Error> {
        // Build rule cell R and get the source-boundary embedding σ: V → R.
        let (rule_cell, source_into_rule) =
            Diagram::cell_with_source_embedding(rule_tag.clone(), source, target)?;

        // Pushout: S = U ∪_V R.
        // match_emb: V → U   and   source_into_rule: V → R share dom = V.shape.
        let Pushout { tip, inl, inr } =
            super::pushout::pushout(match_emb, &source_into_rule);

        // Merge labels from U (dims 0..n) and R (dims 0..n+1).
        let result_labels = merge_pushout_labels(
            &tip.sizes(),
            &inl.map,
            &inr.map,
            &current.labels,
            &rule_cell.labels,
            "whisker rewrite: all cells should be labelled",
        );

        // Paste history: inherit from current for dims 0..n-1; fix dim n target tree;
        // add a properly whiskered tree at dim n+1.
        let n = current.top_dim();
        let mut history = current.paste_history.clone();
        if history.len() <= n {
            // Pad in case paste_history is shorter than expected (e.g., for 0-dim diagrams).
            history.resize_with(n + 1, || BoundaryHistory::from_pair(
                missing_tree(), missing_tree(),
            ));
        }

        // The matched top-dim positions in `current`.  Sort a local copy — the embedding map
        // order is determined by the matching algorithm and is not guaranteed to be ascending.
        let mut matched_buf: Vec<usize> = match_emb.map.get(n).cloned().unwrap_or_default();
        matched_buf.sort_unstable();
        let matched: &[usize] = &matched_buf;

        // Build the whiskered trees:
        //   - dim n+1 source tree: idle cells + rule_tag + idle cells (= the whiskered rule)
        //   - dim n+1 target tree: idle cells + target labels + idle cells
        //   - dim n target tree:   same as dim n+1 target (= the rewritten n-boundary)
        //
        // Paste dimension for folding the pieces: n-1 (or 0 if n == 0, but for n=0 there
        // is at most one piece so the fold dimension is never actually used in a Node).
        let paste_dim = n.saturating_sub(1);
        let num_top = current.labels.get(n).map(Vec::len).unwrap_or(0);
        let tgt_top = target.labels.get(target.top_dim()).map(Vec::as_slice).unwrap_or(&[]);

        let mut src_pieces: Vec<PasteTree> = Vec::new();
        let mut tgt_pieces: Vec<PasteTree> = Vec::new();
        let mut i = 0;
        let mut tgt_i = 0;
        while i < num_top {
            if matched.binary_search(&i).is_ok() {
                // This is the start of the matched segment; the rule_tag covers it all.
                src_pieces.push(PasteTree::Leaf(rule_tag.clone()));
                // The target boundary's labels expand in place of the matched segment.
                while tgt_i < tgt_top.len() {
                    tgt_pieces.push(PasteTree::Leaf(tgt_top[tgt_i].clone()));
                    tgt_i += 1;
                }
                // Skip all matched positions.
                while i < num_top && matched.binary_search(&i).is_ok() {
                    i += 1;
                }
            } else {
                let idle = PasteTree::Leaf(current.labels[n][i].clone());
                src_pieces.push(idle.clone());
                tgt_pieces.push(idle);
                i += 1;
            }
        }

        let src_tree = fold_trees(src_pieces, paste_dim);
        let tgt_tree = fold_trees(tgt_pieces, paste_dim);

        // Fix the dim-n history: source n-boundary is unchanged (= current); target
        // n-boundary is the rewritten diagram, described by tgt_tree.
        history[n] = BoundaryHistory::from_pair(
            history[n].source.clone(),
            tgt_tree.clone(),
        );
        // Dim n+1: both source and target describe the whiskered (n+1)-cell.
        history.push(BoundaryHistory::from_pair(src_tree, tgt_tree));

        Ok(Diagram::make(tip, result_labels, history))
    }
}

// ---- Helpers ----

/// Sentinel paste tree used when history data is absent.
fn missing_tree() -> PasteTree {
    PasteTree::Leaf(Tag::Local("?".into()))
}

/// Fold a list of paste trees into one by left-associative `Node{dim, ...}` nesting.
/// Returns the single element unchanged if `pieces` has length 1, and `missing_tree()`
/// if `pieces` is empty.
fn fold_trees(pieces: Vec<PasteTree>, dim: usize) -> PasteTree {
    let mut iter = pieces.into_iter();
    let Some(first) = iter.next() else { return missing_tree(); };
    iter.fold(first, |acc, p| PasteTree::Node {
        dim,
        left: Arc::new(acc),
        right: Arc::new(p),
    })
}

/// Get a paste tree from a history slice at position `k`, falling back to `fallback()`.
fn history_tree(hist: &[BoundaryHistory], sign: Sign, k: usize, fallback: impl FnOnce() -> PasteTree) -> PasteTree {
    hist.get(k).map(|h| h.get(sign).clone()).unwrap_or_else(fallback)
}

fn labels_equal(a: &[Vec<Tag>], b: &[Vec<Tag>]) -> bool {
    a == b
}

/// Pull back the labels of `d` along `emb`: for each cell `i` in `emb.dom`,
/// the result carries the label of the image `emb.map[dim][i]` in `d`.
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

/// Merge the labels of `left` and `right` into a flat array indexed by the
/// pushout's cell positions, using `inl_map` and `inr_map` to route each source
/// cell to its position in the pushout.
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

/// Compute the paste history for the `(sign, k)`-boundary of a diagram.
///
/// Dimensions below `k` are copied unchanged; at dimension `k` both the source
/// and target history are set to `histories[k][sign]` (collapsing the boundary).
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

/// Compute the paste history when pasting `u` and `v` at dimension `n`.
/// The result has `num_dims` dimensions; each dimension delegates to [`paste_tree`].
fn paste_histories(
    u_hist: &[BoundaryHistory],
    v_hist: &[BoundaryHistory],
    n: usize,
    num_dims: usize,
) -> Vec<BoundaryHistory> {
    let dummy = |k: usize| -> PasteTree {
        u_hist
            .get(k)
            .or(v_hist.get(k))
            .map(|h| h.source.clone())
            .unwrap_or_else(missing_tree)
    };
    (0..num_dims)
        .map(|k| BoundaryHistory::from_pair(
            paste_tree(u_hist, v_hist, n, k, Sign::Source, &dummy),
            paste_tree(u_hist, v_hist, n, k, Sign::Target, &dummy),
        ))
        .collect()
}

/// The paste tree for `sign` at dimension `k` when pasting `u` and `v` at dimension `n`.
///
/// - k < n:  inherit from u
/// - k == n: source from u, target from v
/// - k > n:  join u and v into a Node at dimension n
fn paste_tree(
    u_hist: &[BoundaryHistory],
    v_hist: &[BoundaryHistory],
    n: usize,
    k: usize,
    sign: Sign,
    dummy: &dyn Fn(usize) -> PasteTree,
) -> PasteTree {
    if k < n {
        history_tree(u_hist, sign, k, || dummy(k))
    } else if k == n {
        let hist = match sign { Sign::Source => u_hist, Sign::Target => v_hist };
        history_tree(hist, sign, n, || dummy(n))
    } else {
        PasteTree::Node {
            dim: n,
            left:  Arc::new(history_tree(u_hist, sign, k, || dummy(k))),
            right: Arc::new(history_tree(v_hist, sign, k, || dummy(k))),
        }
    }
}

/// For each of `n` top-boundary cells, produce a coface list pointing to the
/// single new top cell (index 0) if the cell has a preimage in `inv`, or empty
/// otherwise.
fn cofaces_to_top(n: usize, inv: &[usize]) -> Vec<super::intset::IntSet> {
    (0..n)
        .map(|idx| {
            if inv.get(idx).copied().unwrap_or(NO_PREIMAGE) != NO_PREIMAGE {
                vec![0usize]
            } else {
                vec![]
            }
        })
        .collect()
}

/// Build the ogposet shape for an n-dimensional generating cell.
///
/// Given the pushout `bd_uv` of the source and target boundaries (with
/// injections `inl` from source and `inr` from target), constructs a new
/// ogposet with one extra cell at dimension `d + 1` whose source faces are
/// the `inl` image and whose target faces are the `inr` image.
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
        cofaces_in.push(cofaces_to_top(n, &inl.inv[d]));
        cofaces_out.push(cofaces_to_top(n, &inr.inv[d]));
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

/// Build the paste history for a new `d`-dimensional generating cell with
/// the given source and target boundary histories.
///
/// - Dimensions below `d`: carry through the source boundary's history.
/// - Dimension `d`: source from the source boundary, target from the target boundary.
/// - Dimension `d + 1`: both halves are a `Leaf(tag)` (the cell itself).
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

#[cfg(test)]
mod tests {
    use super::{CellData, Diagram, Sign};
    use crate::aux::Tag;

    #[test]
    fn boundary_normal_clamps_history_to_top_dim() {
        let point = Diagram::cell(Tag::Local("pt".into()), &CellData::Zero).unwrap();

        let boundary = Diagram::boundary_normal(Sign::Source, 5, &point).unwrap();

        assert_eq!(boundary.labels.len(), 1);
        assert_eq!(boundary.paste_history.len(), 1);
    }
}
