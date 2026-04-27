//! Normalization of [`GlobalStore`] into the plain [`Store`] data type, and
//! low-level rendering helpers that convert internal diagram representations
//! to strings.
//!
//! The main entry point is [`GlobalStore::normalize`]. The public `render_*`
//! functions are also used by the hole-reporting machinery in
//! [`super::InterpretedFile::report_holes`].

use crate::aux::{self, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, PasteTree, Sign},
    partial_map::PartialMap,
};
use crate::interpreter::{GlobalStore, InterpretedFile};
use crate::interpreter::inference::{BdSlot, SolvedBd, SolvedHole};
use crate::interpreter::PartialHint;
use std::fmt;
use super::types::{Cell, Dim, Map, Module, Store, Type};

// ---- GlobalStore::normalize ----

impl GlobalStore {
    /// Convert this store into a [`Store`]: a plain, name-keyed tree with
    /// no opaque IDs, suitable for `assert_eq!` in tests and as the renderer's input.
    ///
    /// Panics if an interpreter invariant is violated (e.g. a module generator
    /// has no corresponding type entry). Those are interpreter bugs, not caller errors.
    pub fn normalize(&self) -> Store {
        let modules = self
            .modules_iter()
            .map(|(path, mc)| normalize_module(self, path, mc))
            .collect();
        Store {
            cells_count: self.cells_count(),
            types_count: self.types_count(),
            modules,
        }
    }
}

/// Collect all named generators of `mc` into a [`Module`], in insertion order.
fn normalize_module(store: &GlobalStore, path: &str, mc: &Complex) -> Module {
    let mut gen_entries: Vec<(&str, &Tag)> = mc
        .generators_iter()
        .map(|(name, tag, _)| (name.as_str(), tag))
        .collect();
    gen_entries.sort_by_key(|(name, _)| mc.generator_order(name));

    let types = gen_entries
        .iter()
        .map(|(gen_name, gen_tag)| {
            let Tag::Global(gid) = gen_tag else {
                panic!(
                    "interpreter invariant violated: module generator '{}' has a local tag",
                    gen_name
                );
            };
            let type_entry = store
                .find_type(*gid)
                .expect("interpreter invariant violated: module generator has no type entry");
            normalize_type(store, gen_name, mc, &type_entry.complex)
        })
        .collect();

    Module { path: path.to_owned(), types }
}

/// Build a [`Type`] from a type complex `tc`, grouping its generators by
/// dimension and resolving diagrams and maps against `module_complex`.
fn normalize_type(
    store: &GlobalStore,
    name: &str,
    module_complex: &Complex,
    tc: &Complex,
) -> Type {
    let mut dim_set: Vec<usize> = tc.generators_iter().map(|(_, _, d)| d).collect();
    dim_set.sort_unstable();
    dim_set.dedup();

    let dims = dim_set
        .iter()
        .map(|&dim| {
            let mut gens: Vec<(&str, &Tag)> = tc
                .generators_iter()
                .filter(|(_, _, d)| *d == dim)
                .map(|(n, tag, _)| (n.as_str(), tag))
                .collect();
            gens.sort_by_key(|(n, _)| *n);
            let cells = gens
                .iter()
                .map(|(n, tag)| {
                    let data = store
                        .cell_data_for_tag(tc, tag)
                        .expect("interpreter invariant violated: generator has no cell data");
                    cell_from_data(n, &data, tc)
                })
                .collect();
            Dim { dim, cells }
        })
        .collect();

    let mut diag_entries: Vec<(&str, &Diagram)> =
        tc.diagrams_iter().map(|(n, d)| (n.as_str(), d)).collect();
    diag_entries.sort_by_key(|(n, _)| *n);
    let diagrams = diag_entries
        .iter()
        .map(|(n, d)| cell_from_diagram(n, d, tc))
        .collect();

    let mut map_entries: Vec<(&str, &MapDomain)> =
        tc.maps_iter().map(|(n, _, dom)| (n.as_str(), dom)).collect();
    map_entries.sort_by_key(|(n, _)| *n);
    let maps = map_entries
        .iter()
        .map(|(n, dom)| Map { name: n.to_string(), domain: render_domain(dom, module_complex) })
        .collect();

    Type { name: name.to_owned(), dims, diagrams, maps }
}

// ---- Rendering helpers ----

/// Return `"<empty>"` for the empty string, otherwise the string itself.
fn name_or_empty(s: &str) -> &str {
    if s.is_empty() { "<empty>" } else { s }
}

/// Render a [`PasteTree`] as a structured term expression.
///
/// - `Leaf(tag)` → generator name resolved against `scope`
/// - `Node { dim, left, right }` → chains at the same dimension are flattened:
///   `paste(k, paste(k, a, b), c)` renders as `(a #k b #k c)` instead of `((a #k b) #k c)`
fn render_paste_tree(tree: &PasteTree, scope: &Complex) -> String {
    match tree {
        PasteTree::Leaf(tag) => scope
            .find_generator_by_tag(tag)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", tag)),
        PasteTree::Node { dim, .. } => {
            let k = *dim;
            let mut parts = Vec::new();
            collect_chain(tree, k, scope, &mut parts);
            format!("({})", parts.join(&format!(" #{} ", k)))
        }
    }
}

/// Collect all elements of a left- or right-associated chain at dimension `k`.
fn collect_chain(tree: &PasteTree, k: usize, scope: &Complex, parts: &mut Vec<String>) {
    match tree {
        PasteTree::Node { dim, left, right } if *dim == k => {
            collect_chain(left, k, scope, parts);
            collect_chain(right, k, scope, parts);
        }
        _ => parts.push(render_paste_tree(tree, scope)),
    }
}

/// Resolve the top-level labels of `diagram` to generator names in `scope`.
fn diagram_labels(diagram: &Diagram, scope: &Complex) -> Vec<String> {
    match diagram.labels_at(diagram.top_dim()) {
        Some(labels) if !labels.is_empty() => labels
            .iter()
            .map(|tag| {
                scope
                    .find_generator_by_tag(tag)
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag))
            })
            .collect(),
        _ => vec!["?".to_string()],
    }
}

/// Render a diagram as a structured term expression from its paste tree.
///
/// Uses the source paste tree at the top dimension. Falls back to flat
/// label rendering if no paste history is available.
fn render_diagram_tree(diagram: &Diagram, scope: &Complex) -> String {
    let n = diagram.top_dim();
    match diagram.tree(Sign::Source, n) {
        Some(tree) => render_paste_tree(tree, scope),
        None => diagram_labels(diagram, scope).join(" "),
    }
}

/// Render a diagram as a structured term expression, resolved against `scope`.
pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    render_diagram_tree(diagram, scope)
}

/// Render a partial boundary: uses the paste tree structure of `boundary`,
/// mapping each leaf through `map`. Leaves outside the domain of `map` are
/// rendered as `_`. Chains at the same dimension are flattened.
pub fn render_boundary_partial(boundary: &Diagram, map: &PartialMap, scope: &Complex) -> String {
    let n = boundary.top_dim();
    match boundary.tree(Sign::Source, n) {
        Some(tree) => render_tree_partial(tree, map, scope),
        None => "_".to_string(),
    }
}

fn render_tree_partial(tree: &PasteTree, map: &PartialMap, scope: &Complex) -> String {
    match tree {
        PasteTree::Leaf(tag) => {
            match map.image(tag) {
                Ok(img) => render_diagram(img, scope),
                Err(_) => "_".to_string(),
            }
        }
        PasteTree::Node { dim, .. } => {
            let k = *dim;
            let mut parts = Vec::new();
            collect_chain_partial(tree, k, map, scope, &mut parts);
            format!("({})", parts.join(&format!(" #{} ", k)))
        }
    }
}

fn collect_chain_partial(tree: &PasteTree, k: usize, map: &PartialMap, scope: &Complex, parts: &mut Vec<String>) {
    match tree {
        PasteTree::Node { dim, left, right } if *dim == k => {
            collect_chain_partial(left, k, map, scope, parts);
            collect_chain_partial(right, k, map, scope, parts);
        }
        _ => parts.push(render_tree_partial(tree, map, scope)),
    }
}

/// Convert a generator's [`CellData`] into a [`Cell`], resolving boundary
/// labels against `complex`.
fn cell_from_data(name: &str, data: &CellData, complex: &Complex) -> Cell {
    match data {
        CellData::Zero => Cell { name: name.to_owned(), src: String::new(), tgt: String::new() },
        CellData::Boundary { boundary_in, boundary_out } => Cell {
            name: name.to_owned(),
            src: render_diagram_tree(boundary_in, complex),
            tgt: render_diagram_tree(boundary_out, complex),
        },
    }
}

/// Convert a named diagram into a [`Cell`] by extracting its source and target
/// boundaries and resolving their labels against `complex`. Falls back to a
/// 0-dimensional cell if the boundary cannot be computed.
fn cell_from_diagram(name: &str, diag: &Diagram, complex: &Complex) -> Cell {
    let Some(k) = diag.top_dim().checked_sub(1) else {
        return Cell { name: name.to_owned(), src: String::new(), tgt: String::new() };
    };
    let (Ok(src_diag), Ok(tgt_diag)) = (
        Diagram::boundary(Sign::Source, k, diag),
        Diagram::boundary(Sign::Target, k, diag),
    ) else {
        return Cell { name: name.to_owned(), src: String::new(), tgt: String::new() };
    };
    Cell {
        name: name.to_owned(),
        src: render_diagram_tree(&src_diag, complex),
        tgt: render_diagram_tree(&tgt_diag, complex),
    }
}

/// Resolve a [`MapDomain`] to the name of the target type or module.
fn render_domain(domain: &MapDomain, module_complex: &Complex) -> String {
    match domain {
        MapDomain::Type(gid) => {
            let tag = Tag::Global(*gid);
            module_complex
                .find_generator_by_tag(&tag)
                .map(|n| name_or_empty(n).to_owned())
                .unwrap_or_else(|| format!("{}", gid))
        }
        MapDomain::Module(mid) => mid.clone(),
    }
}

// ---- Hole reporting ----

/// Unicode superscript for a boundary sign: ⁻ for Source, ⁺ for Target.
fn sign_superscript(sign: Sign) -> &'static str {
    match sign { Sign::Source => "⁻", Sign::Target => "⁺" }
}

/// Format a boundary slot as `∂⁻ₖ` or `∂⁺ₖ`.
fn format_slot(slot: &BdSlot) -> String {
    format!("∂{}{}", sign_superscript(slot.sign), aux::dim_subscript(slot.dim))
}

/// Render a fully resolved boundary slot.
fn render_solved_bd(bd: &SolvedBd) -> String {
    render_diagram(&bd.diagram, &bd.scope)
}

/// Render a boundary slot, falling back to the partial hint if the slot is not resolved.
fn render_slot_with_hint(
    slot: BdSlot,
    boundaries: &std::collections::BTreeMap<BdSlot, SolvedBd>,
    partial_hints: &[PartialHint],
) -> String {
    if let Some(bd) = boundaries.get(&slot) {
        return render_solved_bd(bd);
    }
    // Fall back to partial hint for this slot if available.
    if let Some(hint) = partial_hints.iter().find(|h| h.slot == slot) {
        return render_boundary_partial(&hint.boundary, &hint.map, &hint.scope);
    }
    "_".to_string()
}

/// Render a solved hole as a diagnostic message.
///
/// Reports the principal boundary `src -> tgt` when the hole's dimension and
/// principal slots are available (either fully resolved or via partial hints);
/// falls back to listing all available slots otherwise.
pub fn render_solved_hole(hole: &SolvedHole) -> String {
    // If an Eq constraint determined the exact value, report it.
    if let Some((ref diag, ref scope)) = hole.value {
        let mut msg = format!("= {}", render_diagram(diag, scope));
        if !hole.inconsistencies.is_empty() {
            msg.push_str(&format!(" [inconsistencies: {}]", hole.inconsistencies.join("; ")));
        }
        return msg;
    }

    // A slot is "available" if it has a resolved boundary or a partial hint.
    let has_slot = |slot: BdSlot| -> bool {
        hole.boundaries.contains_key(&slot)
            || hole.partial_hints.iter().any(|h| h.slot == slot)
    };

    // Collect all dims that have both Source and Target available (either resolved or hinted).
    let paired_max_k = || -> Option<usize> {
        let mut dims: std::collections::BTreeSet<usize> = hole.boundaries.keys()
            .map(|s| s.dim)
            .collect();
        for h in &hole.partial_hints {
            dims.insert(h.slot.dim);
        }
        dims.into_iter()
            .filter(|&k| {
                has_slot(BdSlot { sign: Sign::Source, dim: k })
                    && has_slot(BdSlot { sign: Sign::Target, dim: k })
            })
            .max()
    };

    // Prefer the principal slot (dim n-1) when the dimension is known.
    let best_k: Option<usize> = if let Some(n) = hole.dim {
        if n > 0 {
            let k = n - 1;
            let has_principal = has_slot(BdSlot { sign: Sign::Source, dim: k })
                && has_slot(BdSlot { sign: Sign::Target, dim: k });
            if has_principal { Some(k) } else { paired_max_k() }
        } else {
            None
        }
    } else {
        paired_max_k()
    };

    if let Some(k) = best_k {
        let src_slot = BdSlot { sign: Sign::Source, dim: k };
        let tgt_slot = BdSlot { sign: Sign::Target, dim: k };
        let src = render_slot_with_hint(src_slot, &hole.boundaries, &hole.partial_hints);
        let tgt = render_slot_with_hint(tgt_slot, &hole.boundaries, &hole.partial_hints);
        let mut msg = format!("{} -> {}", src, tgt);
        if !hole.inconsistencies.is_empty() {
            msg.push_str(&format!(" [inconsistencies: {}]", hole.inconsistencies.join("; ")));
        }
        return msg;
    }

    // Fall back: list all available slots grouped by dimension, highest first.
    let has_anything = !hole.boundaries.is_empty() || !hole.partial_hints.is_empty();
    if !has_anything {
        let dim_info = hole.dim.map(|d| format!(" (dim {})", d)).unwrap_or_default();
        if hole.inconsistencies.is_empty() {
            return format!("unknown boundary{}", dim_info);
        } else {
            return format!("inconsistent constraints{}: {}", dim_info, hole.inconsistencies.join("; "));
        }
    }

    // Collect distinct dims from both resolved boundaries and partial hints.
    let mut dims: Vec<usize> = {
        let mut d: std::collections::BTreeSet<usize> = hole.boundaries.keys().map(|s| s.dim).collect();
        for h in &hole.partial_hints {
            d.insert(h.slot.dim);
        }
        d.into_iter().collect()
    };
    dims.sort_unstable();

    let parts: Vec<String> = dims.iter().rev().flat_map(|&k| {
        [Sign::Source, Sign::Target].iter().filter_map(move |&sign| {
            let slot = BdSlot { sign, dim: k };
            if has_slot(slot) {
                Some(format!("{} = {}", format_slot(&slot),
                    render_slot_with_hint(slot, &hole.boundaries, &hole.partial_hints)))
            } else {
                None
            }
        })
    }).collect();

    let mut msg = parts.join(", ");
    if !hole.inconsistencies.is_empty() {
        msg.push_str(&format!(" [inconsistencies: {}]", hole.inconsistencies.join("; ")));
    }
    msg
}

/// Print a diagnostic for each unsolved hole using constraint-solver output.
pub fn report_solved_holes(file: &InterpretedFile) {
    for hole in &file.solved_holes {
        let message = render_solved_hole(hole);
        crate::language::error::report_hole(hole.span, &message, &file.source, &file.path);
    }
}

// ---- Display for GlobalStore ----

impl fmt::Display for GlobalStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalize())
    }
}

// ---- Tests for render_solved_hole ----

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::inference::{HoleId, SolvedHole};
    use crate::language::ast::Span;
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn dummy_span() -> Span { Span { start: 0, end: 0 } }

    fn empty_hole() -> SolvedHole {
        SolvedHole {
            id: HoleId::fresh(),
            span: dummy_span(),
            dim: None,
            boundaries: BTreeMap::new(),
            value: None,
            inconsistencies: vec![],
            partial_hints: vec![],
        }
    }

    #[test]
    fn render_unknown_boundary() {
        let hole = empty_hole();
        assert_eq!(render_solved_hole(&hole), "unknown boundary");
    }

    #[test]
    fn render_dim_only() {
        // When only dim is known (no boundary slots), dim should appear in output.
        let mut hole = empty_hole();
        hole.dim = Some(2);
        let msg = render_solved_hole(&hole);
        assert!(msg.contains("dim 2"), "expected dim in output, got: {msg}");
    }

    #[test]
    fn render_value_only() {
        // A Value-resolved hole shows "= <diagram>".
        use crate::core::diagram::{Diagram, CellData};
        use crate::aux::Tag;
        let cell = Diagram::cell(Tag::Local("x".into()), &CellData::Zero).unwrap();
        let scope = Arc::new(Complex::empty());
        let mut hole = empty_hole();
        hole.value = Some((cell, scope));
        let msg = render_solved_hole(&hole);
        assert!(msg.starts_with("= "), "expected '= ' prefix, got: {msg}");
        assert!(!msg.contains("inconsistencies"), "no inconsistencies expected");
    }

    #[test]
    fn render_value_with_inconsistencies() {
        // Bug E: a Value-resolved hole with inconsistencies should show both.
        use crate::core::diagram::{Diagram, CellData};
        use crate::aux::Tag;
        let cell = Diagram::cell(Tag::Local("x".into()), &CellData::Zero).unwrap();
        let scope = Arc::new(Complex::empty());
        let mut hole = empty_hole();
        hole.value = Some((cell, scope));
        hole.inconsistencies.push("some conflict".to_string());
        let msg = render_solved_hole(&hole);
        assert!(msg.starts_with("= "), "expected value prefix");
        assert!(msg.contains("inconsistencies"), "inconsistencies should appear even when value is set");
    }

    #[test]
    fn render_inconsistencies_no_boundaries() {
        // Inconsistencies with no boundaries: shows inconsistent constraints message.
        let mut hole = empty_hole();
        hole.inconsistencies.push("conflict A".to_string());
        let msg = render_solved_hole(&hole);
        assert!(msg.contains("inconsistent constraints"), "expected inconsistent constraints message");
        assert!(msg.contains("conflict A"));
    }

    #[test]
    fn render_dim_with_inconsistencies() {
        // Dim known + inconsistencies + no boundaries: shows dim in inconsistency message.
        let mut hole = empty_hole();
        hole.dim = Some(1);
        hole.inconsistencies.push("conflict B".to_string());
        let msg = render_solved_hole(&hole);
        assert!(msg.contains("dim 1"), "dim should appear in output: {msg}");
        assert!(msg.contains("conflict B"));
    }
}
