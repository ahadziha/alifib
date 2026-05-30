//! Paste trees and operations on them.
//!
//! A [`PasteTree`] records *one* way of assembling a diagram from generators by
//! iterated pasting: leaves are generating cells, nodes paste their children at
//! a recorded dimension.  [`Diagram`](super::diagram::Diagram) carries such a
//! tree as its assembly history.
//!
//! This module collects the operations on paste trees that stand apart from a
//! diagram's own bookkeeping:
//!
//! - [`realise_tree`] rebuilds the diagram a tree describes;
//! - [`PasteTree::substitute`] rewrites leaves;
//! - [`pseudo_normalise`] rewrites a tree into a canonical *pseudo-normal* form
//!   in which the outermost paste is always at the highest dimension occurring,
//!   with units removed, leaving the realised diagram unchanged up to
//!   isomorphism.

use std::sync::Arc;

use super::complex::Complex;
use super::diagram::{Diagram, Sign};
use crate::aux::{Error, Tag};

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

impl PasteTree {
    /// Replace every leaf whose tag satisfies `f` with the tree `f` returns.
    /// Leaves where `f` returns `None` are kept unchanged.
    pub(crate) fn substitute(&self, f: &impl Fn(&Tag) -> Option<PasteTree>) -> PasteTree {
        match self {
            PasteTree::Leaf(tag) => f(tag).unwrap_or_else(|| self.clone()),
            PasteTree::Node { dim, left, right } => PasteTree::Node {
                dim: *dim,
                left: Arc::new(left.substitute(f)),
                right: Arc::new(right.substitute(f)),
            },
        }
    }
}

/// Reconstruct a diagram from a paste tree by looking up each leaf's classifier
/// diagram in `complex` and pasting at the recorded dimensions.
///
/// - `Leaf(tag)` → the classifier diagram of the generator with that tag.
/// - `Node { dim, left, right }` → `paste(dim, realise(left), realise(right))`.
pub(crate) fn realise_tree(tree: &PasteTree, complex: &Complex) -> Result<Diagram, Error> {
    match tree {
        PasteTree::Leaf(tag) => {
            let name = complex.find_generator_by_tag(tag)
                .ok_or_else(|| Error::new(format!("tag {} not found in complex", tag)))?;
            complex.classifier(name)
                .cloned()
                .ok_or_else(|| Error::new(format!("no classifier for '{}'", name)))
        }
        PasteTree::Node { dim, left, right } => {
            let d1 = realise_tree(left, complex)?;
            let d2 = realise_tree(right, complex)?;
            Diagram::paste(*dim, &d1, &d2)
        }
    }
}

/// Flatten the outermost chain of pastes at dimension `k` into the maximal
/// subtrees pasted at that dimension, left to right.  The dual of the term
/// printer's chain collection: `Node k (Node k a b) c` flattens to `[a, b, c]`.
pub(crate) fn flatten_at(tree: &PasteTree, k: usize) -> Vec<PasteTree> {
    fn go(t: &PasteTree, k: usize, out: &mut Vec<PasteTree>) {
        match t {
            PasteTree::Node { dim, left, right } if *dim == k => {
                go(left, k, out);
                go(right, k, out);
            }
            _ => out.push(t.clone()),
        }
    }
    let mut parts = Vec::new();
    go(tree, k, &mut parts);
    parts
}

/// The tags of every leaf whose generator has the tree's top dimension, in
/// left-to-right order and with multiplicity — the top-dimensional generators
/// the diagram is built from.
pub(crate) fn top_generators(tree: &PasteTree, complex: &Complex) -> Result<Vec<Tag>, Error> {
    fn collect(t: &PasteTree, top: usize, complex: &Complex, out: &mut Vec<Tag>) -> Result<(), Error> {
        match t {
            PasteTree::Leaf(tag) => {
                if leaf_dimension(tag, complex)? == top {
                    out.push(tag.clone());
                }
                Ok(())
            }
            PasteTree::Node { left, right, .. } => {
                collect(left, top, complex, out)?;
                collect(right, top, complex, out)
            }
        }
    }
    let top = tree_dimension(tree, complex)?;
    let mut tags = Vec::new();
    collect(tree, top, complex, &mut tags)?;
    Ok(tags)
}

// ── Pseudo-normalisation ────────────────────────────────────────────────────
//
// Many trees realise the same diagram: the interchange law lets a higher-
// dimensional paste slide above or below a lower-dimensional one.  Pseudo-
// normalisation picks a canonical representative: strip units (`remove_units`),
// then repeatedly lift the highest-dimensional paste to the root by interchange
// (`pseudo_normalise`).  The rewrite engine's `resume` uses this to recover the
// rewrite steps of a proof diagram.

/// The dimension of the generator a leaf names, looked up in `complex`.
fn leaf_dimension(tag: &Tag, complex: &Complex) -> Result<usize, Error> {
    let name = complex
        .find_generator_by_tag(tag)
        .ok_or_else(|| Error::new(format!("tag {} not found in complex", tag)))?;
    complex
        .find_generator(name)
        .map(|(_, dim)| dim)
        .ok_or_else(|| Error::new(format!("generator '{}' has no dimension", name)))
}

/// The highest dimension of any generator appearing at a leaf of `t`.
fn tree_dimension(t: &PasteTree, complex: &Complex) -> Result<usize, Error> {
    match t {
        PasteTree::Leaf(tag) => leaf_dimension(tag, complex),
        PasteTree::Node { left, right, .. } => {
            Ok(tree_dimension(left, complex)?.max(tree_dimension(right, complex)?))
        }
    }
}

/// The highest pasting dimension occurring in a (unit-free) tree, or `-1` for a
/// leaf, which has no pastes.
fn pasting_dimension(t: &PasteTree) -> isize {
    match t {
        PasteTree::Leaf(_) => -1,
        PasteTree::Node { dim, left, right } => {
            (*dim as isize).max(pasting_dimension(left)).max(pasting_dimension(right))
        }
    }
}

/// Whether a *unit-free* tree is pseudo-normal: every node pastes at the highest
/// dimension occurring beneath it, recursively.
pub(crate) fn is_pseudo_normal(t: &PasteTree) -> bool {
    match t {
        PasteTree::Leaf(_) => true,
        PasteTree::Node { dim, left, right } => {
            pasting_dimension(t) == *dim as isize
                && is_pseudo_normal(left)
                && is_pseudo_normal(right)
        }
    }
}

/// Remove units: a paste `t1 *ₖ t2` in which one side has dimension ≤ k is that
/// side acting as a unit, so it collapses to the other side.
pub(crate) fn remove_units(t: &PasteTree, complex: &Complex) -> Result<PasteTree, Error> {
    match t {
        PasteTree::Leaf(_) => Ok(t.clone()),
        PasteTree::Node { dim, left, right } => {
            if tree_dimension(left, complex)? <= *dim {
                remove_units(right, complex)
            } else if tree_dimension(right, complex)? <= *dim {
                remove_units(left, complex)
            } else {
                Ok(paste_node(
                    *dim,
                    remove_units(left, complex)?,
                    remove_units(right, complex)?,
                ))
            }
        }
    }
}

/// Pseudo-normalise a paste tree, preserving the realised diagram up to
/// isomorphism.  The result is always unit-free and pseudo-normal.
///
/// After stripping units, a node `t1 *ₖ t2` whose pasting dimension `k` is not
/// already the maximum is rewritten by interchange so that the *highest* nested
/// pasting dimension `j` becomes the root.  Splitting the dominant side `Node j
/// u1 u2`, interchange gives
///
/// ```text
///   (u1 *ⱼ u2) *ₖ w  =  (u1 *ₖ w) *ⱼ (u2 *ₖ ∂⁺ⱼ w)
///   w *ₖ (u1 *ⱼ u2)  =  (w *ₖ u1) *ⱼ (∂⁺ⱼ w *ₖ u2)
/// ```
///
/// (with `w` the other side), and we recurse into the two new `*ₖ` pastes.
///
/// Note: we lift `j = max(pasting_dimension t1, pasting_dimension t2)` — the
/// *larger* side — rather than always the first.  Lifting the first would leave
/// a higher paste stranded below the root, realising the same diagram but
/// failing to be pseudo-normal.
pub(crate) fn pseudo_normalise(t: &PasteTree, complex: &Complex) -> Result<PasteTree, Error> {
    let t = remove_units(t, complex)?;
    let (k, t1, t2) = match &t {
        PasteTree::Leaf(_) => return Ok(t),
        PasteTree::Node { dim, left, right } => (*dim, (**left).clone(), (**right).clone()),
    };

    let t1 = pseudo_normalise(&t1, complex)?;
    let t2 = pseudo_normalise(&t2, complex)?;
    let (pd1, pd2, ki) = (pasting_dimension(&t1), pasting_dimension(&t2), k as isize);

    // `k` already dominates: the node is pseudo-normal as it stands.
    if ki >= pd1 && ki >= pd2 {
        return Ok(paste_node(k, t1, t2));
    }

    if pd1 >= pd2 {
        // Lift t1's root j > k:  (u1 *ⱼ u2) *ₖ t2.
        let (j, u1, u2) = split_node(&t1)?;
        let bd = target_boundary_tree(j, &t2, complex)?;
        Ok(paste_node(
            j,
            pseudo_normalise(&paste_node(k, u1, t2.clone()), complex)?,
            pseudo_normalise(&paste_node(k, u2, bd), complex)?,
        ))
    } else {
        // Lift t2's root j > k:  t1 *ₖ (u1 *ⱼ u2).
        let (j, u1, u2) = split_node(&t2)?;
        let bd = target_boundary_tree(j, &t1, complex)?;
        Ok(paste_node(
            j,
            pseudo_normalise(&paste_node(k, t1.clone(), u1), complex)?,
            pseudo_normalise(&paste_node(k, bd, u2), complex)?,
        ))
    }
}

/// The target `j`-boundary `∂⁺ⱼ` of the diagram `t` realises, returned as a
/// paste tree ready to splice back into a larger tree.
fn target_boundary_tree(j: usize, t: &PasteTree, complex: &Complex) -> Result<PasteTree, Error> {
    let diagram = realise_tree(t, complex)?;
    let boundary = Diagram::boundary(Sign::Target, j, &diagram)?;
    let top = boundary.top_dim();
    boundary
        .tree(Sign::Source, top)
        .cloned()
        .ok_or_else(|| Error::new("boundary diagram carries no paste tree"))
}

/// Build a paste node, wrapping the children in `Arc`.
fn paste_node(dim: usize, left: PasteTree, right: PasteTree) -> PasteTree {
    PasteTree::Node {
        dim,
        left: Arc::new(left),
        right: Arc::new(right),
    }
}

/// Decompose a node, cloning its children out.  A pseudo-normal tree of positive
/// pasting dimension is always a node, so this never fails where we call it.
fn split_node(t: &PasteTree) -> Result<(usize, PasteTree, PasteTree), Error> {
    match t {
        PasteTree::Node { dim, left, right } => Ok((*dim, (**left).clone(), (**right).clone())),
        PasteTree::Leaf(_) => Err(Error::new("a leaf has no pasting dimension to lift")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::loader::Loader;
    use crate::core::complex::Complex;
    use crate::interpreter::InterpretedFile;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn fixture(name: &str) -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn examples_dir() -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .to_string_lossy()
            .into_owned()
    }

    /// Load a file and return the named type's complex, with `extra` search paths
    /// for resolving `include`s.
    fn load_type(path: &str, type_name: &str, extra: Vec<String>) -> Arc<Complex> {
        let loader = Loader::default(extra);
        let file = InterpretedFile::load(&loader, path).ok().expect("fixture should load");
        let store = Arc::clone(&file.state);
        let module = store.find_module(&file.path).expect("module should exist");
        let (tag, _) = module.find_generator(type_name).expect("type not found");
        let gid = match tag { Tag::Global(gid) => *gid, _ => panic!("expected global tag") };
        store.find_type(gid).expect("type entry not found").complex.clone()
    }

    // ── realise_tree ──────────────────────────────────────────────────────────

    /// For a diagram, realise from its top-dim source paste tree and check
    /// isomorphism with the original.
    fn assert_realise_roundtrip(diagram: &Diagram, complex: &Complex, label: &str) {
        let n = diagram.top_dim();
        let tree = diagram.tree(Sign::Source, n)
            .unwrap_or_else(|| panic!("{}: no paste tree at dim {}", label, n));
        let reconstructed = realise_tree(tree, complex)
            .unwrap_or_else(|e| panic!("{}: realise_tree failed: {}", label, e));
        assert!(
            Diagram::isomorphic(diagram, &reconstructed),
            "{}: reconstructed diagram is not isomorphic to original",
            label,
        );
    }

    #[test]
    fn realise_generator_classifier() {
        // A generator's classifier is a single cell — realising its Leaf tree
        // should return an isomorphic diagram.
        let complex = load_type(&fixture("Idem.ali"), "Idem", vec![]);
        let id_diag = complex.classifier("id").expect("id classifier");
        assert_realise_roundtrip(id_diag, &complex, "id");
    }

    #[test]
    fn realise_composite_diagram_dim1() {
        // lhs = id id id — a 3-cell paste at dim 0.
        let complex = load_type(&fixture("Idem.ali"), "Idem", vec![]);
        let lhs = complex.find_diagram("lhs").expect("lhs diagram");
        assert_realise_roundtrip(lhs, &complex, "lhs");
    }

    #[test]
    fn realise_composite_diagram_dim2() {
        // lhs2 = alpha alpha alpha — a 3-cell paste at dim 1.
        let complex = load_type(&fixture("Assoc.ali"), "Assoc", vec![]);
        let lhs2 = complex.find_diagram("lhs2").expect("lhs2 diagram");
        assert_realise_roundtrip(lhs2, &complex, "lhs2");
    }

    #[test]
    fn realise_single_cell_diagram() {
        // rhs = id — a single cell, same as the classifier.
        let complex = load_type(&fixture("Idem.ali"), "Idem", vec![]);
        let rhs = complex.find_diagram("rhs").expect("rhs diagram");
        assert_realise_roundtrip(rhs, &complex, "rhs");
    }

    #[test]
    fn realise_generator_with_composite_boundary() {
        // m : Ob.ob Ob.ob -> Ob.ob — its classifier has composite boundaries.
        let complex = load_type(&fixture("Magma.ali"), "Magma", vec![]);
        let m_diag = complex.classifier("m").expect("m classifier");
        assert_realise_roundtrip(m_diag, &complex, "m");
    }

    #[test]
    fn realise_idem_classifier() {
        // idem : id id -> id — a 2-cell with composite source boundary.
        let complex = load_type(&fixture("Idem.ali"), "Idem", vec![]);
        let idem_diag = complex.classifier("idem").expect("idem classifier");
        assert_realise_roundtrip(idem_diag, &complex, "idem");
    }

    #[test]
    fn realise_beta_classifier() {
        // beta : alpha alpha -> alpha — a 3-cell.
        let complex = load_type(&fixture("Assoc.ali"), "Assoc", vec![]);
        let beta_diag = complex.classifier("beta").expect("beta classifier");
        assert_realise_roundtrip(beta_diag, &complex, "beta");
    }

    // ── pseudo-normalisation ──────────────────────────────────────────────────

    /// Pseudo-normalise the top-dimensional source paste tree of `diagram`, then
    /// check the result is genuinely pseudo-normal and rebuilds to an isomorphic
    /// diagram.
    fn assert_pseudo_normal_roundtrip(diagram: &Diagram, complex: &Complex, label: &str) {
        let n = diagram.top_dim();
        let tree = diagram.tree(Sign::Source, n)
            .unwrap_or_else(|| panic!("{}: no paste tree at dim {}", label, n));

        let normalised = pseudo_normalise(tree, complex)
            .unwrap_or_else(|e| panic!("{}: pseudo_normalise failed: {}", label, e));

        assert!(
            is_pseudo_normal(&normalised),
            "{}: pseudo_normalise output is not pseudo-normal: {:?}",
            label, normalised,
        );

        let rebuilt = realise_tree(&normalised, complex)
            .unwrap_or_else(|e| panic!("{}: realise_tree failed: {}", label, e));

        assert!(
            Diagram::isomorphic(diagram, &rebuilt),
            "{}: rebuilt diagram is not isomorphic to the original",
            label,
        );
    }

    /// `(α *₁ α) *₀ α`: a dimension-0 paste sitting above a dimension-1 paste —
    /// the canonical non-pseudo-normal interchange shape.
    #[test]
    fn interchange_left_nested() {
        let complex = load_type(&fixture("Assoc.ali"), "Assoc", vec![]);
        let alpha = complex.classifier("alpha").expect("alpha classifier");

        let vert = Diagram::paste(1, alpha, alpha).expect("vertical paste");
        let diagram = Diagram::paste(0, &vert, alpha).expect("horizontal paste");

        let tree = diagram.tree(Sign::Source, diagram.top_dim()).unwrap();
        assert!(!is_pseudo_normal(tree), "test setup: tree should not be pseudo-normal");

        assert_pseudo_normal_roundtrip(&diagram, &complex, "(α*₁α)*₀α");
    }

    /// The mirror shape `α *₀ (α *₁ α)`, exercising the `t2`-split branch.
    #[test]
    fn interchange_right_nested() {
        let complex = load_type(&fixture("Assoc.ali"), "Assoc", vec![]);
        let alpha = complex.classifier("alpha").expect("alpha classifier");

        let vert = Diagram::paste(1, alpha, alpha).expect("vertical paste");
        let diagram = Diagram::paste(0, alpha, &vert).expect("horizontal paste");

        let tree = diagram.tree(Sign::Source, diagram.top_dim()).unwrap();
        assert!(!is_pseudo_normal(tree), "test setup: tree should not be pseudo-normal");

        assert_pseudo_normal_roundtrip(&diagram, &complex, "α*₀(α*₁α)");
    }

    /// Idempotence on already-normal input: pseudo-normalising `α *₀ α` (a single
    /// dimension-0 paste of two cells) is a no-op up to isomorphism.
    #[test]
    fn already_pseudo_normal_is_stable() {
        let complex = load_type(&fixture("Assoc.ali"), "Assoc", vec![]);
        let alpha = complex.classifier("alpha").expect("alpha classifier");
        let diagram = Diagram::paste(0, alpha, alpha).expect("horizontal paste");

        let tree = diagram.tree(Sign::Source, diagram.top_dim()).unwrap();
        assert!(is_pseudo_normal(tree), "α*₀α should already be pseudo-normal");
        assert_pseudo_normal_roundtrip(&diagram, &complex, "α*₀α");
    }

    /// The lambda-sigma rewriting examples: real, larger paste trees that mix
    /// pasting dimensions.  Every `example_N` whose top tree is not already
    /// pseudo-normal is normalised and round-tripped.
    #[test]
    fn lambda_sigma_examples_roundtrip() {
        let complex = load_type(&fixture("bench_rewrite.ali"), "LambdaSigma.LambdaSigma_var", vec![examples_dir()]);

        let mut tested = 0;
        for (name, diagram) in complex.diagrams_iter() {
            if !name.starts_with("example_") {
                continue;
            }
            let n = diagram.top_dim();
            let Some(tree) = diagram.tree(Sign::Source, n) else { continue };
            if is_pseudo_normal(tree) {
                continue;
            }
            assert_pseudo_normal_roundtrip(diagram, &complex, name);
            tested += 1;
        }

        assert!(tested > 0, "expected at least one non-pseudo-normal example to test");
    }
}
