use super::diagram::{CellData, Diagram};
use super::map::PMap;
use crate::aux::{GlobalId, LocalId, ModuleId, Tag};
use std::collections::{HashMap, HashSet};

/// The domain of a map entry: either a type or a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapDomain {
    Type(GlobalId),
    Module(ModuleId),
}

/// Metadata for a generator within a complex.
#[derive(Debug, Clone)]
pub struct GeneratorEntry {
    /// The tag (Global or Local) that identifies this generator in diagrams and maps.
    pub tag: Tag,
    /// The dimension of this generator.
    pub dim: usize,
}

/// A named partial map together with the complex it maps from.
#[derive(Debug, Clone)]
pub struct MapEntry {
    pub map: PMap,
    /// Records whether the domain is a type complex or a module complex.
    pub domain: MapDomain,
}

/// A locally-scoped cell created during type elaboration, not persisted in the global store.
#[derive(Debug, Clone)]
pub struct LocalCellEntry {
    /// The boundary specification (Zero for 0-cells, Boundary for n-cells).
    pub data: CellData,
}

#[derive(Debug, Clone, Default)]
struct Generators {
    /// Primary index: generator name -> metadata.
    by_name: HashMap<LocalId, GeneratorEntry>,
    /// Reverse index: generator tag -> generator name.
    by_tag: HashMap<Tag, LocalId>,
    /// Dimension index: dim -> generator names in that dim.
    by_dim: HashMap<usize, HashSet<LocalId>>,
    /// Classifier diagram for each generator name.
    classifiers: HashMap<LocalId, Diagram>,
}

#[derive(Debug, Clone, Default)]
struct LocalCells {
    /// Name -> cell entry.
    by_id: HashMap<LocalId, LocalCellEntry>,
    /// Dimension index: dim -> local cell names in that dimension.
    by_dim: HashMap<usize, HashSet<LocalId>>,
}

/// A complex: the environment of generators, diagrams, maps, and local cells
/// associated with a single type or module.
///
/// Invariants (checked in debug builds):
/// - every generator in `by_name` has matching entries in `by_tag` and `classifiers`
/// - every `(dim, name)` membership in `by_dim` agrees with `by_name[name].dim`
/// - every map/diagram name is included in `used_names`
/// - every local-cell `(dim, name)` membership in `local_cells.by_dim` has a `by_id` entry
#[derive(Debug, Clone, Default)]
pub struct Complex {
    generators: Generators,
    diagrams: HashMap<LocalId, Diagram>,
    maps: HashMap<LocalId, MapEntry>,
    local_cells: LocalCells,
    used_names: HashSet<LocalId>,
}

impl Complex {
    pub fn empty() -> Self {
        Self::default()
    }

    // ---- Generators ----

    pub fn add_generator(&mut self, name: LocalId, tag: Tag, classifier: Diagram) {
        let dim = classifier.top_dim();
        debug_assert_eq!(classifier.labels.get(dim).and_then(|r| r.first()), Some(&tag));

        self.generators.by_tag.insert(tag.clone(), name.clone());
        self.generators.by_dim.entry(dim).or_default().insert(name.clone());
        self.generators.classifiers.insert(name.clone(), classifier);
        self.generators.by_name.insert(name, GeneratorEntry { tag, dim });

        self.assert_invariants();
    }

    pub fn find_generator(&self, name: &str) -> Option<&GeneratorEntry> {
        self.generators.by_name.get(name)
    }

    pub fn find_generator_by_tag(&self, tag: &Tag) -> Option<&LocalId> {
        self.generators.by_tag.get(tag)
    }

    pub fn classifier(&self, name: &str) -> Option<&Diagram> {
        self.generators.classifiers.get(name)
    }

    pub fn generator_dim(&self, name: &str) -> Option<usize> {
        self.generators.by_name.get(name).map(|e| e.dim)
    }

    pub fn generators_in_dim(&self, dim: usize) -> Vec<LocalId> {
        self.generators
            .by_dim
            .get(&dim)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn generators_iter(&self) -> impl Iterator<Item = (&LocalId, &GeneratorEntry)> {
        self.generators.by_name.iter()
    }

    // ---- Diagrams ----

    pub fn add_diagram(&mut self, name: LocalId, diagram: Diagram) {
        self.diagrams.insert(name.clone(), diagram);
        self.used_names.insert(name);
        self.assert_invariants();
    }

    pub fn find_diagram(&self, name: &str) -> Option<&Diagram> {
        self.diagrams.get(name)
    }

    pub fn diagrams_iter(&self) -> impl Iterator<Item = (&LocalId, &Diagram)> {
        self.diagrams.iter()
    }

    // ---- Maps ----

    pub fn add_map(&mut self, name: LocalId, domain: MapDomain, map: PMap) {
        self.maps.insert(name.clone(), MapEntry { map, domain });
        self.used_names.insert(name);
        self.assert_invariants();
    }

    pub fn find_map(&self, name: &str) -> Option<&MapEntry> {
        self.maps.get(name)
    }

    pub fn maps_iter(&self) -> impl Iterator<Item = (&LocalId, &MapEntry)> {
        self.maps.iter()
    }

    // ---- Local cells ----

    pub fn add_local_cell(&mut self, name: LocalId, dim: usize, data: CellData) {
        self.local_cells
            .by_dim
            .entry(dim)
            .or_default()
            .insert(name.clone());
        self.local_cells.by_id.insert(name, LocalCellEntry { data });
        self.assert_invariants();
    }

    pub fn find_local_cell(&self, name: &str) -> Option<&LocalCellEntry> {
        self.local_cells.by_id.get(name)
    }

    // ---- Name management ----

    pub fn name_in_use(&self, name: &str) -> bool {
        self.used_names.contains(name)
    }

    fn assert_invariants(&self) {
        debug_assert_eq!(
            self.generators.by_name.len(),
            self.generators.classifiers.len()
        );

        for (name, generator_entry) in &self.generators.by_name {
            debug_assert!(self.generators.classifiers.contains_key(name));
            debug_assert_eq!(self.generators.by_tag.get(&generator_entry.tag), Some(name));
            debug_assert!(
                self.generators
                    .by_dim
                    .get(&generator_entry.dim)
                    .is_some_and(|names| names.contains(name))
            );
        }

        for (dim, names) in &self.generators.by_dim {
            for name in names {
                let Some(generator_entry) = self.generators.by_name.get(name) else {
                    debug_assert!(false, "generator present in by_dim without by_name entry");
                    continue;
                };
                debug_assert_eq!(generator_entry.dim, *dim);
            }
        }

        for name in self.diagrams.keys() {
            debug_assert!(self.used_names.contains(name));
        }

        for name in self.maps.keys() {
            debug_assert!(self.used_names.contains(name));
        }

        for names in self.local_cells.by_dim.values() {
            for name in names {
                debug_assert!(self.local_cells.by_id.contains_key(name));
            }
        }
    }
}
