use std::collections::{BTreeMap, HashMap};
use crate::aux::{Error, Tag};
use super::diagram::{CellData, Diagram, PasteTree, Sign};

#[derive(Debug, Clone)]
pub struct Entry {
    pub cell_data: CellData,
    pub image: Diagram,
}

/// A partial map: maps tags (cells) to diagrams.
#[derive(Debug, Clone)]
pub struct PMap {
    table: HashMap<Tag, Entry>,
    by_dim: BTreeMap<usize, Vec<Tag>>,
    pub cellular: bool,
}

impl PMap {
    /// Create an empty partial map.
    pub fn empty() -> Result<PMap, Error> {
        Ok(PMap {
            table: HashMap::new(),
            by_dim: BTreeMap::new(),
            cellular: true,
        })
    }

    /// Build a partial map from a list of (tag, dim, cell_data, image) entries.
    pub fn of_entries(entries: Vec<(Tag, usize, CellData, Diagram)>, cellular: bool) -> PMap {
        let mut table = HashMap::with_capacity(entries.len());
        let mut by_dim: BTreeMap<usize, Vec<Tag>> = BTreeMap::new();
        for (tag, dim, cell_data, image) in entries {
            table.insert(tag.clone(), Entry { cell_data, image });
            by_dim.entry(dim).or_default().push(tag);
        }
        PMap { table, by_dim, cellular }
    }

    pub fn is_defined_at(&self, tag: &Tag) -> bool {
        self.table.contains_key(tag)
    }

    pub fn image(&self, tag: &Tag) -> Result<&Diagram, Error> {
        self.table.get(tag)
            .map(|e| &e.image)
            .ok_or_else(|| Error::new("not in the domain of definition"))
    }

    /// Return all (dim, tags) pairs, tags in insertion order.
    pub fn domain_by_dim(&self) -> Vec<(usize, Vec<Tag>)> {
        self.by_dim.iter().map(|(&d, tags)| (d, tags.iter().rev().cloned().collect())).collect()
    }

    pub fn has_local_labels(&self) -> bool {
        self.table.values().any(|e| e.image.has_local_labels())
    }

    /// Insert an entry directly without boundary validation. Used for incremental
    /// construction where boundaries have already been verified by other means.
    pub fn insert_raw(&mut self, tag: Tag, dim: usize, cell_data: CellData, image: Diagram) {
        self.cellular = self.cellular && image.is_cell();
        self.by_dim.entry(dim).or_default().push(tag.clone());
        self.table.insert(tag, Entry { cell_data, image });
    }

    /// Extend the partial map with a new entry, checking boundary compatibility.
    /// Consumes `f` to avoid an unnecessary clone.
    pub fn extend(
        f: PMap,
        tag: Tag,
        dim: usize,
        cell_data: CellData,
        image: Diagram,
    ) -> Result<PMap, Error> {
        if f.is_defined_at(&tag) {
            return Err(Error::new("already defined"));
        }
        if image.dim() != dim as isize {
            return Err(Error::new("dimensions do not match"));
        }
        if !image.is_round() {
            return Err(Error::new("image is not round"));
        }

        if dim == 0 {
            match &cell_data {
                CellData::Zero => {
                    let mut new_m = f;
                    new_m.table.insert(tag.clone(), Entry { cell_data, image });
                    new_m.by_dim.entry(dim).or_default().push(tag);
                    Ok(new_m)
                }
                CellData::Boundary { .. } => panic!("0-cell cannot have boundaries"),
            }
        } else {
            match &cell_data {
                CellData::Zero => panic!("n-cell must have boundaries"),
                CellData::Boundary { boundary_in, boundary_out } => {
                    let mapped_in = PMap::apply(&f, boundary_in)?;
                    let mapped_out = PMap::apply(&f, boundary_out)?;

                    let boundary_idx = dim - 1;
                    let expected_input = Diagram::boundary_normal(Sign::Input, boundary_idx, &image)?;
                    let mapped_input = Diagram::normal(&mapped_in);
                    if !Diagram::equal(&mapped_input, &expected_input) {
                        return Err(Error::new("input boundaries do not match"));
                    }

                    let expected_output = Diagram::boundary_normal(Sign::Output, boundary_idx, &image)?;
                    let mapped_output = Diagram::normal(&mapped_out);
                    if !Diagram::equal(&mapped_output, &expected_output) {
                        return Err(Error::new("output boundaries do not match"));
                    }

                    let cellular = f.cellular && image.is_cell();
                    let mut new_m = f;
                    new_m.table.insert(tag.clone(), Entry { cell_data, image });
                    new_m.by_dim.entry(dim).or_default().push(tag);
                    new_m.cellular = cellular;
                    Ok(new_m)
                }
            }
        }
    }

    /// Apply partial map f to a diagram by following its paste tree structure.
    pub fn apply(f: &PMap, diagram: &Diagram) -> Result<Diagram, Error> {
        let n = if diagram.dim() < 0 { 0 } else { diagram.dim() as usize };
        let root_tree = match diagram.tree(Sign::Input, n) {
            Some(t) => t.clone(),
            None => return Err(Error::new("diagram has no tree")),
        };

        if let Some(missing) = find_undefined(f, &root_tree) {
            return Err(Error::new("diagram value outside of domain of definition")
                .with_note(format!("tag: {}", missing)));
        }

        if f.cellular {
            // Fast path: remap labels in-place
            let mut cache: HashMap<Tag, Tag> = HashMap::new();
            let top_label = |tag: &Tag, cache: &mut HashMap<Tag, Tag>| -> Tag {
                if let Some(mapped) = cache.get(tag) {
                    return mapped.clone();
                }
                let cell_diag = f.table.get(tag).map(|e| &e.image).unwrap();
                let d = cell_diag.dim();
                let d = if d < 0 { 0 } else { d as usize };
                let mapped = cell_diag.labels[d][0].clone();
                cache.insert(tag.clone(), mapped.clone());
                mapped
            };

            let new_labels: Vec<Vec<Tag>> = diagram.labels.iter().map(|level| {
                level.iter().map(|tag| top_label(tag, &mut cache)).collect()
            }).collect();

            let new_trees: Vec<[PasteTree; 2]> = diagram.trees.iter().map(|[it, ot]| {
                [map_tree(it, &cache), map_tree(ot, &cache)]
            }).collect();

            Ok(Diagram::new(diagram.shape.clone(), new_labels, new_trees))
        } else {
            apply_tree(f, &root_tree)
        }
    }

    /// Compose partial maps: g after f (g . f).
    pub fn compose(g: &PMap, f: &PMap) -> PMap {
        let mut table = HashMap::with_capacity(f.table.len());
        let mut by_dim: BTreeMap<usize, Vec<Tag>> = BTreeMap::new();
        let mut cellular = true;

        for (dim, tags) in f.domain_by_dim() {
            for tag in tags {
                let f_entry = match f.table.get(&tag) { Some(e) => e, None => continue };
                let image_gf = match PMap::apply(g, &f_entry.image) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                cellular = cellular && image_gf.is_cell();
                table.insert(tag.clone(), Entry {
                    cell_data: f_entry.cell_data.clone(),
                    image: image_gf,
                });
                by_dim.entry(dim).or_default().push(tag);
            }
        }

        PMap { table, by_dim, cellular }
    }
}


fn find_undefined<'a>(f: &PMap, tree: &'a PasteTree) -> Option<&'a Tag> {
    match tree {
        PasteTree::Leaf(tag) => {
            if f.is_defined_at(tag) { None } else { Some(tag) }
        }
        PasteTree::Node { left, right, .. } => {
            find_undefined(f, left).or_else(|| find_undefined(f, right))
        }
    }
}

fn map_tree(tree: &PasteTree, cache: &HashMap<Tag, Tag>) -> PasteTree {
    match tree {
        PasteTree::Leaf(tag) => {
            PasteTree::Leaf(cache.get(tag).cloned().unwrap_or_else(|| tag.clone()))
        }
        PasteTree::Node { dim, left, right } => PasteTree::Node {
            dim: *dim,
            left: Box::new(map_tree(left, cache)),
            right: Box::new(map_tree(right, cache)),
        },
    }
}

fn apply_tree(f: &PMap, tree: &PasteTree) -> Result<Diagram, Error> {
    match tree {
        PasteTree::Leaf(tag) => {
            f.image(tag).cloned()
        }
        PasteTree::Node { dim, left, right } => {
            let d1 = apply_tree(f, left)?;
            let d2 = apply_tree(f, right)?;
            Diagram::paste(*dim, &d1, &d2)
        }
    }
}
