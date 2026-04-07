use crate::aux::{GlobalId, ModuleId, Tag};
use crate::core::complex::Complex;
use crate::core::diagram::CellData;
use indexmap::IndexMap;
use std::collections::HashMap;
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
    pub(crate) cells: HashMap<GlobalId, CellEntry>,
    pub(crate) cells_by_dim: HashMap<usize, Vec<GlobalId>>,
    pub(crate) types: HashMap<GlobalId, TypeEntry>,
    /// Modules in load order (dependencies before the files that depend on them).
    pub(crate) modules: IndexMap<ModuleId, Arc<Complex>>,
}

impl GlobalStore {
    /// Register a non-type cell with the given dimension and boundary data.
    pub fn set_cell(&mut self, id: GlobalId, dim: usize, data: CellData) {
        self.cells_by_dim.entry(dim).or_default().push(id);
        self.cells.insert(id, CellEntry { data });
        self.assert_invariants();
    }

    /// Register a type cell with its boundary data and definition complex.
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

    /// Register a module with its complex.
    pub fn set_module(&mut self, id: ModuleId, complex: Complex) {
        self.modules.insert(id, Arc::new(complex));
        self.assert_invariants();
    }

    /// Mutate the `Complex` for a module in place via `Arc::make_mut` (copy-on-write).
    ///
    /// Silently does nothing if the module id is not found.
    pub fn modify_module(&mut self, id: &str, f: impl FnOnce(&mut Complex)) {
        debug_assert!(self.modules.contains_key(id), "modify_module: module `{}` not found", id);
        if let Some(arc) = self.modules.get_mut(id) {
            f(Arc::make_mut(arc));
            self.assert_invariants();
        }
    }

    /// Mutate the `Complex` of a type entry in place via `Arc::make_mut` (copy-on-write).
    ///
    /// Silently does nothing if the type id is not found.
    pub fn modify_type_complex(&mut self, id: GlobalId, f: impl FnOnce(&mut Complex)) {
        debug_assert!(self.types.contains_key(&id), "modify_type_complex: type {} not found", id);
        if let Some(entry) = self.types.get_mut(&id) {
            f(Arc::make_mut(&mut entry.complex));
            self.assert_invariants();
        }
    }

    /// Look up a non-type cell by its global ID.
    pub fn find_cell(&self, id: GlobalId) -> Option<&CellEntry> {
        self.cells.get(&id)
    }

    /// Look up a type entry by its global ID.
    pub fn find_type(&self, id: GlobalId) -> Option<&TypeEntry> {
        self.types.get(&id)
    }

    /// Look up a module's complex by its string ID, returning a reference.
    pub fn find_module(&self, id: &str) -> Option<&Complex> {
        self.modules.get(id).map(|arc| &**arc)
    }

    /// Look up a module's complex by its string ID, returning a cloned `Arc`.
    ///
    /// The `Arc` lets callers cheaply share the module complex without cloning the data.
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

    /// Returns the number of non-type cells in the store.
    pub fn cells_count(&self) -> usize {
        self.cells.len()
    }

    /// Returns the number of types in the store.
    pub fn types_count(&self) -> usize {
        self.types.len()
    }

    /// Returns the number of modules in the store.
    pub fn modules_count(&self) -> usize {
        self.modules.len()
    }

    /// Returns an iterator over `(module_id, module_complex)` pairs, in load order.
    ///
    /// Dependencies always appear before the modules that include them.
    pub fn modules_iter(&self) -> impl Iterator<Item = (&str, &Complex)> {
        self.modules.iter().map(|(id, arc)| (id.as_str(), &**arc))
    }

    /// Debug-only check that every ID in `cells_by_dim` exists in `cells`.
    fn assert_invariants(&self) {
        for ids in self.cells_by_dim.values() {
            for id in ids {
                debug_assert!(self.cells.contains_key(id));
            }
        }
    }
}
