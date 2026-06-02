//! The bracketed match-display string for a rewrite step.
//!
//! Transcript layout lives in [`super::richtext`]; this module only builds the
//! `(a #0 [idem]) #0 b` string stored in `RewriteInfo.match_display` (consumed by
//! [`super::protocol`] and re-parsed into segments by `richtext`).

use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::paste_tree::PasteTree;

/// Render a rewrite step's paste tree expression, highlighting every rewrite
/// rule leaf (top-dimension cell) with `[brackets]`.
///
/// For example, if the step's paste tree renders as `(a #0 idem) #0 b`,
/// the result is `(a #0 [idem]) #0 b`.  Works for both individual and
/// parallel rewrites.
pub fn render_step(step: &Diagram, scope: &Complex) -> String {
    let n_plus_1 = step.top_dim();
    let tree = match step.tree(Sign::Input, n_plus_1) {
        Some(t) => t,
        None => return "?".to_string(),
    };
    let rule_tags: std::collections::HashSet<&Tag> = step.labels_at(n_plus_1)
        .into_iter()
        .flat_map(|labels| labels.iter())
        .collect();
    render_tree_highlighting(tree, scope, &rule_tags)
}

fn render_tree_highlighting(
    tree: &PasteTree,
    scope: &Complex,
    highlight: &std::collections::HashSet<&Tag>,
) -> String {
    match tree {
        PasteTree::Leaf(tag) => {
            let name = scope.find_generator_by_tag(tag)
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("{}", tag));
            if highlight.contains(tag) {
                format!("[{}]", name)
            } else {
                name
            }
        }
        PasteTree::Node { dim, .. } => {
            let k = *dim;
            let mut parts = Vec::new();
            collect_chain_highlighting(tree, k, scope, highlight, &mut parts);
            format!("({})", parts.join(&format!(" #{} ", k)))
        }
    }
}

fn collect_chain_highlighting(
    tree: &PasteTree,
    k: usize,
    scope: &Complex,
    highlight: &std::collections::HashSet<&Tag>,
    parts: &mut Vec<String>,
) {
    match tree {
        PasteTree::Node { dim, left, right } if *dim == k => {
            collect_chain_highlighting(left, k, scope, highlight, parts);
            collect_chain_highlighting(right, k, scope, highlight, parts);
        }
        _ => parts.push(render_tree_highlighting(tree, scope, highlight)),
    }
}
