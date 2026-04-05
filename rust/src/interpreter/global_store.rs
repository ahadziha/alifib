use crate::aux::{GlobalId, ModuleId, Tag};
use crate::core::complex::Complex;
use crate::core::diagram::CellData;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TypeEntry {
    pub data: CellData,
    pub complex: Arc<Complex>,
}

#[derive(Debug, Clone)]
pub struct CellEntry {
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
            Tag::Local(name) => complex.find_local_cell(name).map(|e| e.data.clone()),
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
