//! Textual rendering of a [`ResponseData`] for the CLI REPL.
//!
//! Every function here is a **pure string builder**: it takes the shared
//! [`ResponseData`] (the same payload the web and daemon receive) and returns a
//! coloured, multi-line block, which the adapter prints via [`Display`].  The
//! layout deliberately mirrors the web front-end's `render*` functions in
//! `web/frontend/src/app.js`, so the CLI transcript matches the web's for the
//! same session — only the colour medium differs (ANSI here, CSS there).
//!
//! Colour roles map onto the [`Display`] palette: expressions and values use the
//! code colour, choice indices the accent colour, success the ok colour; field
//! labels and section titles are left in the default foreground.

use crate::analysis::homology::Homology;
use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::paste_tree::PasteTree;
use super::display::Display;
use super::protocol::{
    HoleInfo, ResponseData, RuleInfo, TypeDetailInfo, TypeSummaryInfo, ZeroCellInfo,
};

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

// ── ResponseData renderers (web-style) ──────────────────────────────────────────

/// Render an active rewrite state: the step count, current/target diagrams, and
/// the list of available rewrites (each with its `in → out` rule and bracketed
/// match), or `no rewrites available`.
pub fn render_state(display: &Display, data: &ResponseData) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("step: {}", display.code(&data.step_count.to_string())));

    if let Some(cur) = &data.current {
        let label = if cur.label.is_empty() { "—".to_owned() } else { cur.label.clone() };
        out.push(format!("current: {}", display.code(&label)));
    }

    if let Some(t) = &data.target {
        let reached = if data.target_reached { format!(" {}", display.ok("✓ reached")) } else { String::new() };
        out.push(format!("target: {}{}", display.code(&t.label), reached));
    }

    if data.rewrites.is_empty() {
        out.push("no rewrites available".to_owned());
    } else {
        out.push(String::new());
        out.push("available rewrites:".to_owned());
        for r in &data.rewrites {
            let label = if r.family.is_empty() {
                format!("{}  {} → {}",
                    display.code(&r.rule_name),
                    display.code(&r.input.label),
                    display.code(&r.output.label))
            } else {
                format!("{}  (parallel ×{})", display.code(&r.rule_name), r.family.len())
            };
            out.push(format!("  [{}] {}", display.acc(&r.index.to_string()), label));
            if !r.match_display.is_empty() {
                out.push(format!("      match: {}", display.colorize_match_display(&r.match_display)));
            }
        }
    }

    out.join("\n")
}

/// Render a boundaryless 0-cell fill: the (synthetic) step count, a target-reached
/// banner once a cell is chosen, and the candidate 0-cells while unchosen.
pub fn render_zero_cell(display: &Display, zc: &ZeroCellInfo) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("step: {}", display.code(if zc.chosen.is_some() { "1" } else { "0" })));
    if zc.target_reached {
        out.push(display.ok("✓ target reached"));
    }
    if zc.choices.is_empty() {
        out.push("no rewrites available".to_owned());
    } else {
        out.push(String::new());
        out.push("available rewrites:".to_owned());
        for c in &zc.choices {
            out.push(format!("  [{}] {}", display.acc(&c.index.to_string()), display.code(&c.name)));
        }
    }
    out.join("\n")
}

/// Render an `auto`/`random` run: the summary line then the resulting state.
pub fn render_auto(display: &Display, data: &ResponseData) -> String {
    let (applied, reason) = match &data.auto {
        Some(a) => (a.applied, if a.stop_reason.is_empty() { String::new() } else { format!(" ({})", a.stop_reason) }),
        None => (0, String::new()),
    };
    let summary = format!("applied {} step{}{}", applied, if applied == 1 { "" } else { "s" }, reason);
    format!("{}\n{}", summary, render_state(display, data))
}

/// Render the result of `store`: the confirmation and the appended `let` clause.
pub fn render_store(display: &Display, data: &ResponseData) -> String {
    match &data.stored {
        Some(s) => format!("{}\n  let {} = {}",
            display.ok(&format!("Stored '{}'", s.def_name)),
            display.code(&s.def_name),
            display.code(&s.expr)),
        None => "store failed".to_owned(),
    }
}

/// Render the module's open holes, numbered for `fill <n>`.
pub fn render_holes(display: &Display, holes: &[HoleInfo]) -> String {
    if holes.is_empty() {
        return "(no open holes)".to_owned();
    }
    let mut out = vec!["open holes:".to_owned()];
    for h in holes {
        out.push(format!("  [{}] @{} {} :: {}",
            display.acc(&h.index.to_string()), h.type_name, h.map_name, h.domain_name));
        out.push(format!("      {}", display.code(&h.boundary)));
    }
    out.join("\n")
}

/// Render the type summaries for `types`: one line each.
pub fn render_types(display: &Display, types: &[TypeSummaryInfo]) -> String {
    if types.is_empty() {
        return "(no types)".to_owned();
    }
    types.iter().map(|t| {
        let dim = t.max_dim.map(|d| d.to_string()).unwrap_or_else(|| "?".to_owned());
        format!("{} — {} gen, {} diag, dim {}",
            display.code(&t.name), t.generator_count, t.diagram_count, dim)
    }).collect::<Vec<_>>().join("\n")
}

/// Render the full detail of a type for `type <name>`: generators, diagrams, maps.
pub fn render_type_detail(display: &Display, d: &TypeDetailInfo) -> String {
    let mut out = vec![display.code(&d.name)];
    if !d.generators.is_empty() {
        out.push("generators:".to_owned());
        for g in &d.generators {
            let bounds = match (&g.input, &g.output) {
                (Some(i), Some(o)) => format!("  {} → {}", i.label, o.label),
                _ => String::new(),
            };
            out.push(format!("  {} (dim {}){}", display.code(&g.name), g.dim, bounds));
        }
    }
    if !d.diagrams.is_empty() {
        out.push("diagrams:".to_owned());
        for g in &d.diagrams {
            let header = match (&g.input, &g.output) {
                (Some(i), Some(o)) => format!("{} : {} → {}", display.code(&g.name), i.label, o.label),
                _ => display.code(&g.name),
            };
            out.push(format!("  {}  = {}", header, g.expr));
        }
    }
    if !d.maps.is_empty() {
        out.push("maps:".to_owned());
        for m in &d.maps {
            out.push(format!("  {} :: {}", display.code(&m.name), m.domain));
        }
    }
    out.join("\n")
}

/// Render the rewrite rules at the current dimension for `rules`.
pub fn render_rules(display: &Display, rules: &[RuleInfo]) -> String {
    if rules.is_empty() {
        return "(no rules)".to_owned();
    }
    rules.iter().map(|r|
        format!("  {}  {} → {}", display.code(&r.name), r.input.label, r.output.label)
    ).collect::<Vec<_>>().join("\n")
}

/// Render the running proof for `proof`: the re-parseable expression `store`
/// would persist, headed by its boundary.  `(no proof yet)` before any step.
pub fn render_proof(display: &Display, data: &ResponseData) -> String {
    let Some(expr) = &data.proof_expr else {
        return "(no proof yet)".to_owned();
    };
    let mut out = Vec::new();
    match &data.proof {
        Some(p) => out.push(format!("proof : {} → {}",
            display.code(&p.input_label), display.code(&p.output_label))),
        None => out.push("proof:".to_owned()),
    }
    for line in expr.lines() {
        out.push(format!("  {}", display.code(line)));
    }
    out.join("\n")
}

/// Render the move history for `history`.
pub fn render_history(display: &Display, data: &ResponseData) -> String {
    if data.history.is_empty() {
        return "(no moves yet)".to_owned();
    }
    data.history.iter().map(|h| {
        let choice = match &h.choice {
            None => "[n/a]".to_owned(),
            Some(v) => format!("[choice {}]", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")),
        };
        format!("  {}. {} {}", h.step, display.code(&h.rule_name), choice)
    }).collect::<Vec<_>>().join("\n")
}

/// Render the cellular homology of a type for `homology <name>`, mirroring the
/// web's `H_d = …` / `χ = …` layout.
pub fn render_homology(display: &Display, h: &Homology) -> String {
    if h.groups.is_empty() {
        return "(no generators)".to_owned();
    }
    let mut out: Vec<String> = h.groups.iter()
        .map(|(dim, group)| format!("  H_{} = {}", dim, display.code(&format!("{}", group))))
        .collect();
    out.push(format!("  χ = {}", display.code(&h.euler_characteristic.to_string())));
    out.join("\n")
}
