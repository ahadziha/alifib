//! Holes in a partial map.
//!
//! A [`MapHole`] records one unfilled assignment in a "map with holes": the
//! unknown image of a single domain generator, written `arr => ?`.  Because a
//! filler is a (possibly non-round) diagram and not a generator, a hole is never
//! built into a semantic [`Diagram`][crate::core::diagram::Diagram]; instead its
//! boundaries are kept as paste trees whose leaves may themselves be
//! metavariables ([`Tag::Hole`]).  Filling a hole — substituting a filler's
//! paste tree for its metavariable, then realising — is handled elsewhere.

use crate::aux::{HoleId, Tag};
use crate::core::paste_tree::PasteTree;
use std::collections::BTreeSet;

/// One unfilled hole: the unknown image of a single domain generator under a map.
#[derive(Debug, Clone)]
pub struct MapHole {
    /// The metavariable standing for this image.  Equal to the [`HoleId`] inside
    /// the [`Tag::Hole`] leaf that dependent holes use to reference this one.
    pub(crate) meta: HoleId,
    /// The domain generator whose image is unknown (the `arr` in `arr => ?`).
    pub(crate) source: Tag,
    /// Dimension of the source generator = dimension the filler must have.
    pub(crate) dim: usize,
    /// Input boundary of the image, as a paste tree whose leaves are either
    /// concrete image tags (from the real map) or [`Tag::Hole`] metavariables.
    /// `None` for a 0-cell, which has no boundary.
    pub(crate) boundary_in: Option<PasteTree>,
    /// Output boundary of the image, dual to `boundary_in`.  `None` for a 0-cell.
    pub(crate) boundary_out: Option<PasteTree>,
    /// The metavariables referenced by either boundary tree — the holes this one
    /// depends on, which must be filled first.  Used to order filling and to
    /// render the dependency hierarchy.
    pub(crate) deps: BTreeSet<HoleId>,
}

/// Collect the metavariables (`Tag::Hole`) appearing as leaves of `tree`.
pub(crate) fn collect_hole_deps(tree: &PasteTree) -> BTreeSet<HoleId> {
    fn go(t: &PasteTree, acc: &mut BTreeSet<HoleId>) {
        match t {
            PasteTree::Leaf(Tag::Hole(id)) => {
                acc.insert(*id);
            }
            PasteTree::Leaf(_) => {}
            PasteTree::Node { left, right, .. } => {
                go(left, acc);
                go(right, acc);
            }
        }
    }
    let mut acc = BTreeSet::new();
    go(tree, &mut acc);
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::GlobalId;
    use std::sync::Arc;

    #[test]
    fn collects_only_hole_leaves() {
        let h1 = HoleId::fresh();
        let h2 = HoleId::fresh();
        // A tree mixing a concrete leaf and two metavariable leaves.
        let tree = PasteTree::Node {
            dim: 0,
            left: Arc::new(PasteTree::Leaf(Tag::Hole(h1))),
            right: Arc::new(PasteTree::Node {
                dim: 0,
                left: Arc::new(PasteTree::Leaf(Tag::Global(GlobalId::fresh()))),
                right: Arc::new(PasteTree::Leaf(Tag::Hole(h2))),
            }),
        };
        let deps = collect_hole_deps(&tree);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&h1) && deps.contains(&h2));
    }

    #[test]
    fn no_holes_is_empty() {
        let tree = PasteTree::Leaf(Tag::Global(GlobalId::fresh()));
        assert!(collect_hole_deps(&tree).is_empty());
    }
}
