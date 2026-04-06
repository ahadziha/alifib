use crate::aux::{GlobalId, ModuleId, Tag};
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::{CellData, Diagram, Sign};
use crate::core::partial_map::PartialMap;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

/// A global type cell together with its definition complex.
#[derive(Debug, Clone)]
pub struct TypeEntry {
    /// The boundary specification of the type cell itself
    /// (typically `Zero` for top-level types).
    pub data: CellData,
    /// The complex accumulated from the generators, diagrams, and maps
    /// declared inside this type's body.
    pub complex: Arc<Complex>,
}

/// A non-type global cell in the interpreter's persistent state.
#[derive(Debug, Clone)]
pub struct CellEntry {
    /// The boundary specification of this cell.
    pub data: CellData,
}

/// Global interpreter storage of persistent entities.
///
/// Invariants (checked in debug builds):
/// - every id listed in `cells_by_dim[d]` exists in `cells`
/// - every type/module id is unique in its table
#[derive(Debug, Clone, Default)]
pub struct GlobalStore {
    pub cells: HashMap<GlobalId, CellEntry>,
    pub cells_by_dim: HashMap<usize, Vec<GlobalId>>,
    pub types: HashMap<GlobalId, TypeEntry>,
    pub modules: HashMap<ModuleId, Arc<Complex>>,
}

impl GlobalStore {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn set_cell(&mut self, id: GlobalId, dim: usize, data: CellData) {
        self.cells_by_dim.entry(dim).or_default().push(id);
        self.cells.insert(id, CellEntry { data });
        self.assert_invariants();
    }

    pub fn set_type(&mut self, id: GlobalId, data: CellData, complex: Complex) {
        self.types.insert(
            id,
            TypeEntry {
                data,
                complex: Arc::new(complex),
            },
        );
        self.assert_invariants();
    }

    pub fn set_module(&mut self, id: ModuleId, complex: Complex) {
        self.modules.insert(id, Arc::new(complex));
        self.assert_invariants();
    }

    /// Mutate the Complex for a module in place via Arc::make_mut (copy-on-write).
    /// Silently does nothing if the module id is not found.
    pub fn modify_module(&mut self, id: &str, f: impl FnOnce(&mut Complex)) {
        if let Some(arc) = self.modules.get_mut(id) {
            f(Arc::make_mut(arc));
            self.assert_invariants();
        }
    }

    /// Mutate the Complex of a type entry in place via Arc::make_mut (copy-on-write).
    /// Silently does nothing if the type id is not found.
    pub fn modify_type_complex(&mut self, id: GlobalId, f: impl FnOnce(&mut Complex)) {
        if let Some(entry) = self.types.get_mut(&id) {
            f(Arc::make_mut(&mut entry.complex));
            self.assert_invariants();
        }
    }

    pub fn find_cell(&self, id: GlobalId) -> Option<&CellEntry> {
        self.cells.get(&id)
    }

    pub fn find_type(&self, id: GlobalId) -> Option<&TypeEntry> {
        self.types.get(&id)
    }

    pub fn find_module(&self, id: &str) -> Option<&Complex> {
        self.modules.get(id).map(|arc| &**arc)
    }

    /// Returns a cloned Arc so callers can cheaply share the module complex.
    pub fn find_module_arc(&self, id: &str) -> Option<Arc<Complex>> {
        self.modules.get(id).map(Arc::clone)
    }

    /// Look up cell data for a tag, checking global cells, types, then local cells.
    pub fn cell_data_for_tag(&self, complex: &Complex, tag: &Tag) -> Option<CellData> {
        match tag {
            Tag::Global(gid) => self
                .find_cell(*gid)
                .map(|e| e.data.clone())
                .or_else(|| self.find_type(*gid).map(|e| e.data.clone())),
            Tag::Local(name) => complex.find_local_cell(name).cloned(),
        }
    }

    fn assert_invariants(&self) {
        for ids in self.cells_by_dim.values() {
            for id in ids {
                debug_assert!(self.cells.contains_key(id));
            }
        }
    }
}

// ---- Rendering helpers ----

fn name_or_empty(s: &str) -> &str {
    if s.is_empty() { "<empty>" } else { s }
}

fn top_labels_rendered(diagram: &Diagram, f: impl Fn(&Tag) -> String) -> String {
    match diagram.labels_at(diagram.top_dim()) {
        Some(labels) if !labels.is_empty() => labels.iter().map(f).collect::<Vec<_>>().join(" "),
        _ => "?".to_string(),
    }
}

pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    top_labels_rendered(diagram, |tag| {
        scope
            .find_generator_by_tag(tag)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", tag))
    })
}

pub fn render_boundary_partial(boundary: &Diagram, map: &PartialMap, scope: &Complex) -> String {
    top_labels_rendered(boundary, |tag| match map.image(tag) {
        Ok(img) => render_diagram(img, scope),
        Err(_) => "?".to_string(),
    })
}

fn render_cell(name: &str, data: &CellData, complex: &Complex) -> String {
    let label = name_or_empty(name);
    match data {
        CellData::Zero => label.to_owned(),
        CellData::Boundary { boundary_in, boundary_out } => {
            let src = render_diagram(boundary_in, complex);
            let tgt = render_diagram(boundary_out, complex);
            format!("{} : {} -> {}", label, src, tgt)
        }
    }
}

fn render_named_diagram(name: &str, diag: &Diagram, complex: &Complex) -> String {
    let label = name_or_empty(name);
    let Some(k) = diag.top_dim().checked_sub(1) else {
        return label.to_owned();
    };
    let (Ok(src), Ok(tgt)) = (
        Diagram::boundary(Sign::Source, k, diag),
        Diagram::boundary(Sign::Target, k, diag),
    ) else {
        return label.to_owned();
    };
    format!(
        "{} : {} -> {}",
        label,
        render_diagram(&src, complex),
        render_diagram(&tgt, complex),
    )
}

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
        writeln!(
            f,
            "{} cells, {} types, {} modules",
            self.cells.len(),
            self.types.len(),
            self.modules.len(),
        )?;

        let mut module_entries: Vec<_> = self
            .modules
            .iter()
            .map(|(id, arc)| (id.as_str(), &**arc))
            .collect();
        module_entries.sort_by_key(|(id, _)| *id);

        for (module_id, module_complex) in &module_entries {
            write!(f, "\n* Module {}\n", module_id)?;

            let mut gen_entries: Vec<(&str, &Tag)> = module_complex
                .generators_iter()
                .map(|(name, tag, _)| (name.as_str(), tag))
                .collect();
            gen_entries.sort_by_key(|(name, _)| *name);

            for (i, (gen_name, gen_tag)) in gen_entries.iter().enumerate() {
                if i > 0 {
                    writeln!(f)?;
                }
                let type_label = name_or_empty(gen_name);

                let Tag::Global(gid) = gen_tag else {
                    writeln!(f, "Type {} (local)", type_label)?;
                    continue;
                };
                let Some(type_entry) = self.find_type(*gid) else {
                    writeln!(f, "Type {} (not found)", type_label)?;
                    continue;
                };

                writeln!(f, "Type {}", type_label)?;
                let tc = &type_entry.complex;

                // Cells grouped by dimension, with boundaries
                let mut dims: Vec<usize> = tc.generators_iter().map(|(_, _, dim)| dim).collect();
                dims.sort_unstable();
                dims.dedup();

                if dims.is_empty() {
                    writeln!(f, "  (no cells)")?;
                } else {
                    for dim in &dims {
                        let mut gens: Vec<(&str, &Tag)> = tc
                            .generators_iter()
                            .filter(|(_, _, d)| d == dim)
                            .map(|(name, tag, _)| (name.as_str(), tag))
                            .collect();
                        gens.sort_by_key(|(name, _)| *name);

                        let rendered: Vec<String> = gens
                            .iter()
                            .filter_map(|(name, tag)| {
                                let data = self.cell_data_for_tag(tc, tag)?;
                                Some(render_cell(name, &data, tc))
                            })
                            .collect();

                        if !rendered.is_empty() {
                            writeln!(f, "  [{}] {}", dim, rendered.join(", "))?;
                        }
                    }
                }

                // Diagrams
                let mut diag_entries: Vec<(&str, &Diagram)> =
                    tc.diagrams_iter().map(|(name, diag)| (name.as_str(), diag)).collect();
                if !diag_entries.is_empty() {
                    diag_entries.sort_by_key(|(name, _)| *name);
                    let diags: Vec<String> = diag_entries
                        .iter()
                        .map(|(name, diag)| render_named_diagram(name, diag, tc))
                        .collect();
                    writeln!(f, "  Diagrams: {}", diags.join(", "))?;
                }

                // Maps
                let mut map_entries: Vec<(&str, &MapDomain)> =
                    tc.maps_iter().map(|(name, _, domain)| (name.as_str(), domain)).collect();
                if !map_entries.is_empty() {
                    map_entries.sort_by_key(|(name, _)| *name);
                    let maps: Vec<String> = map_entries
                        .iter()
                        .map(|(name, domain)| {
                            let dom = render_domain(domain, module_complex);
                            format!("{} :: {}", name_or_empty(name), dom)
                        })
                        .collect();
                    writeln!(f, "  Maps: {}", maps.join(", "))?;
                }
            }
        }
        Ok(())
    }
}
