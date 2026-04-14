//! Shared rendering helpers for the REPL.
//!
//! Pure string-building functions (`render_*`) are separated from display
//! functions (`print_*`) that accept a [`Display`] and produce output.
//! All output goes through `Display`; no `println!` appears here.

use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, PasteTree, Sign};
use crate::core::matching::MatchResult;
use crate::output::render_diagram;
use super::display::Display;

// ── Pure string builders ──────────────────────────────────────────────────────

/// Render a match by taking the step diagram's paste tree and replacing the
/// rewrite rule leaf (the single (n+1)-dimensional cell) with a bracketed
/// rendering of the rule's source boundary.
///
/// For example, if the step's paste tree renders as `(a #0 rewrite) #0 b`
/// and the rewrite's source is `(a #0 a)`, the result is
/// `(a #0 [(a #0 a)]) #0 b`.
pub fn render_match_from_step(
    step: &Diagram,
    scope: &Complex,
) -> String {
    let n_plus_1 = step.top_dim();
    // The step has exactly one (n+1)-cell; its tag is the rewrite rule.
    // Get the step's top-dim paste tree.
    let tree = match step.tree(Sign::Source, n_plus_1) {
        Some(t) => t,
        None => return "?".to_string(),
    };

    // Find the rewrite rule tag: the unique label at the top dimension.
    let rule_tag = match step.labels_at(n_plus_1) {
        Some(labels) if labels.len() == 1 => &labels[0],
        _ => return "?".to_string(),
    };

    // Get the rule's input boundary rendering (the pattern being matched).
    // Look up the rule's classifier and extract its input boundary.
    let n = n_plus_1.saturating_sub(1);
    let source_render = scope.find_generator_by_tag(rule_tag)
        .and_then(|name| scope.classifier(name))
        .and_then(|classifier| Diagram::boundary(Sign::Source, n, classifier).ok())
        .map(|src| render_diagram(&src, scope))
        .unwrap_or_else(|| "?".to_string());

    render_tree_with_substitution(tree, scope, rule_tag, &source_render)
}

/// Render a paste tree, substituting one specific leaf tag with a bracketed string.
/// Chains at the same dimension are flattened.
fn render_tree_with_substitution(
    tree: &PasteTree,
    scope: &Complex,
    replace_tag: &Tag,
    replacement: &str,
) -> String {
    match tree {
        PasteTree::Leaf(tag) => {
            if tag == replace_tag {
                format!("[{}]", replacement)
            } else {
                scope.find_generator_by_tag(tag)
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag))
            }
        }
        PasteTree::Node { dim, .. } => {
            let k = *dim;
            let mut parts = Vec::new();
            collect_chain_with_sub(tree, k, scope, replace_tag, replacement, &mut parts);
            format!("({})", parts.join(&format!(" #{} ", k)))
        }
    }
}

fn collect_chain_with_sub(
    tree: &PasteTree,
    k: usize,
    scope: &Complex,
    replace_tag: &Tag,
    replacement: &str,
    parts: &mut Vec<String>,
) {
    match tree {
        PasteTree::Node { dim, left, right } if *dim == k => {
            collect_chain_with_sub(left, k, scope, replace_tag, replacement, parts);
            collect_chain_with_sub(right, k, scope, replace_tag, replacement, parts);
        }
        _ => parts.push(render_tree_with_substitution(tree, scope, replace_tag, replacement)),
    }
}

// ── Display functions ─────────────────────────────────────────────────────────

/// Display the current rewrite state.
///
/// Format:
/// ```text
/// (blank)
/// >> [REMAINING SOURCE]  <current diagram>
/// >> [TARGET]            <target diagram>
/// (blank)
/// >> rewrites:
/// >>
/// >>   (0) [id id] id
/// >>       by idem : id id -> id
/// >> ...
/// ```
/// If the target is reached, prints `>> Rewrite complete.` and the proof cell.
pub fn print_state(
    display: &Display,
    current: &Diagram,
    target: Option<&Diagram>,
    rewrites: &[MatchResult],
    scope: &Complex,
    // Running proof for completion display (source label, target label, proof label).
    proof: Option<(&str, &str, &str)>,
) {
    display.meta(&format!("{:<18}  {}", "[REMAINING SOURCE]", render_diagram(current, scope)));
    if let Some(t) = target {
        display.meta(&format!("{:<18}  {}", "[TARGET]", render_diagram(t, scope)));
    }
    display.blank();

    // `proof` is Some only when target_reached() is true (steps taken + diagrams match).
    if let Some((src_label, tgt_label, proof_label)) = proof {
        display.meta("Rewrite complete.");
        display.blank();
        display.inspect("proof:");
        display.inspect(&format!("  {proof_label} : {src_label} -> {tgt_label}"));
        return;
    }

    if rewrites.is_empty() {
        display.meta("no rewrites available");
        return;
    }

    display.meta("rewrites:");
    for (i, m) in rewrites.iter().enumerate() {
        let highlight = render_match_from_step(&m.step, scope);
        let n = m.step.top_dim().saturating_sub(1);
        let (rule_src, rule_tgt) = match (
            Diagram::boundary(Sign::Source, n, &m.step),
            Diagram::boundary(Sign::Target, n, &m.step),
        ) {
            (Ok(src), Ok(tgt)) => (render_diagram(&src, scope), render_diagram(&tgt, scope)),
            _ => ("?".to_string(), "?".to_string()),
        };
        display.blank();
        display.inspect(&format!("  ({i}) {highlight}"));
        display.inspect(&format!("      by {} : {} -> {}", m.rule_name, rule_src, rule_tgt));
    }
}

/// Display the move history.
pub fn print_history(
    display: &Display,
    source: &Diagram,
    history_entries: &[(usize, &str)],
    scope: &Complex,
) {
    display.inspect(&format!("step 0 (source): {}", render_diagram(source, scope)));
    for (i, (choice, rule)) in history_entries.iter().enumerate() {
        display.inspect(&format!("step {} — {} (choice {})", i + 1, rule, choice));
    }
}
