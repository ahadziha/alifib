//! Shared rendering helpers for the REPL and daemon.
//!
//! Builds on the low-level `output::render_diagram` function to produce
//! richer output: bracket-highlighted match positions, per-dimension cell
//! lists, and proof-trace summaries.

use crate::core::complex::Complex;
use crate::core::diagram::Diagram;
use crate::core::rewrite::CandidateRewrite;
use crate::output::render_diagram;

/// Render the top-dim labels of `diagram` with brackets around cells whose
/// positions appear in `match_positions`.
///
/// Example: labels `[id, id, id]`, positions `[0, 1]` → `"[id id] id"`.
pub fn render_match_highlight(
    diagram: &Diagram,
    scope: &Complex,
    match_positions: &[usize],
) -> String {
    let n = diagram.top_dim();
    let labels: Vec<String> = match diagram.labels_at(n) {
        Some(ls) if !ls.is_empty() => ls
            .iter()
            .map(|tag| {
                scope
                    .find_generator_by_tag(tag)
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag))
            })
            .collect(),
        _ => return "?".to_string(),
    };

    if match_positions.is_empty() {
        return labels.join(" ");
    }

    // Sort positions for contiguous-group detection.
    let mut positions: Vec<usize> = match_positions.to_vec();
    positions.sort_unstable();

    let mut result = String::new();
    let mut in_bracket = false;

    for (i, label) in labels.iter().enumerate() {
        let matched = positions.binary_search(&i).is_ok();

        if matched && !in_bracket {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push('[');
            in_bracket = true;
        } else if !matched && in_bracket {
            result.push(']');
            in_bracket = false;
            result.push(' ');
        } else if !result.is_empty() && !in_bracket {
            result.push(' ');
        }

        if in_bracket && i > *positions.first().unwrap_or(&0) && matched {
            result.push(' ');
        }

        result.push_str(label);
    }

    if in_bracket {
        result.push(']');
    }

    result
}

/// Per-dimension cell-count summary for a diagram, e.g. "dim 1, 3 cells".
pub fn render_dim_summary(diagram: &Diagram) -> String {
    let dim = diagram.top_dim();
    let count = diagram.labels_at(dim).map(|ls| ls.len()).unwrap_or(0);
    format!("dim {}, {} cell{}", dim, count, if count == 1 { "" } else { "s" })
}

/// Print a compact state display to stdout for the REPL.
///
/// Shows the step index, the current diagram label, and the full list of
/// available rewrites with bracket-highlighted match positions.
pub fn print_state(
    step: usize,
    current: &Diagram,
    target: Option<&Diagram>,
    rewrites: &[CandidateRewrite],
    scope: &Complex,
) {
    println!();
    println!("[{}] {}", step, render_diagram(current, scope));
    if let Some(t) = target {
        if Diagram::equal(current, t) {
            println!("  (target reached: {})", render_diagram(t, scope));
        }
    }
    println!();
    if rewrites.is_empty() {
        println!("  no rewrites available");
    } else {
        println!("rewrites:");
        for (i, c) in rewrites.iter().enumerate() {
            let highlight = render_match_highlight(current, scope, &c.image_positions);
            let tgt_label = render_diagram(&c.target_boundary, scope);
            println!("  [{}] {} : {}  ->  {}", i, c.rule_name, highlight, tgt_label);
        }
    }
}

/// Print a history summary to stdout for the REPL.
pub fn print_history(
    source: &Diagram,
    history_entries: &[(usize, &str)],  // (choice, rule_name)
    scope: &Complex,
) {
    println!("[0] {} (source)", render_diagram(source, scope));
    for (i, (choice, rule)) in history_entries.iter().enumerate() {
        println!("  step {} — {} (choice {})", i + 1, rule, choice);
    }
}
