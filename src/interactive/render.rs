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

/// Render a rewrite step's paste tree expression, highlighting every rewrite
/// rule leaf (top-dimension cell) with `[brackets]`.
///
/// For example, if the step's paste tree renders as `(a #0 idem) #0 b`,
/// the result is `(a #0 [idem]) #0 b`.  Works for both individual and
/// parallel rewrites.
pub fn render_step(step: &Diagram, scope: &Complex) -> String {
    let n_plus_1 = step.top_dim();
    let tree = match step.tree(Sign::Source, n_plus_1) {
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
    proof: Option<(&str, &str, &str)>,
) {
    display.meta(&format!("{:<18}  {}", "[REMAINING SOURCE]", render_diagram(current, scope)));
    if let Some(t) = target {
        display.meta(&format!("{:<18}  {}", "[TARGET]", render_diagram(t, scope)));
    }
    display.blank();

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
    let n_plus_1 = if let Some(pr) = rewrites.first() { pr.step.top_dim() } else { return };
    let n = n_plus_1.saturating_sub(1);

    for (idx, pr) in rewrites.iter().enumerate() {
        let highlight = render_step(&pr.step, scope);
        let colored_highlight = display.colorize_match_display(&highlight);
        display.blank();
        display.inspect_rich(&format!("  ({idx}) {colored_highlight}"));

        let rule_names: Vec<&str> = pr.members.iter()
            .map(|m| m.rule_name.as_str())
            .collect();
        if pr.members.len() == 1 {
            let (rule_src, rule_tgt) = pr.step.labels_at(n_plus_1)
                .and_then(|ls| ls.first())
                .and_then(|tag| scope.find_generator_by_tag(tag))
                .and_then(|name| scope.classifier(name))
                .and_then(|classifier| {
                    let src = Diagram::boundary(Sign::Source, n, classifier).ok()?;
                    let tgt = Diagram::boundary(Sign::Target, n, classifier).ok()?;
                    Some((render_diagram(&src, scope), render_diagram(&tgt, scope)))
                })
                .unwrap_or_else(|| ("?".to_string(), "?".to_string()));
            let colored_src = display.paint_source(&rule_src);
            let colored_tgt = display.paint_target(&rule_tgt);
            display.inspect_rich(&format!("      by {} : {} -> {}",
                rule_names[0], colored_src, colored_tgt));
        } else {
            display.inspect_rich(&format!("      parallel: {}", rule_names.join(", ")));
        }
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
