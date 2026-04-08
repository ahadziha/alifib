//! Shared rendering helpers for the REPL.
//!
//! Pure string-building functions (`render_*`) are separated from display
//! functions (`print_*`) that accept a [`Display`] and produce output.
//! All output goes through `Display`; no `println!` appears here.

use crate::core::complex::Complex;
use crate::core::diagram::Diagram;
use crate::core::rewrite::CandidateRewrite;
use crate::output::render_diagram;
use super::display::Display;

// ── Pure string builders ──────────────────────────────────────────────────────

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

// ── Display functions ─────────────────────────────────────────────────────────

/// Display the current rewrite state.
///
/// Format:
/// ```text
/// (blank)
/// <current diagram>       ← cell line (plain)
/// (blank)
/// >> rewrites:
/// >>
/// >>   0  <highlight>  ->  <target>
/// >>     by <rule> : <src> -> <tgt>
/// >> ...
/// ```
/// If the target is reached, prints `>> Rewrite complete.` and the proof cell.
pub fn print_state(
    display: &Display,
    current: &Diagram,
    target: Option<&Diagram>,
    rewrites: &[CandidateRewrite],
    scope: &Complex,
    // Running proof for completion display (source label, target label, proof label).
    proof: Option<(&str, &str, &str)>,
) {
    display.blank();
    display.inspect(&render_diagram(current, scope));
    display.blank();

    // `proof` is Some only when target_reached() is true (steps taken + diagrams match).
    if let Some((src_label, tgt_label, proof_label)) = proof {
        display.meta("Rewrite complete.");
        display.blank();
        display.inspect("proof:");
        display.inspect(&format!("  {proof_label} : {src_label} -> {tgt_label}"));
        return;
    }

    if let Some(t) = target {
        display.inspect(&format!("target: {}", render_diagram(t, scope)));
    }

    if rewrites.is_empty() {
        display.meta("no rewrites available");
        return;
    }

    display.meta("rewrites:");
    for (i, c) in rewrites.iter().enumerate() {
        let highlight = render_match_highlight(current, scope, &c.image_positions);
        let rule_src = render_diagram(&c.source_boundary, scope);
        let rule_tgt = render_diagram(&c.target_boundary, scope);
        display.blank();
        display.inspect(&format!("  ({i}) {highlight}"));
        display.inspect(&format!("      by {} : {} -> {}", c.rule_name, rule_src, rule_tgt));
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
