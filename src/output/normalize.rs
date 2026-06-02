//! Normalization of [`GlobalStore`] into the plain [`Store`] data type, and
//! low-level rendering helpers that convert internal diagram representations
//! to strings.
//!
//! The main entry point is [`GlobalStore::normalize`]. The public `render_*`
//! functions are also used to list a map's unfilled holes.

use crate::aux::{HoleId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign},
    map_hole::MapHole,
    paste_tree::PasteTree,
};
use crate::interpreter::{GlobalStore, InterpretedFile};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
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
    match diagram.tree(Sign::Input, n) {
        Some(tree) => render_paste_tree(tree, scope),
        None => diagram_labels(diagram, scope).join(" "),
    }
}

/// Render a diagram as a structured term expression, resolved against `scope`.
pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    render_diagram_tree(diagram, scope)
}

/// Convert a generator's [`CellData`] into a [`Cell`], resolving boundary
/// labels against `complex`.
fn cell_from_data(name: &str, data: &CellData, complex: &Complex) -> Cell {
    match data {
        CellData::Zero => Cell { name: name.to_owned(), input: String::new(), output: String::new() },
        CellData::Boundary { boundary_in, boundary_out } => Cell {
            name: name.to_owned(),
            input: render_diagram_tree(boundary_in, complex),
            output: render_diagram_tree(boundary_out, complex),
        },
    }
}

/// Convert a named diagram into a [`Cell`] by extracting its source and target
/// boundaries and resolving their labels against `complex`. Falls back to a
/// 0-dimensional cell if the boundary cannot be computed.
fn cell_from_diagram(name: &str, diag: &Diagram, complex: &Complex) -> Cell {
    let Some(k) = diag.top_dim().checked_sub(1) else {
        return Cell { name: name.to_owned(), input: String::new(), output: String::new() };
    };
    let (Ok(src_diag), Ok(tgt_diag)) = (
        Diagram::boundary(Sign::Input, k, diag),
        Diagram::boundary(Sign::Output, k, diag),
    ) else {
        return Cell { name: name.to_owned(), input: String::new(), output: String::new() };
    };
    Cell {
        name: name.to_owned(),
        input: render_diagram_tree(&src_diag, complex),
        output: render_diagram_tree(&tgt_diag, complex),
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

// ---- Map hole listing ----

/// Display names for metavariables: `?` + the source generator's name.
type HoleNames = HashMap<HoleId, String>;

/// Build display names for a map's holes, resolving each against its source
/// generator in `domain` (`?f`, `?b`), disambiguating collisions numerically.
fn hole_names(holes: &[MapHole], domain: &Complex) -> HoleNames {
    let mut names = HoleNames::new();
    let mut used: HashSet<String> = HashSet::new();
    for h in holes {
        let base = domain
            .find_generator_by_tag(&h.source)
            .filter(|n| !n.is_empty())
            .map(|n| format!("?{}", n))
            .unwrap_or_else(|| format!("{}", h.meta));
        let mut name = base.clone();
        let mut i = 1;
        while used.contains(&name) {
            name = format!("{}#{}", base, i);
            i += 1;
        }
        used.insert(name.clone());
        names.insert(h.meta, name);
    }
    names
}

/// Like [`render_paste_tree`], but renders a [`Tag::Hole`] leaf as its
/// metavariable name from `names`.
fn render_paste_tree_with_holes(tree: &PasteTree, scope: &Complex, names: &HoleNames) -> String {
    match tree {
        PasteTree::Leaf(Tag::Hole(id)) => {
            names.get(id).cloned().unwrap_or_else(|| format!("{}", id))
        }
        PasteTree::Leaf(tag) => scope
            .find_generator_by_tag(tag)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", tag)),
        PasteTree::Node { dim, .. } => {
            let k = *dim;
            let mut parts = Vec::new();
            collect_chain_with_holes(tree, k, scope, names, &mut parts);
            format!("({})", parts.join(&format!(" #{} ", k)))
        }
    }
}

fn collect_chain_with_holes(
    tree: &PasteTree,
    k: usize,
    scope: &Complex,
    names: &HoleNames,
    parts: &mut Vec<String>,
) {
    match tree {
        PasteTree::Node { dim, left, right } if *dim == k => {
            collect_chain_with_holes(left, k, scope, names, parts);
            collect_chain_with_holes(right, k, scope, names, parts);
        }
        _ => parts.push(render_paste_tree_with_holes(tree, scope, names)),
    }
}

/// Render one pending entry: a pure hole as `?name : <in> -> <out>` (or
/// `?name : (0-cell)`), a conditional as `name => <image>`; both with a
/// `(depends on …)` suffix listing the holes they reference.  Image leaves
/// resolve against `scope`, metavariables against `names`.
fn render_map_hole(hole: &MapHole, scope: &Complex, names: &HoleNames) -> String {
    let name = names.get(&hole.meta).cloned().unwrap_or_else(|| format!("{}", hole.meta));
    let mut s = match &hole.image {
        // Conditional assignment `x => a`, awaiting its boundary faces.
        Some(image) => {
            let src = name.strip_prefix('?').unwrap_or(&name);
            format!("{} => {}", src, render_diagram(image, scope))
        }
        // Pure hole: show the inferred boundary.
        None => {
            let body = match (&hole.boundary_in, &hole.boundary_out) {
                (Some(input), Some(output)) => format!(
                    "{} -> {}",
                    render_paste_tree_with_holes(input, scope, names),
                    render_paste_tree_with_holes(output, scope, names),
                ),
                _ => "(0-cell)".to_string(),
            };
            format!("{} : {}", name, body)
        }
    };
    if !hole.deps.is_empty() {
        let mut deps: Vec<String> = hole
            .deps
            .iter()
            .map(|d| names.get(d).cloned().unwrap_or_else(|| format!("{}", d)))
            .collect();
        deps.sort();
        s.push_str(&format!("   (depends on {})", deps.join(", ")));
    }
    s
}

/// List a map's unfilled holes, lowest dimension first so a hole's faces print
/// before the holes that depend on them.  `scope` is the complex the map maps
/// into (where image leaves resolve); `domain` is the complex it maps from
/// (where holes are named after their source generators).
pub fn list_map_holes(map_name: &str, holes: &[MapHole], scope: &Complex, domain: &Complex) -> String {
    let names = hole_names(holes, domain);
    let mut order: Vec<&MapHole> = holes.iter().collect();
    order.sort_by_key(|h| (h.dim, names.get(&h.meta).cloned().unwrap_or_default()));
    let lines: Vec<String> = order
        .iter()
        .map(|h| format!("  {}", render_map_hole(h, scope, &names)))
        .collect();
    format!("unfilled holes in `{}`:\n{}", map_name, lines.join("\n"))
}

/// Resolve a map's domain to the complex it maps from.
fn domain_complex(store: &GlobalStore, domain: &MapDomain) -> Option<Arc<Complex>> {
    match domain {
        MapDomain::Type(gid) => store.find_type(*gid).map(|e| Arc::clone(&e.complex)),
        MapDomain::Module(mid) => store.find_module_arc(mid),
    }
}

/// Print, to stderr, the unfilled holes of every named map in every type of the
/// loaded file.  Holes are informational: a map with holes is a legitimate
/// partial definition.
pub fn report_map_holes(file: &InterpretedFile) {
    let store = &file.state;
    for (_path, module_complex) in store.modules_iter() {
        for (type_name, tag, _) in module_complex.generators_iter() {
            let Tag::Global(gid) = tag else { continue; };
            let Some(type_entry) = store.find_type(*gid) else { continue; };
            let tc = type_entry.complex.as_ref();
            let mut map_names: Vec<&str> = tc.maps_iter().map(|(n, _, _)| n.as_str()).collect();
            map_names.sort_unstable();
            for map_name in map_names {
                let Some(holes) = tc.map_holes(map_name) else { continue; };
                if holes.is_empty() {
                    continue;
                }
                let (_, _, dom) = tc.maps_iter().find(|(n, _, _)| n.as_str() == map_name).unwrap();
                let domain = domain_complex(store, dom);
                let domain_ref = domain.as_deref().unwrap_or(tc);
                let listing = list_map_holes(map_name, holes, tc, domain_ref);
                eprintln!("In type `{}`, {}", type_name, listing);
            }
        }
    }
}

// ---- Display for GlobalStore ----

impl fmt::Display for GlobalStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalize())
    }
}
