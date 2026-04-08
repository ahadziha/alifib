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
/// Handles any set of positions, including non-contiguous ones.
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

    let mut positions: Vec<usize> = match_positions.to_vec();
    positions.sort_unstable();

    let mut out = String::new();
    let mut i = 0;
    while i < labels.len() {
        if !out.is_empty() { out.push(' '); }
        if positions.binary_search(&i).is_ok() {
            // Consume all contiguous matched positions as one bracketed group.
            out.push('[');
            let mut first = true;
            while i < labels.len() && positions.binary_search(&i).is_ok() {
                if !first { out.push(' '); }
                out.push_str(&labels[i]);
                first = false;
                i += 1;
            }
            out.push(']');
        } else {
            out.push_str(&labels[i]);
            i += 1;
        }
    }
    out
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
