//! Shared rendering helpers for the REPL.
//!
//! Pure string-building functions (`render_*`) are separated from display
//! functions (`print_*`) that accept a [`Display`] and produce output.
//! All output goes through `Display`; no `println!` appears here.

use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, PasteTree, Sign};
use crate::core::matching::{MatchResult, ParallelMatchResult};
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

    let subs = std::collections::HashMap::from([(rule_tag.clone(), source_render)]);
    render_tree_with_substitutions(tree, scope, &subs)
}

/// Render a parallel match by taking the step diagram's paste tree and replacing
/// every rewrite rule leaf with a bracketed rendering of that rule's source boundary.
pub fn render_parallel_match_from_step(
    step: &Diagram,
    scope: &Complex,
) -> String {
    let n_plus_1 = step.top_dim();
    let tree = match step.tree(Sign::Source, n_plus_1) {
        Some(t) => t,
        None => return "?".to_string(),
    };

    let Some(labels) = step.labels_at(n_plus_1) else {
        return "?".to_string();
    };

    let n = n_plus_1.saturating_sub(1);
    let mut subs = std::collections::HashMap::new();
    for tag in labels {
        if subs.contains_key(tag) { continue; }
        let source_render = scope.find_generator_by_tag(tag)
            .and_then(|name| scope.classifier(name))
            .and_then(|classifier| Diagram::boundary(Sign::Source, n, classifier).ok())
            .map(|src| render_diagram(&src, scope))
            .unwrap_or_else(|| "?".to_string());
        subs.insert(tag.clone(), source_render);
    }

    render_tree_with_substitutions(tree, scope, &subs)
}

/// Render a paste tree, substituting specific leaf tags with bracketed strings.
/// Chains at the same dimension are flattened.
fn render_tree_with_substitutions(
    tree: &PasteTree,
    scope: &Complex,
    subs: &std::collections::HashMap<Tag, String>,
) -> String {
    match tree {
        PasteTree::Leaf(tag) => {
            if let Some(replacement) = subs.get(tag) {
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
            collect_chain_with_subs(tree, k, scope, subs, &mut parts);
            format!("({})", parts.join(&format!(" #{} ", k)))
        }
    }
}

fn collect_chain_with_subs(
    tree: &PasteTree,
    k: usize,
    scope: &Complex,
    subs: &std::collections::HashMap<Tag, String>,
    parts: &mut Vec<String>,
) {
    match tree {
        PasteTree::Node { dim, left, right } if *dim == k => {
            collect_chain_with_subs(left, k, scope, subs, parts);
            collect_chain_with_subs(right, k, scope, subs, parts);
        }
        _ => parts.push(render_tree_with_substitutions(tree, scope, subs)),
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
    parallel_rewrites: &[ParallelMatchResult],
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

    let total = parallel_rewrites.len() + rewrites.len();
    if total == 0 {
        display.meta("no rewrites available");
        return;
    }

    display.meta("rewrites:");
    let mut idx = 0usize;

    // Parallel families first.
    for pr in parallel_rewrites {
        let highlight = render_parallel_match_from_step(&pr.step, scope);
        let rule_names: Vec<&str> = pr.family.iter()
            .map(|&i| rewrites[i].rule_name.as_str())
            .collect();
        let colored_highlight = display.colorize_match_display(&highlight);
        display.blank();
        display.inspect_rich(&format!("  ({idx}) {colored_highlight}"));
        display.inspect_rich(&format!("      parallel: {}", rule_names.join(", ")));
        idx += 1;
    }

    // Individual matches.
    for m in rewrites {
        let highlight = render_match_from_step(&m.step, scope);
        let n_plus_1 = m.step.top_dim();
        let n = n_plus_1.saturating_sub(1);
        let rule_tag = m.step.labels_at(n_plus_1).and_then(|ls| ls.first());
        let (rule_src, rule_tgt) = rule_tag
            .and_then(|tag| scope.find_generator_by_tag(tag))
            .and_then(|name| scope.classifier(name))
            .and_then(|classifier| {
                let src = Diagram::boundary(Sign::Source, n, classifier).ok()?;
                let tgt = Diagram::boundary(Sign::Target, n, classifier).ok()?;
                Some((render_diagram(&src, scope), render_diagram(&tgt, scope)))
            })
            .unwrap_or_else(|| ("?".to_string(), "?".to_string()));
        let colored_highlight = display.colorize_match_display(&highlight);
        let colored_src = display.paint_source(&rule_src);
        let colored_tgt = display.paint_target(&rule_tgt);
        display.blank();
        display.inspect_rich(&format!("  ({idx}) {colored_highlight}"));
        display.inspect_rich(&format!("      by {} : {} -> {}", m.rule_name, colored_src, colored_tgt));
        idx += 1;
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
