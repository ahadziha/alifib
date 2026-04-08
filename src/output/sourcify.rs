//! Sourcefier: convert a [`Diagram`]'s paste-tree back to valid `.ali` source syntax.
//!
//! The inverse of the interpreter's paste evaluation: a [`PasteTree::Leaf`] becomes a
//! generator name and a [`PasteTree::Node`] becomes `lhs #dim rhs`, matching the parser's
//! `#k` paste syntax.  Left-associativity is preserved naturally by the recursive descent.

use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, PasteTree, Sign};
use crate::output::render_diagram;

/// Convert a [`PasteTree`] to its `.ali` source representation.
///
/// - `Leaf(tag)` → generator name from `scope` (falls back to the tag's display form)
/// - `Node { dim, left, right }` → `"left #dim right"`
fn paste_tree_to_source(tree: &PasteTree, scope: &Complex) -> String {
    match tree {
        PasteTree::Leaf(tag) => scope
            .find_generator_by_tag(tag)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", tag)),
        PasteTree::Node { dim, left, right } => {
            let l = paste_tree_to_source(left, scope);
            let r = paste_tree_to_source(right, scope);
            format!("{} #{} {}", l, dim, r)
        }
    }
}

/// Convert a [`Diagram`] to its `.ali` source expression by reading its top-dim source
/// paste tree.
///
/// Falls back to flat label rendering (space-separated generator names) for diagrams that
/// have no paste history — e.g. those loaded directly from the interpreter's initial state.
pub fn diagram_to_source(diagram: &Diagram, scope: &Complex) -> String {
    let n = diagram.top_dim();
    match diagram.tree(Sign::Source, n) {
        Some(tree) => paste_tree_to_source(tree, scope),
        None => render_diagram(diagram, scope),
    }
}
