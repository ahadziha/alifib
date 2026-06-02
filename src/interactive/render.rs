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
//
// Each renderer mirrors the web front-end's matching `render*` function — same
// layout, and the same colour roles via the `Display` palette: `dim` for field
// labels, `hi` for values, `sec` for section titles, `src`/`tgt` for the
// input/output sides of a rewrite, `ok` for success.

/// Render an active rewrite state: the step count, current/target diagrams, and
/// the list of available rewrites (each with its `in → out` rule and bracketed
/// match), or `no rewrites available`.
pub fn render_state(display: &Display, data: &ResponseData) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("{} {}", display.dim("step:"), display.hi(&data.step_count.to_string())));

    if let Some(cur) = &data.current {
        let label = if cur.label.is_empty() { "—".to_owned() } else { cur.label.clone() };
        out.push(format!("{} {}", display.dim("current:"), display.hi(&label)));
    }

    if let Some(t) = &data.target {
        let reached = if data.target_reached { format!(" {}", display.ok("✓ reached")) } else { String::new() };
        out.push(format!("{} {}{}", display.dim("target:"), display.hi(&t.label), reached));
    }

    if data.rewrites.is_empty() {
        out.push(display.dim("no rewrites available"));
    } else {
        out.push(String::new());
        out.push(display.sec("available rewrites:"));
        for r in &data.rewrites {
            let label = if r.family.is_empty() {
                format!("{}  {} → {}",
                    display.hi(&r.rule_name),
                    display.src(&r.input.label),
                    display.tgt(&r.output.label))
            } else {
                format!("{}  (parallel ×{})", display.hi(&r.rule_name), r.family.len())
            };
            out.push(format!("  [{}] {}", display.hi(&r.index.to_string()), label));
            if !r.match_display.is_empty() {
                out.push(format!("      {} {}",
                    display.dim("match:"), display.colorize_match_display(&r.match_display)));
            }
        }
    }

    out.join("\n")
}

/// Render a boundaryless 0-cell fill: the (synthetic) step count, a target-reached
/// banner once a cell is chosen, and the candidate 0-cells while unchosen.
pub fn render_zero_cell(display: &Display, zc: &ZeroCellInfo) -> String {
    let mut out: Vec<String> = Vec::new();
    out.push(format!("{} {}", display.dim("step:"), display.hi(if zc.chosen.is_some() { "1" } else { "0" })));
    if zc.target_reached {
        out.push(display.ok("✓ target reached"));
    }
    if zc.choices.is_empty() {
        out.push(display.dim("no rewrites available"));
    } else {
        out.push(String::new());
        out.push(display.sec("available rewrites:"));
        for c in &zc.choices {
            out.push(format!("  [{}] {}", display.hi(&c.index.to_string()), display.hi(&c.name)));
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
    format!("{}\n{}", display.dim(&summary), render_state(display, data))
}

/// Render the result of `store`: the confirmation and the appended `let` clause.
pub fn render_store(display: &Display, data: &ResponseData) -> String {
    match &data.stored {
        Some(s) => format!("{}\n  let {} = {}",
            display.ok(&format!("Stored '{}'", s.def_name)),
            display.hi(&s.def_name),
            display.dim(&s.expr)),
        None => "store failed".to_owned(),
    }
}

/// Render the module's open holes, numbered for `fill <n>`.
pub fn render_holes(display: &Display, holes: &[HoleInfo]) -> String {
    if holes.is_empty() {
        return display.dim("(no open holes)");
    }
    let mut out = vec![display.sec("open holes:")];
    for h in holes {
        out.push(format!("  [{}] {}",
            display.hi(&h.index.to_string()),
            display.dim(&format!("@{} {} :: {}", h.type_name, h.map_name, h.domain_name))));
        out.push(format!("      {}", display.hi(&h.boundary)));
    }
    out.join("\n")
}

/// Render the type summaries for `types`: one line each, `name (dim …, N
/// generators, …)`.  This keeps the CLI's own layout, shared verbatim with the
/// web, rather than a terse summary.
pub fn render_types(display: &Display, types: &[TypeSummaryInfo]) -> String {
    if types.is_empty() {
        return display.dim("  (No types found)");
    }
    types.iter().map(|t| {
        let mut parts = Vec::new();
        if let Some(d) = t.max_dim { parts.push(format!("dim {}", d)); }
        if t.generator_count > 0 {
            parts.push(format!("{} generator{}", t.generator_count, plural(t.generator_count)));
        }
        if t.diagram_count > 0 {
            parts.push(format!("{} diagram{}", t.diagram_count, plural(t.diagram_count)));
        }
        if t.map_count > 0 {
            parts.push(format!("{} map{}", t.map_count, plural(t.map_count)));
        }
        if parts.is_empty() {
            format!("  {}", display.hi(&t.name))
        } else {
            format!("  {} {}", display.hi(&t.name), display.dim(&format!("({})", parts.join(", "))))
        }
    }).collect::<Vec<_>>().join("\n")
}

/// Render the full detail of a type for `type <name>`: generators grouped by
/// dimension, named diagrams with their `= expr`, and maps (flagged `… with
/// holes` when open).  Shared verbatim with the web.
pub fn render_type_detail(display: &Display, d: &TypeDetailInfo) -> String {
    let mut out = vec![format!("{} {}", display.dim("Type"), display.hi(&d.name))];

    let mut last_dim: Option<usize> = None;
    for g in &d.generators {
        if last_dim != Some(g.dim) {
            out.push(format!("  {}", display.dim(&format!("[{}]", g.dim))));
            last_dim = Some(g.dim);
        }
        out.push(format!("    {}", boundary_line(display, &g.name, &g.input, &g.output)));
    }

    if !d.diagrams.is_empty() {
        out.push(format!("  {}", display.sec("Diagrams")));
        for g in &d.diagrams {
            out.push(format!("    {}", boundary_line(display, &g.name, &g.input, &g.output)));
            out.push(format!("      = {}", display.dim(&g.expr)));
        }
    }

    if !d.maps.is_empty() {
        out.push(format!("  {}", display.sec("Maps")));
        for m in &d.maps {
            let holes = if m.holes.is_empty() { String::new() } else { display.dim(" with holes") };
            out.push(format!("    {} :: {}{}", display.hi(&m.name), display.dim(&m.domain), holes));
            for hole in &m.holes {
                out.push(format!("      {}", display.src(hole)));
            }
        }
    }

    out.join("\n")
}

/// `name : in → out` for a cell with a boundary, or just `name` for a 0-cell.
fn boundary_line(
    display: &Display,
    name: &str,
    input: &Option<super::protocol::DiagramInfo>,
    output: &Option<super::protocol::DiagramInfo>,
) -> String {
    match (input, output) {
        (Some(i), Some(o)) =>
            format!("{} : {} → {}", display.hi(name), display.src(&i.label), display.tgt(&o.label)),
        _ => display.hi(name),
    }
}

fn plural(n: usize) -> &'static str { if n == 1 { "" } else { "s" } }

/// Render the rewrite rules at the current dimension for `rules`.
pub fn render_rules(display: &Display, rules: &[RuleInfo]) -> String {
    if rules.is_empty() {
        return display.dim("(no rules)");
    }
    rules.iter().map(|r|
        format!("  {}  {} → {}", display.hi(&r.name), display.dim(&r.input.label), display.dim(&r.output.label))
    ).collect::<Vec<_>>().join("\n")
}

/// Render the running proof for `proof`: the re-parseable expression `store`
/// would persist, headed by its boundary.  A zero-step session is the identity
/// proof on the initial diagram, so this is never empty for an engine session.
pub fn render_proof(display: &Display, data: &ResponseData) -> String {
    let Some(expr) = &data.proof_expr else {
        return display.dim("(no proof yet)");
    };
    let mut out = Vec::new();
    match &data.proof {
        Some(p) => out.push(format!("{} {} → {}",
            display.dim("proof :"), display.src(&p.input_label), display.tgt(&p.output_label))),
        None => out.push(display.dim("proof:")),
    }
    for line in expr.lines() {
        out.push(format!("  {}", display.hi(line)));
    }
    out.join("\n")
}

/// Render the move history for `history`.
pub fn render_history(display: &Display, data: &ResponseData) -> String {
    if data.history.is_empty() {
        return display.dim("(no moves yet)");
    }
    data.history.iter().map(|h| {
        let choice = match &h.choice {
            None => "[n/a]".to_owned(),
            Some(v) => format!("[choice {}]", v.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")),
        };
        format!("  {} {} {}", display.dim(&format!("{}.", h.step)), display.hi(&h.rule_name), display.dim(&choice))
    }).collect::<Vec<_>>().join("\n")
}

/// Render the cellular homology of a type for `homology <name>`, mirroring the
/// web's `H_d = …` / `χ = …` layout.
pub fn render_homology(display: &Display, h: &Homology) -> String {
    if h.groups.is_empty() {
        return display.dim("(no generators)");
    }
    let mut out: Vec<String> = h.groups.iter()
        .map(|(dim, group)| format!("  {} = {}", display.dim(&format!("H_{}", dim)), display.hi(&format!("{}", group))))
        .collect();
    out.push(format!("  {} = {}", display.dim("χ"), display.hi(&h.euler_characteristic.to_string())));
    out.join("\n")
}
