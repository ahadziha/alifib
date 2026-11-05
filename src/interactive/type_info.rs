//! One module-addressed description of a type, shared by all front ends.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::aux::{GlobalId, Tag};
use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram, Sign};
use crate::interpreter::GlobalStore;
use crate::output::render_diagram;

use super::engine::resolve_type_id;

use super::response::{
    build_map_entries, diagram_info, resolve_domain_complex, DiagramEntryInfo, GeneratorInfo, TypeInfo,
};

/// Thinness in normalize order, including propagation from earlier types.
pub struct ThinIndex {
    by_type: HashMap<GlobalId, Vec<usize>>,
}

impl ThinIndex {
    pub fn of(store: &GlobalStore) -> Self {
        let norm = store.normalize();
        let mut known_thin: HashSet<Tag> = HashSet::new();
        let mut by_type = HashMap::new();
        for module in &norm.modules {
            let Some(mc) = store.find_module(&module.path) else { continue };
            for t in module.types.iter().filter(|t| !t.name.is_empty()) {
                let Some((Tag::Global(gid), _)) = mc.find_generator(&t.name) else { continue };
                let tc = store.find_type(*gid).map(|te| &*te.complex);
                let mut new_thin: Vec<Tag> = tc
                    .map(compute_thin_tags)
                    .unwrap_or_default();
                known_thin.extend(new_thin.iter().cloned());
                let propagated = tc
                    .map(|tc| propagate_thin_through_maps(store, tc, &known_thin))
                    .unwrap_or_default();
                new_thin.extend(propagated.iter().cloned());
                known_thin.extend(propagated);
                by_type.insert(*gid, new_thin.iter().filter_map(global_tag).collect());
            }
        }
        Self { by_type }
    }
}

/// Describe every named type, in the store's module and definition order.
pub fn store_types(store: &GlobalStore, thin: &ThinIndex) -> Vec<TypeInfo> {
    store.modules_iter()
        .flat_map(|(path, _)| module_types(store, thin, path))
        .collect()
}

/// Describe the named types of one store module, in definition order.
pub fn module_types(store: &GlobalStore, thin: &ThinIndex, module_path: &str) -> Vec<TypeInfo> {
    let Some(mc) = store.find_module(module_path) else { return Vec::new() };
    let mut names: Vec<_> = mc.generators_iter()
        .filter(|(name, _, _)| !name.is_empty())
        .map(|(name, _, _)| name)
        .collect();
    names.sort_by_key(|name| mc.generator_order(name));
    names.into_iter()
        .map(|name| type_info(store, thin, module_path, name)
            .expect("interpreter invariant violated: module generator has no type entry"))
        .collect()
}

/// A tag's global id, the identity the wire uses; other tags have none.
pub(super) fn global_tag(tag: &Tag) -> Option<usize> {
    match tag {
        Tag::Global(gid) => Some(gid.as_usize()),
        Tag::Local(_) | Tag::Hole(_) => None,
    }
}

fn face_tags(store: &GlobalStore, tc: &Complex, tag: &Tag) -> Vec<usize> {
    let Some(data) = store.cell_data_for_tag(tc, tag) else { return Vec::new() };
    let CellData::Boundary { boundary_in, boundary_out } = &data else { return Vec::new() };
    let mut face_tags = Vec::new();
    for bd in [boundary_in, boundary_out] {
        if let Some(labels) = bd.labels_at(bd.top_dim()) {
            face_tags.extend(labels.iter().filter_map(global_tag));
        }
    }
    face_tags
}

fn compute_thin_tags(tc: &Complex) -> Vec<Tag> {
    let Some(values) = tc.find_index("thin") else { return Vec::new() };
    let mut tags = Vec::new();
    for name in values {
        if let Some((tag, _)) = tc.find_generator(name) {
            tags.push(tag.clone());
        } else if let Some(diag) = tc.find_diagram(name)
            && let Some(tag) = diag.top_label()
        {
            tags.push(tag.clone());
        }
    }
    tags
}

fn propagate_thin_through_maps(
    store: &GlobalStore,
    tc: &Complex,
    known_thin: &HashSet<Tag>,
) -> Vec<Tag> {
    let mut tags = Vec::new();
    for (_, pmap, domain) in tc.maps_iter() {
        let Some(dc) = resolve_domain_complex(store, domain) else { continue };
        for (_, gen_tag, _) in dc.generators_iter() {
            if known_thin.contains(gen_tag)
                && let Ok(image) = pmap.image(gen_tag)
                && image.is_cell()
                && let Some(tag) = image.top_label()
            {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

/// Describe a type in exactly the given store module.
pub fn type_info(
    store: &GlobalStore,
    thin: &ThinIndex,
    module_path: &str,
    name: &str,
) -> Result<TypeInfo, String> {
    let gid = resolve_type_id(store, module_path, name)?;
    let type_complex = store.find_type(gid)
        .map(|entry| Arc::clone(&entry.complex))
        .ok_or_else(|| format!("type entry for '{}' not found", name))?;
    let module_complex = store.find_module(module_path)
        .ok_or_else(|| format!("module '{}' not found", module_path))?;

    let mut generators: Vec<GeneratorInfo> = type_complex
        .generators_iter()
        .filter(|(n, _, _)| !n.is_empty())
        .map(|(gen_name, tag, dim)| {
            let (input, output) = if dim > 0 {
                match store.cell_data_for_tag(&type_complex, tag) {
                    Some(CellData::Boundary { boundary_in, boundary_out }) => (
                        Some(diagram_info(&boundary_in, &type_complex)),
                        Some(diagram_info(&boundary_out, &type_complex)),
                    ),
                    _ => (None, None),
                }
            } else {
                (None, None)
            };
            GeneratorInfo {
                name: gen_name.clone(), dim, input, output,
                tag: global_tag(tag),
                face_tags: face_tags(store, &type_complex, tag),
            }
        })
        .collect();
    generators.sort_by(|a, b| a.dim.cmp(&b.dim).then_with(|| a.name.cmp(&b.name)));

    let mut diagrams: Vec<DiagramEntryInfo> = type_complex
        .diagrams_iter()
        .filter(|(n, _)| !n.is_empty())
        .map(|(diag_name, diag)| {
            let dim = diag.top_dim();
            let (input, output) = if dim > 0 {
                let k = dim - 1;
                let inp = Diagram::boundary(Sign::Input, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, &type_complex));
                let out = Diagram::boundary(Sign::Output, k, diag)
                    .ok()
                    .map(|d| diagram_info(&d, &type_complex));
                (inp, out)
            } else {
                (None, None)
            };
            DiagramEntryInfo {
                name: diag_name.clone(),
                dim,
                input,
                output,
                expr: render_diagram(diag, &type_complex),
            }
        })
        .collect();
    diagrams.sort_by(|a, b| a.name.cmp(&b.name));

    let maps = build_map_entries(&type_complex, module_complex, store);

    Ok(TypeInfo {
        name: name.to_owned(), module: module_path.to_owned(), generators, diagrams, maps,
        thin_tags: thin.by_type.get(&gid).cloned().unwrap_or_default(),
    })
}
