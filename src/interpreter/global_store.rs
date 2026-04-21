use crate::aux::{GlobalId, ModuleId, Tag};
use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram, Sign};
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
    /// Module short names (filename without extension) to canonical paths.
    /// Populated as modules are registered; used for domain resolution in type blocks.
    pub(crate) module_names: HashMap<String, ModuleId>,
}

/// Allocate a fresh global cell, create its classifier diagram, and insert
/// both the generator entry and the diagram into `complex`.
///
/// Returns `(GlobalId, dim)`. The caller is responsible for forwarding
/// these to [`GlobalStore::set_cell`] to complete registration in the store.
///
/// If `diagram` is `Some` it is stored as the named diagram (e.g. the full
/// proof term); if `None` the classifier is stored instead, matching the
/// behaviour of the static interpreter for source-declared generators.
///
/// Works on any `&mut Complex` — a local interpreter scope or a store-managed
/// type complex accessed through [`GlobalStore::modify_type_complex`].
pub fn insert_global_cell(
    complex: &mut Complex,
    name: String,
    cell_data: &CellData,
    diagram: Option<Diagram>,
) -> Result<(GlobalId, usize), crate::aux::Error> {
    let gid = GlobalId::fresh();
    let tag = Tag::Global(gid);
    let dim = match cell_data {
        CellData::Zero => 0,
        CellData::Boundary { boundary_in, .. } => boundary_in.top_dim() + 1,
    };
    let classifier = Diagram::cell(tag.clone(), cell_data)?;
    complex.add_generator(name.clone(), tag, classifier.clone());
    complex.add_diagram(name, diagram.unwrap_or(classifier));
    Ok((gid, dim))
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
    ///
    /// Also records the module's short name (filename without `.ali` extension)
    /// in the module names table for domain resolution in type blocks.
    pub fn set_module(&mut self, id: ModuleId, complex: Complex) {
        if let Some(short_name) = module_short_name(&id) {
            self.module_names.insert(short_name, id.clone());
        }
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
    /// Returns `Some(f(...))` if the type exists, `None` otherwise.
    /// Silently does nothing (returns `None`) if the type id is not found.
    pub fn modify_type_complex<T>(&mut self, id: GlobalId, f: impl FnOnce(&mut Complex) -> T) -> Option<T> {
        debug_assert!(self.types.contains_key(&id), "modify_type_complex: type {} not found", id);
        if let Some(entry) = self.types.get_mut(&id) {
            let result = f(Arc::make_mut(&mut entry.complex));
            self.assert_invariants();
            Some(result)
        } else {
            None
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

    /// Search every loaded module for a generator named `type_name` and return
    /// its `GlobalId` if found.
    ///
    /// Used when the canonical module path is not known (e.g. from the REPL),
    /// avoiding the canonical-path vs. source-file key mismatch.
    pub fn find_type_gid(&self, type_name: &str) -> Option<GlobalId> {
        self.modules_iter().find_map(|(_, mc)| {
            mc.find_generator(type_name).and_then(|(tag, _)| match tag {
                Tag::Global(gid) => Some(*gid),
                _ => None,
            })
        })
    }

    /// Register a cell as a first-class generator inside an *existing* type
    /// that is already present in the store.
    ///
    /// Delegates the complex-level work to [`insert_global_cell`], then
    /// records the cell in the global cell table via [`set_cell`].
    ///
    /// If `diagram` is `Some`, it is stored alongside the classifier (e.g. the
    /// full proof diagram); otherwise the classifier itself is stored as the
    /// diagram (matching the behaviour of the static interpreter).
    pub fn register_generator(
        &mut self,
        type_gid: GlobalId,
        name: String,
        cell_data: CellData,
        diagram: Option<Diagram>,
    ) -> Result<GlobalId, String> {
        let (gid, dim) = self
            .modify_type_complex(type_gid, |cx| {
                insert_global_cell(cx, name, &cell_data, diagram)
            })
            .ok_or_else(|| format!("type {} not found in store", type_gid))?
            .map_err(|e| format!("{}", e))?;

        self.set_cell(gid, dim, cell_data);
        Ok(gid)
    }

    /// Register a completed proof diagram as a first-class generator.
    ///
    /// Extracts the source and target (dim-1)-boundaries from `diagram`,
    /// constructs the [`CellData`], and delegates to [`register_generator`].
    /// `dim` is the dimension of the proof cell (i.e. `source_diagram.top_dim() + 1`).
    pub fn register_proof_diagram(
        &mut self,
        type_gid: GlobalId,
        name: String,
        diagram: Diagram,
        dim: usize,
    ) -> Result<GlobalId, String> {
        let n = dim - 1;
        let boundary_in = Arc::new(
            Diagram::boundary(Sign::Source, n, &diagram)
                .map_err(|e| format!("source boundary: {}", e))?,
        );
        let boundary_out = Arc::new(
            Diagram::boundary(Sign::Target, n, &diagram)
                .map_err(|e| format!("target boundary: {}", e))?,
        );
        let cell_data = CellData::Boundary { boundary_in, boundary_out };
        self.register_generator(type_gid, name, cell_data, Some(diagram))
    }

    /// Look up a module's complex by its short name (filename without extension).
    ///
    /// Returns the canonical path and an `Arc` to the module's complex.
    pub fn resolve_module_by_name(&self, name: &str) -> Option<(&str, Arc<Complex>)> {
        let canonical_path = self.module_names.get(name)?;
        let arc = self.modules.get(canonical_path).map(Arc::clone)?;
        Some((canonical_path.as_str(), arc))
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

/// Extract a module's short name from its canonical file path.
///
/// Strips the directory and the `.ali` extension: `/path/to/Filename.ali` → `"Filename"`.
fn module_short_name(canonical_path: &str) -> Option<String> {
    let filename = std::path::Path::new(canonical_path)
        .file_stem()?
        .to_str()?;
    Some(filename.to_owned())
}
