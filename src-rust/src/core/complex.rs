use std::collections::{BTreeMap, BTreeSet};
use crate::helper::{GlobalId, LocalId, ModuleId, Tag};
use super::diagram::{CellData, Diagram};
use super::morphism::Morphism;

/// The domain of a morphism entry: either a type or a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MorphismDomain {
    Type(GlobalId),
    Module(ModuleId),
}

#[derive(Debug, Clone)]
pub struct GeneratorEntry {
    pub tag: Tag,
    pub dim: usize,
}

#[derive(Debug, Clone)]
pub struct MorphismEntry {
    pub morphism: Morphism,
    pub domain: MorphismDomain,
}

#[derive(Debug, Clone)]
pub struct LocalCellEntry {
    pub data: CellData,
    pub dim: usize,
}

#[derive(Debug, Clone, Default)]
struct Generators {
    by_name: BTreeMap<LocalId, GeneratorEntry>,
    by_tag: BTreeMap<Tag, LocalId>,
    by_dim: BTreeMap<usize, BTreeSet<LocalId>>,
    classifiers: BTreeMap<LocalId, Diagram>,
}

#[derive(Debug, Clone, Default)]
struct LocalCells {
    by_id: BTreeMap<LocalId, LocalCellEntry>,
    by_dim: BTreeMap<usize, BTreeSet<LocalId>>,
}

/// A complex: the environment of generators, diagrams, morphisms, and local cells
/// associated with a single type or module.
#[derive(Debug, Clone, Default)]
pub struct Complex {
    generators: Generators,
    diagrams: BTreeMap<LocalId, Diagram>,
    morphisms: BTreeMap<LocalId, MorphismEntry>,
    local_cells: LocalCells,
    used_names: BTreeSet<LocalId>,
}

impl Complex {
    pub fn empty() -> Self {
        Self::default()
    }

    // ---- Generators ----

    pub fn add_generator(mut self, name: LocalId, classifier: Diagram) -> Self {
        let dim = if classifier.dim() < 0 { 0 } else { classifier.dim() as usize };
        let labels = &classifier.labels;
        let top_labels = &labels[dim];
        assert!(!top_labels.is_empty());
        let tag = top_labels[0].clone();

        self.generators.by_tag.insert(tag.clone(), name.clone());
        self.generators.by_dim.entry(dim).or_default().insert(name.clone());
        self.generators.classifiers.insert(name.clone(), classifier);
        self.generators.by_name.insert(name, GeneratorEntry { tag, dim });
        self
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
        self.generators.by_dim.get(&dim)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn generator_names(&self) -> Vec<LocalId> {
        self.generators.by_name.keys().cloned().collect()
    }

    // ---- Diagrams ----

    pub fn add_diagram(mut self, name: LocalId, diagram: Diagram) -> Self {
        self.diagrams.insert(name.clone(), diagram);
        self.used_names.insert(name);
        self
    }

    pub fn find_diagram(&self, name: &str) -> Option<&Diagram> {
        self.diagrams.get(name)
    }

    pub fn diagram_names(&self) -> Vec<LocalId> {
        self.diagrams.keys().cloned().collect()
    }

    // ---- Morphisms ----

    pub fn add_morphism(mut self, name: LocalId, domain: MorphismDomain, morphism: Morphism) -> Self {
        self.morphisms.insert(name.clone(), MorphismEntry { morphism, domain });
        self.used_names.insert(name);
        self
    }

    pub fn find_morphism(&self, name: &str) -> Option<&MorphismEntry> {
        self.morphisms.get(name)
    }

    pub fn morphism_names(&self) -> Vec<LocalId> {
        self.morphisms.keys().cloned().collect()
    }

    // ---- Local cells ----

    pub fn add_local_cell(mut self, name: LocalId, dim: usize, data: CellData) -> Self {
        self.local_cells.by_dim.entry(dim).or_default().insert(name.clone());
        self.local_cells.by_id.insert(name, LocalCellEntry { data, dim });
        self
    }

    pub fn find_local_cell(&self, name: &str) -> Option<&LocalCellEntry> {
        self.local_cells.by_id.get(name)
    }

    pub fn local_cell_dim(&self, name: &str) -> Option<usize> {
        self.local_cells.by_id.get(name).map(|e| e.dim)
    }

    pub fn local_cells_in_dim(&self, dim: usize) -> Vec<LocalId> {
        self.local_cells.by_dim.get(&dim)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    // ---- Name management ----

    pub fn name_in_use(&self, name: &str) -> bool {
        self.used_names.contains(name)
    }

    pub fn used_names(&self) -> Vec<LocalId> {
        self.used_names.iter().cloned().collect()
    }
}
