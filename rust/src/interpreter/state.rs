use std::collections::HashMap;
use std::sync::Arc;
use crate::aux::{GlobalId, ModuleId, Tag};
use crate::core::complex::{Complex, MapDomain};
use crate::core::diagram::CellData;

#[derive(Debug, Clone)]
pub struct TypeEntry {
    pub data: CellData,
    pub complex: Arc<Complex>,
}

#[derive(Debug, Clone)]
pub struct CellEntry {
    pub data: CellData,
}

/// The global interpreter state.
#[derive(Debug, Clone, Default)]
pub struct State {
    pub cells: HashMap<GlobalId, CellEntry>,
    pub cells_by_dim: HashMap<usize, Vec<GlobalId>>,
    pub types: HashMap<GlobalId, TypeEntry>,
    pub modules: HashMap<ModuleId, Arc<Complex>>,
}

impl State {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn set_cell(&mut self, id: GlobalId, dim: usize, data: CellData) {
        self.cells_by_dim.entry(dim).or_default().push(id);
        self.cells.insert(id, CellEntry { data });
    }

    pub fn set_type(&mut self, id: GlobalId, data: CellData, complex: Complex) {
        self.types.insert(id, TypeEntry { data, complex: Arc::new(complex) });
    }

    pub fn update_type_complex(&mut self, id: GlobalId, complex: Complex) {
        if let Some(entry) = self.types.get_mut(&id) {
            entry.complex = Arc::new(complex);
        }
    }

    pub fn set_module(&mut self, id: ModuleId, complex: Complex) {
        self.modules.insert(id, Arc::new(complex));
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

    /// Pretty-print the state in a human-readable format.
    pub fn display(&self) -> String {
        let mut out = String::new();
        let cells_count = self.cells.len();
        let types_count = self.types.len();
        let modules_count = self.modules.len();
        out.push_str(&format!("{} cells, {} types, {} modules\n\n", cells_count, types_count, modules_count));

        let empty_or = |s: &str| if s.is_empty() { "<empty>".to_owned() } else { s.to_owned() };
        let render_list = |items: Vec<String>| {
            if items.is_empty() { "(none)".to_owned() } else { items.join(", ") }
        };

        let render_cells_by_dim = |cplx: &Complex| -> String {
            let names = cplx.generator_names();
            if names.is_empty() { return "(none)".to_owned(); }
            let mut dims: Vec<usize> = names.iter()
                .filter_map(|n| cplx.generator_dim(n))
                .collect();
            dims.sort_unstable();
            dims.dedup();
            dims.iter().map(|&dim| {
                let mut gens = cplx.generators_in_dim(dim);
                gens.sort();
                let rendered = gens.iter().map(|n| empty_or(n)).collect::<Vec<_>>().join(", ");
                if rendered.is_empty() { format!("[{}]", dim) }
                else { format!("[{}] {}", dim, rendered) }
            }).collect::<Vec<_>>().join(", ")
        };

        let mut module_entries: Vec<(&ModuleId, &Complex)> =
            self.modules.iter().map(|(id, arc)| (id, &**arc)).collect();
        module_entries.sort_by_key(|(id, _)| id.as_str());

        let mut entries_str: Vec<String> = Vec::new();
        for (module_id, module_complex) in module_entries {
            let mut module_str = format!("* Module {}\n", module_id);
            let generator_names = module_complex.generator_names();

            let string_of_domain = |domain: &MapDomain| -> String {
                match domain {
                    MapDomain::Type(gid) => {
                        let tag = Tag::Global(*gid);
                        match module_complex.find_generator_by_tag(&tag) {
                            Some(name) => empty_or(name),
                            None => format!("{}", gid),
                        }
                    }
                    MapDomain::Module(mid) => mid.clone(),
                }
            };

            let mut type_entries: Vec<String> = Vec::new();
            for gen_name in &generator_names {
                let type_label = empty_or(gen_name);
                let details = match module_complex.find_generator(gen_name) {
                    None => ("(missing)".into(), "(missing)".into(), "(missing)".into()),
                    Some(entry) => {
                        match &entry.tag {
                            Tag::Local(_) => {
                                ("(local tag)".into(), "(local tag)".into(), "(local tag)".into())
                            }
                            Tag::Global(gid) => {
                                match self.find_type(*gid) {
                                    None => ("(not found)".into(), "(not found)".into(), "(not found)".into()),
                                    Some(type_entry) => {
                                        let cells = render_cells_by_dim(&*type_entry.complex);
                                        let diagrams = render_list(
                                            type_entry.complex.diagram_names().into_iter()
                                                .map(|n| empty_or(&n)).collect()
                                        );
                                        let maps = render_list(
                                            type_entry.complex.map_names().into_iter().map(|mn| {
                                                let dom = match type_entry.complex.find_map(&mn) {
                                                    Some(me) => string_of_domain(&me.domain),
                                                    None => "?".into(),
                                                };
                                                format!("{} :: {}", empty_or(&mn), dom)
                                            }).collect()
                                        );
                                        (cells, diagrams, maps)
                                    }
                                }
                            }
                        }
                    }
                };
                type_entries.push(format!(
                    "Type {}\n  - Cells: {}\n  - Diagrams: {}\n  - Maps: {}\n",
                    type_label, details.0, details.1, details.2
                ));
            }
            module_str.push_str(&type_entries.join("\n"));
            entries_str.push(module_str);
        }
        out.push_str(&entries_str.join("\n"));
        out
    }
}
