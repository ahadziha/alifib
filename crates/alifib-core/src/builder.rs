use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
    partial_map::PartialMap,
};
use crate::global_store::GlobalStore;
use std::sync::Arc;

fn mapped_cell_data(map: &PartialMap, source_cell_data: &CellData) -> Option<CellData> {
    match source_cell_data {
        CellData::Zero => Some(CellData::Zero),
        CellData::Boundary { boundary_in, boundary_out } => {
            let image_in = PartialMap::apply(map, boundary_in).ok()?;
            let image_out = PartialMap::apply(map, boundary_out).ok()?;
            Some(CellData::Boundary {
                boundary_in: Arc::new(image_in),
                boundary_out: Arc::new(image_out),
            })
        }
    }
}

#[derive(Debug, Clone)]
pub enum BuildError {
    NameConflict(String),
    DiagramError(String),
    NotFound(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::NameConflict(name) => write!(f, "name already in use: {}", name),
            BuildError::DiagramError(msg) => write!(f, "diagram error: {}", msg),
            BuildError::NotFound(name) => write!(f, "not found: {}", name),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeneratorHandle {
    pub tag: Tag,
    pub dim: usize,
    pub classifier: Diagram,
}

pub struct ModuleBuilder {
    module_id: String,
    state: Arc<GlobalStore>,
}

impl ModuleBuilder {
    pub fn new(module_id: impl Into<String>) -> Self {
        let module_id = module_id.into();
        let mut state = GlobalStore::empty();

        let root_id = GlobalId::fresh();
        let root_diagram = Diagram::cell(Tag::Global(root_id), &CellData::Zero)
            .expect("failed to create root type cell");

        let root_name: LocalId = String::new();
        let mut module_complex = Complex::empty();
        module_complex.add_generator(root_name.clone(), Tag::Global(root_id), root_diagram.clone());
        module_complex.add_diagram(root_name, root_diagram);

        state.set_type(root_id, CellData::Zero, Complex::empty());
        state.set_module(module_id.clone(), module_complex);

        ModuleBuilder { module_id, state: Arc::new(state) }
    }

    pub fn with_state(module_id: impl Into<String>, state: Arc<GlobalStore>) -> Self {
        let module_id = module_id.into();
        // Initialize module context if not already present
        if state.find_module(&module_id).is_some() {
            return ModuleBuilder { module_id, state };
        }

        let mut new_state = (*state).clone();
        let root_id = GlobalId::fresh();
        let root_diagram = Diagram::cell(Tag::Global(root_id), &CellData::Zero)
            .expect("failed to create root type cell");

        let root_name: LocalId = String::new();
        let mut module_complex = Complex::empty();
        module_complex.add_generator(root_name.clone(), Tag::Global(root_id), root_diagram.clone());
        module_complex.add_diagram(root_name, root_diagram);

        new_state.set_type(root_id, CellData::Zero, Complex::empty());
        new_state.set_module(module_id.clone(), module_complex);

        ModuleBuilder { module_id, state: Arc::new(new_state) }
    }

    pub fn add_type(&mut self, name: &str) -> Result<TypeBuilder<'_>, BuildError> {
        let module_scope = self.state.find_module(&self.module_id)
            .ok_or_else(|| BuildError::NotFound(self.module_id.clone()))?;

        if module_scope.name_in_use(name) {
            return Err(BuildError::NameConflict(name.to_owned()));
        }

        let type_id = GlobalId::fresh();
        Arc::make_mut(&mut self.state).set_type(type_id, CellData::Zero, Complex::empty());

        Ok(TypeBuilder {
            module: self,
            type_name: name.to_owned(),
            type_id,
            working_complex: Complex::empty(),
        })
    }

    pub fn finish(self) -> Arc<GlobalStore> {
        self.state
    }

    pub fn state(&self) -> &GlobalStore {
        &self.state
    }
}

pub struct TypeBuilder<'a> {
    module: &'a mut ModuleBuilder,
    type_name: String,
    type_id: GlobalId,
    working_complex: Complex,
}

impl<'a> TypeBuilder<'a> {
    fn state_mut(&mut self) -> &mut GlobalStore {
        Arc::make_mut(&mut self.module.state)
    }

    pub fn add_object(&mut self, name: &str) -> Result<GeneratorHandle, BuildError> {
        if self.working_complex.name_in_use(name) {
            return Err(BuildError::NameConflict(name.to_owned()));
        }

        let id = GlobalId::fresh();
        let classifier = Diagram::cell(Tag::Global(id), &CellData::Zero)
            .map_err(|e| BuildError::DiagramError(e.to_string()))?;

        self.working_complex.add_generator(name.to_owned(), Tag::Global(id), classifier.clone());
        self.working_complex.add_diagram(name.to_owned(), classifier.clone());
        self.state_mut().set_cell(id, 0, CellData::Zero);

        Ok(GeneratorHandle { tag: Tag::Global(id), dim: 0, classifier })
    }

    pub fn add_generator(
        &mut self,
        name: &str,
        source: &Diagram,
        target: &Diagram,
    ) -> Result<GeneratorHandle, BuildError> {
        if self.working_complex.name_in_use(name) {
            return Err(BuildError::NameConflict(name.to_owned()));
        }

        let boundaries = CellData::Boundary {
            boundary_in: Arc::new(source.clone()),
            boundary_out: Arc::new(target.clone()),
        };
        let dim = source.top_dim() + 1;
        let id = GlobalId::fresh();
        let classifier = Diagram::cell(Tag::Global(id), &boundaries)
            .map_err(|e| BuildError::DiagramError(e.to_string()))?;

        self.working_complex.add_generator(name.to_owned(), Tag::Global(id), classifier.clone());
        self.working_complex.add_diagram(name.to_owned(), classifier.clone());
        self.state_mut().set_cell(id, dim, boundaries);

        Ok(GeneratorHandle { tag: Tag::Global(id), dim, classifier })
    }

    pub fn add_diagram(&mut self, name: &str, diagram: Diagram) -> Result<(), BuildError> {
        if self.working_complex.name_in_use(name) {
            return Err(BuildError::NameConflict(name.to_owned()));
        }
        self.working_complex.add_diagram(name.to_owned(), diagram);
        Ok(())
    }

    pub fn add_map(
        &mut self,
        name: &str,
        domain: MapDomain,
        map: PartialMap,
    ) -> Result<(), BuildError> {
        if self.working_complex.name_in_use(name) {
            return Err(BuildError::NameConflict(name.to_owned()));
        }
        self.working_complex.add_map(name.to_owned(), domain, map);
        Ok(())
    }

    pub fn attach(
        &mut self,
        name: &str,
        attach_type_id: GlobalId,
        mut map: PartialMap,
    ) -> Result<(), BuildError> {
        let attachment = {
            let entry = self.module.state.find_type(attach_type_id)
                .ok_or_else(|| BuildError::NotFound(format!("type {}", attach_type_id)))?;
            Arc::clone(&entry.complex)
        };

        // Collect generators sorted by dim for ordered processing
        let mut sorted_gens: Vec<(usize, LocalId, Tag)> = attachment
            .generators_iter()
            .map(|(n, t, d)| (d, n.clone(), t.clone()))
            .collect();
        sorted_gens.sort_by_key(|(d, _, _)| *d);

        for (generator_dim, generator_name, generator_tag) in &sorted_gens {
            if map.is_defined_at(generator_tag) {
                continue;
            }

            let Tag::Global(global_id) = generator_tag else { continue; };
            let source_cell_data = {
                let Some(cell_entry) = self.module.state.find_cell(*global_id) else { continue; };
                cell_entry.data.clone()
            };

            let image_cell_data = mapped_cell_data(&map, &source_cell_data);

            let Some(image_cell_data) = image_cell_data else { continue; };

            let qualified_name = if name.is_empty() {
                generator_name.clone()
            } else if generator_name.is_empty() {
                name.to_owned()
            } else {
                format!("{}.{}", name, generator_name)
            };

            let image_id = GlobalId::fresh();
            self.state_mut().set_cell(image_id, *generator_dim, image_cell_data.clone());
            let image_tag = Tag::Global(image_id);

            let Ok(image_classifier) = Diagram::cell(image_tag.clone(), &image_cell_data) else {
                continue;
            };

            self.working_complex.add_generator(qualified_name, image_tag, image_classifier.clone());
            map.insert_raw(Tag::Global(*global_id), *generator_dim, source_cell_data, image_classifier);
        }

        self.working_complex.add_map(name.to_owned(), MapDomain::Type(attach_type_id), map);
        Ok(())
    }

    pub fn diagram(&self, name: &str) -> Option<&Diagram> {
        self.working_complex.find_diagram(name)
    }

    pub fn type_id(&self) -> GlobalId {
        self.type_id
    }

    pub fn finish(mut self) -> Result<GlobalId, BuildError> {
        let type_id = self.type_id;
        let type_name = self.type_name.clone();

        // Build identity map for the working complex
        let identity = {
            let entries: Vec<(Tag, usize, CellData, Diagram)> = self.working_complex
                .generators_iter()
                .filter_map(|(gen_name, tag, dim)| {
                    let cell_data = self.module.state.cell_data_for_tag(&self.working_complex, tag)?;
                    let image = self.working_complex.classifier(gen_name)?.clone();
                    Some((tag.clone(), dim, cell_data, image))
                })
                .collect();
            PartialMap::of_entries(entries, true)
        };

        self.working_complex.add_map(type_name.clone(), MapDomain::Type(type_id), identity);

        let state = Arc::make_mut(&mut self.module.state);
        state.set_type(type_id, CellData::Zero, self.working_complex);

        let classifier = Diagram::cell(Tag::Global(type_id), &CellData::Zero)
            .map_err(|e| BuildError::DiagramError(e.to_string()))?;

        state.modify_module(&self.module.module_id, |m| {
            m.add_generator(type_name.clone(), Tag::Global(type_id), classifier.clone());
            m.add_diagram(type_name, classifier);
        });

        Ok(type_id)
    }
}
