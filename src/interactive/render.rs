//! Shared rendering helpers for the REPL.
//!
//! Pure string-building functions (`render_*`) are separated from display
//! functions (`print_*`) that accept a [`Display`] and produce output.
//! All output goes through `Display`; no `println!` appears here.

use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::paste_tree::PasteTree;
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

// ── Display functions ─────────────────────────────────────────────────────────

/// Pad a styled label to `width` columns based on its *plain* text.
///
/// Painting changes the byte length, so we measure the uncoloured label and
/// append the trailing spaces afterwards — keeping columns aligned whether or
/// not colour is enabled.
fn label(painted: String, plain: &str, width: usize) -> String {
    format!("{painted}{}", " ".repeat(width.saturating_sub(plain.len())))
}

/// Display the current rewrite state.
///
/// Format:
/// ```text
/// >> source    <current diagram>
/// >> target    <target diagram>
/// (blank)
/// >> rewrites:
/// (blank)
/// >>   (0) [id id] id
/// >>       by idem : id id -> id
/// >> ...
/// ```
/// `source`/`target` are styled labels; `rewrites:` is a section title; the
/// `(idx)` badges are accent-coloured.  If the target is reached, prints a green
/// `Rewrite complete.` and the proof cell.
pub fn print_state(
    display: &Display,
    current: &Diagram,
    target: Option<&Diagram>,
    rewrites: &[MatchResult],
    scope: &Complex,
    proof: Option<(&str, &str, &str)>,
) {
    display.inspect_rich(&format!("{}  {}",
        label(display.paint_source("source"), "source", 8),
        render_diagram(current, scope)));
    if let Some(t) = target {
        display.inspect_rich(&format!("{}  {}",
            label(display.paint_target("target"), "target", 8),
            render_diagram(t, scope)));
    }
    display.blank();

    if let Some((src_label, tgt_label, proof_label)) = proof {
        display.inspect_rich(&display.ok("Rewrite complete."));
        display.blank();
        display.inspect_rich(&display.sec("proof:"));
        display.inspect_rich(&format!("  {} {} {} {} {}",
            display.hi(proof_label),
            display.dim(":"),
            display.paint_source(src_label),
            display.dim("->"),
            display.paint_target(tgt_label)));
        return;
    }

    if rewrites.is_empty() {
        display.inspect_rich(&display.dim("no rewrites available"));
        return;
    }

    display.inspect_rich(&display.sec("rewrites:"));
    let n_plus_1 = if let Some(pr) = rewrites.first() { pr.step.top_dim() } else { return };
    let n = n_plus_1.saturating_sub(1);

    for (idx, pr) in rewrites.iter().enumerate() {
        let highlight = render_step(&pr.step, scope);
        let colored_highlight = display.colorize_match_display(&highlight);
        display.blank();
        display.inspect_rich(&format!("  {} {}",
            display.acc(&format!("({idx})")), colored_highlight));

        let rule_names: Vec<&str> = pr.members.iter()
            .map(|m| m.rule_name.as_str())
            .collect();
        if pr.members.len() == 1 {
            let (rule_src, rule_tgt) = pr.step.labels_at(n_plus_1)
                .and_then(|ls| ls.first())
                .and_then(|tag| scope.find_generator_by_tag(tag))
                .and_then(|name| scope.classifier(name))
                .and_then(|classifier| {
                    let src = Diagram::boundary(Sign::Input, n, classifier).ok()?;
                    let tgt = Diagram::boundary(Sign::Output, n, classifier).ok()?;
                    Some((render_diagram(&src, scope), render_diagram(&tgt, scope)))
                })
                .unwrap_or_else(|| ("?".to_string(), "?".to_string()));
            display.inspect_rich(&format!("      {} {} {} {} {}",
                display.dim("by"),
                display.hi(rule_names[0]),
                display.dim(":"),
                display.paint_source(&rule_src),
                format!("{} {}", display.dim("->"), display.paint_target(&rule_tgt))));
        } else {
            display.inspect_rich(&format!("      {} {}",
                display.dim("parallel:"),
                display.hi(&rule_names.join(", "))));
        }
    }
}

/// Display the move history.
///
/// Mirrors the web composition: dim step label, bold rule name, dim choice tag.
pub fn print_history(
    display: &Display,
    source: &Diagram,
    history_entries: &[(Option<Vec<usize>>, &str)],
    scope: &Complex,
) {
    display.inspect_rich(&format!("{} {}",
        display.dim("step 0 (source):"), render_diagram(source, scope)));
    for (i, (choice, rule)) in history_entries.iter().enumerate() {
        let tag = match choice {
            Some(v) if v.len() == 1 => format!("choice {}", v[0]),
            Some(v) => format!("choice {}", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")),
            None => "n/a".into(),
        };
        display.inspect_rich(&format!("{} {} {}",
            display.dim(&format!("step {}", i + 1)),
            display.hi(rule),
            display.dim(&format!("({tag})"))));
    }
}
