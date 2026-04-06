//! The `Complex` type: the local environment for a single type or module.
//!
//! A [`Complex`] stores all the generators, diagrams, maps, and temporary
//! local cells in scope during elaboration of one type or module.  All write
//! operations maintain internal consistency invariants checked in debug builds.

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
struct GeneratorEntry {
    tag: Tag,
    dim: usize,
}

/// A named partial map together with the complex it maps from.
#[derive(Debug, Clone)]
struct MapEntry {
    map: PMap,
    domain: MapDomain,
}

/// A cell scoped to a type body, carrying a local tag rather than a global ID.
#[derive(Debug, Clone)]
struct LocalCellEntry {
    data: CellData,
}

/// All generators in a complex, kept in three parallel indices for O(1) lookup
/// by name, by tag, and by dimension.
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

/// Cells scoped to this type body: tagged with a local name rather than a global ID,
/// so their boundary data lives only in this Complex, not in the global cell tables.
#[derive(Debug, Clone, Default)]
struct LocalCells {
    /// Name -> cell entry.
    by_id: HashMap<LocalId, LocalCellEntry>,
    /// Dimension index: dim -> local cell names in that dimension.
    by_dim: HashMap<usize, HashSet<LocalId>>,
}

/// The environment of generators, diagrams, maps, and locally-scoped cells
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
    /// Create a complex with no generators, diagrams, maps, or local cells.
    pub fn empty() -> Self {
        Self::default()
    }

    // ---- Generators ----

    /// Register a generator with its name, runtime tag, and classifier diagram.
    pub fn add_generator(&mut self, name: LocalId, tag: Tag, classifier: Diagram) {
        let dim = classifier.top_dim();
        debug_assert_eq!(classifier.labels.get(dim).and_then(|r| r.first()), Some(&tag));

        self.generators.by_tag.insert(tag.clone(), name.clone());
        self.generators.by_dim.entry(dim).or_default().insert(name.clone());
        self.generators.classifiers.insert(name.clone(), classifier);
        self.generators.by_name.insert(name, GeneratorEntry { tag, dim });

        self.assert_invariants();
    }

    /// Look up a generator by name; returns its tag and dimension if found.
    pub fn find_generator(&self, name: &str) -> Option<(&Tag, usize)> {
        self.generators.by_name.get(name).map(|e| (&e.tag, e.dim))
    }

    /// Look up a generator by its runtime tag; returns the local name if found.
    pub fn find_generator_by_tag(&self, tag: &Tag) -> Option<&LocalId> {
        self.generators.by_tag.get(tag)
    }

    /// Return the classifier diagram for the named generator, if it exists.
    pub fn classifier(&self, name: &str) -> Option<&Diagram> {
        self.generators.classifiers.get(name)
    }

    /// Iterate over all generators as `(name, tag, dim)` triples.
    pub fn generators_iter(&self) -> impl Iterator<Item = (&LocalId, &Tag, usize)> {
        self.generators.by_name.iter().map(|(name, e)| (name, &e.tag, e.dim))
    }

    // ---- Diagrams ----

    /// Store a named diagram in the complex.
    pub fn add_diagram(&mut self, name: LocalId, diagram: Diagram) {
        self.diagrams.insert(name.clone(), diagram);
        self.used_names.insert(name);
        self.assert_invariants();
    }

    /// Look up a diagram by name.
    pub fn find_diagram(&self, name: &str) -> Option<&Diagram> {
        self.diagrams.get(name)
    }

    /// Iterate over all stored diagrams as `(name, diagram)` pairs.
    pub fn diagrams_iter(&self) -> impl Iterator<Item = (&LocalId, &Diagram)> {
        self.diagrams.iter()
    }

    // ---- Maps ----

    /// Store a named partial map together with the complex it maps from.
    pub fn add_map(&mut self, name: LocalId, domain: MapDomain, map: PMap) {
        self.maps.insert(name.clone(), MapEntry { map, domain });
        self.used_names.insert(name);
        self.assert_invariants();
    }

    /// Look up a map by name; returns the map and its domain if found.
    pub fn find_map(&self, name: &str) -> Option<(&PMap, &MapDomain)> {
        self.maps.get(name).map(|e| (&e.map, &e.domain))
    }

    /// Iterate over all stored maps as `(name, map, domain)` triples.
    pub fn maps_iter(&self) -> impl Iterator<Item = (&LocalId, &PMap, &MapDomain)> {
        self.maps.iter().map(|(name, e)| (name, &e.map, &e.domain))
    }

    // ---- Local cells ----

    /// Register a cell scoped to this type body (carries a local tag; boundary data
    /// lives here rather than in the global cell tables).
    pub fn add_local_cell(&mut self, name: LocalId, dim: usize, data: CellData) {
        self.local_cells
            .by_dim
            .entry(dim)
            .or_default()
            .insert(name.clone());
        self.local_cells.by_id.insert(name, LocalCellEntry { data });
        self.assert_invariants();
    }

    /// Look up a local cell by name; returns its boundary data if found.
    pub fn find_local_cell(&self, name: &str) -> Option<&CellData> {
        self.local_cells.by_id.get(name).map(|e| &e.data)
    }

    // ---- Name management ----

    /// True if `name` is already taken by a diagram or map in this complex.
    pub fn name_in_use(&self, name: &str) -> bool {
        self.used_names.contains(name)
    }

    // ---- Internal ----

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
