use std::collections::HashMap;
use std::sync::Arc;
use crate::aux::{Error, Tag};
use super::diagram::{BoundaryHistory, CellData, Diagram, PasteTree, Sign};

/// A single entry in a partial map: the source cell's boundary data and its image.
#[derive(Debug, Clone)]
struct Entry {
    cell_data: CellData,
    image: Diagram,
}

/// A partial map: a structure-preserving assignment of diagrams to generating cells.
///
/// All entries must satisfy the cellular map condition: if a cell is in the
/// domain, then every cell in its boundary is too, and images are compatible
/// with the boundary maps.
#[derive(Debug, Clone)]
pub struct PMap {
    /// Primary index: tag -> (cell data, image diagram).
    table: HashMap<Tag, Entry>,
    /// Dimension index: dim -> tags in that dimension, in insertion order.
    by_dim: HashMap<usize, Vec<Tag>>,
    /// True when every image is a single generating cell (not a composite diagram).
    /// Enables a fast in-place label-remapping path in `apply` instead of
    /// reconstructing the diagram by pasting.
    cellular: bool,
}

// ---- Public interface ----

impl PMap {
    /// Create an empty partial map.
    pub fn empty() -> PMap {
        PMap {
            table: HashMap::new(),
            by_dim: HashMap::new(),
            cellular: true,
        }
    }

    /// Build a partial map from a list of (tag, dim, cell_data, image) entries.
    pub fn of_entries(entries: Vec<(Tag, usize, CellData, Diagram)>, cellular: bool) -> PMap {
        let mut table = HashMap::with_capacity(entries.len());
        let mut by_dim: HashMap<usize, Vec<Tag>> = HashMap::new();
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

    /// Return all (dim, tags) pairs sorted by dimension, tags in insertion order.
    pub fn domain_by_dim(&self) -> Vec<(usize, Vec<Tag>)> {
        let mut result: Vec<(usize, Vec<Tag>)> = self.by_dim.iter()
            .map(|(&d, tags)| (d, tags.clone()))
            .collect();
        result.sort_by_key(|(d, _)| *d);
        result
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

        let cellular = match (dim, &cell_data) {
            (0, CellData::Zero) => f.cellular,
            (0, CellData::Boundary { .. }) => {
                return Err(Error::new("0-cell cannot have boundary data"))
            }
            (_, CellData::Zero) => {
                return Err(Error::new("higher-dimensional cell has no boundary data"))
            }
            (_, CellData::Boundary { boundary_in, boundary_out }) => {
                let k = dim - 1;
                check_boundary_match(&f, boundary_in, Sign::Source, k, &image)?;
                check_boundary_match(&f, boundary_out, Sign::Target, k, &image)?;
                f.cellular && image.is_cell()
            }
        };

        let mut new_m = f;
        new_m.cellular = cellular;
        new_m.table.insert(tag.clone(), Entry { cell_data, image });
        new_m.by_dim.entry(dim).or_default().push(tag);
        Ok(new_m)
    }

    /// Apply partial map f to a diagram by following its paste tree structure.
    pub fn apply(f: &PMap, diagram: &Diagram) -> Result<Diagram, Error> {
        let n = diagram.top_dim();
        let root_tree = match diagram.tree(Sign::Source, n) {
            Some(t) => t.clone(),
            None => return Err(Error::new("diagram has no tree")),
        };

        if let Some(missing) = find_undefined(f, &root_tree) {
            return Err(Error::new("diagram value outside of domain of definition")
                .with_note(format!("tag: {}", missing)));
        }

        if f.cellular {
            // Fast path: since every image is a single cell, we can remap labels in-place
            // without reconstructing the diagram by pasting.
            let mut cache: HashMap<Tag, Tag> = HashMap::new();
            let new_labels: Vec<Vec<Tag>> = diagram.labels.iter().map(|level| {
                level.iter().map(|tag| remap_tag(tag, &f.table, &mut cache)).collect()
            }).collect();

            let new_trees: Vec<BoundaryHistory> = diagram.paste_history.iter().map(|h| {
                BoundaryHistory::from_pair(
                    map_tree(&h.source, &cache),
                    map_tree(&h.target, &cache),
                )
            }).collect();

            Ok(Diagram::make(diagram.shape.clone(), new_labels, new_trees))
        } else {
            apply_tree(f, &root_tree)
        }
    }

    /// Compose partial maps: g after f (g . f).
    pub fn compose(g: &PMap, f: &PMap) -> PMap {
        let mut table = HashMap::with_capacity(f.table.len());
        let mut by_dim: HashMap<usize, Vec<Tag>> = HashMap::new();
        let mut cellular = true;

        for (dim, tags) in f.domain_by_dim() {
            for tag in tags {
                let Some(f_entry) = f.table.get(&tag) else { continue };
                let Ok(image_gf) = PMap::apply(g, &f_entry.image) else { continue };
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


// ---- Internal helpers ----

/// Apply `f` to `boundary_diag` and verify the result equals the `sign`-boundary
/// of `image` at dimension `k` (after normalisation).
fn check_boundary_match(
    f: &PMap,
    boundary_diag: &Diagram,
    sign: Sign,
    k: usize,
    image: &Diagram,
) -> Result<(), Error> {
    let mapped = PMap::apply(f, boundary_diag)?;
    let expected = Diagram::boundary_normal(sign, k, image)?;
    if Diagram::equal(&Diagram::normal(&mapped), &expected) {
        Ok(())
    } else {
        Err(Error::new(match sign {
            Sign::Source => "input boundaries do not match",
            Sign::Target => "output boundaries do not match",
        }))
    }
}

/// Look up the top label of a tag's image in the map, using `cache` to avoid
/// repeated lookups. Panics if `tag` is not in `table` — callers must ensure
/// all tags are in the domain (verified by `find_undefined` before this runs).
fn remap_tag(tag: &Tag, table: &HashMap<Tag, Entry>, cache: &mut HashMap<Tag, Tag>) -> Tag {
    if let Some(hit) = cache.get(tag) {
        return hit.clone();
    }
    let entry = table.get(tag).expect("tag in domain (verified by find_undefined)");
    let d = entry.image.top_dim();
    let mapped = entry.image.labels[d][0].clone();
    cache.insert(tag.clone(), mapped.clone());
    mapped
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
            left: Arc::new(map_tree(left, cache)),
            right: Arc::new(map_tree(right, cache)),
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
