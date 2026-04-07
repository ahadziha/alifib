//! Normalization of [`GlobalStore`] into the plain [`Store`] data type, and
//! low-level rendering helpers that convert internal diagram representations
//! to strings.
//!
//! The main entry point is [`GlobalStore::normalize`]. The public `render_*`
//! functions are also used by the hole-reporting machinery in
//! [`super::InterpretedFile::report_holes`].

use crate::aux::Tag;
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign},
    partial_map::PartialMap,
};
use crate::interpreter::{GlobalStore, HoleBd};
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

/// Collect all named generators of `mc` into a [`Module`], sorted by name.
fn normalize_module(store: &GlobalStore, path: &str, mc: &Complex) -> Module {
    let mut gen_entries: Vec<(&str, &Tag)> = mc
        .generators_iter()
        .map(|(name, tag, _)| (name.as_str(), tag))
        .collect();
    gen_entries.sort_by_key(|(name, _)| *name);

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

/// Render the top-level labels of `diagram` as a space-separated string of
/// generator names, resolved against `scope`.
pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    diagram_labels(diagram, scope).join(" ")
}

/// Render a partial boundary: each top-level label of `boundary` is mapped
/// through `map` and the result rendered against `scope`. Labels outside the
/// domain of `map` are rendered as `?`.
pub fn render_boundary_partial(boundary: &Diagram, map: &PartialMap, scope: &Complex) -> String {
    match boundary.labels_at(boundary.top_dim()) {
        Some(labels) if !labels.is_empty() => labels
            .iter()
            .map(|tag| match map.image(tag) {
                Ok(img) => render_diagram(img, scope),
                Err(_) => "?".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => "?".to_string(),
    }
}

/// Render a hole boundary for use in a diagnostic message.
pub(super) fn render_hole_bd(bd: &HoleBd) -> String {
    match bd {
        HoleBd::Unknown => "?".to_string(),
        HoleBd::Full(diagram, scope) => render_diagram(diagram, scope),
        HoleBd::Partial { boundary, map, scope } => render_boundary_partial(boundary, map, scope),
    }
}

/// Convert a generator's [`CellData`] into a [`Cell`], resolving boundary
/// labels against `complex`.
fn cell_from_data(name: &str, data: &CellData, complex: &Complex) -> Cell {
    match data {
        CellData::Zero => Cell { name: name.to_owned(), src: vec![], tgt: vec![] },
        CellData::Boundary { boundary_in, boundary_out } => Cell {
            name: name.to_owned(),
            src: diagram_labels(boundary_in, complex),
            tgt: diagram_labels(boundary_out, complex),
        },
    }
}

/// Convert a named diagram into a [`Cell`] by extracting its source and target
/// boundaries and resolving their labels against `complex`. Falls back to a
/// 0-dimensional cell if the boundary cannot be computed.
fn cell_from_diagram(name: &str, diag: &Diagram, complex: &Complex) -> Cell {
    let Some(k) = diag.top_dim().checked_sub(1) else {
        return Cell { name: name.to_owned(), src: vec![], tgt: vec![] };
    };
    let (Ok(src_diag), Ok(tgt_diag)) = (
        Diagram::boundary(Sign::Source, k, diag),
        Diagram::boundary(Sign::Target, k, diag),
    ) else {
        return Cell { name: name.to_owned(), src: vec![], tgt: vec![] };
    };
    Cell {
        name: name.to_owned(),
        src: diagram_labels(&src_diag, complex),
        tgt: diagram_labels(&tgt_diag, complex),
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

// ---- Display for GlobalStore ----

impl fmt::Display for GlobalStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalize())
    }
}
