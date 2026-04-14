//! Reconstruction of a [`Diagram`] from a pre-diagram (ogposet + labels).
//!
//! Given an ogposet with a tag-labelling and a [`Complex`] that defines the
//! generators, [`reconstruct`] tries to find a paste tree that, when realised,
//! produces a diagram isomorphic to the pre-diagram.
//!
//! The algorithm works by computing the *layering dimension* of the ogposet,
//! building a topological sort of the maximal flow graph at that dimension,
//! decomposing into layers, and recursing on each layer.

use std::sync::Arc;

use crate::aux::{Error, Tag};
use crate::core::bitset::BitSet;
use super::complex::Complex;
use super::diagram::{Diagram, PasteTree};
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::graph;
use super::intset::{self, IntSet};
use super::ogposet::{self, Ogposet, Sign};

/// A pre-diagram: an ogposet with labels but no paste history.
struct PreDiagram {
    shape: Arc<Ogposet>,
    labels: Vec<Vec<Tag>>,
}

/// Reconstruct a [`Diagram`] from a pre-diagram (ogposet + labels) and a complex.
///
/// Returns `Ok(diagram)` if a paste tree can be found that realises to a diagram
/// whose shape has the same sizes as the pre-diagram, or `Err` if no valid
/// reconstruction exists.
pub fn reconstruct(
    shape: &Arc<Ogposet>,
    labels: &[Vec<Tag>],
    complex: &Complex,
) -> Result<Diagram, Error> {
    let pd = PreDiagram {
        shape: Arc::clone(shape),
        labels: labels.to_vec(),
    };
    let tree = build_paste_tree(&pd, complex)?;
    let diagram = Diagram::realise_tree(&tree, complex)?;
    check_sizes(&pd, &diagram)?;
    Ok(diagram)
}

/// Check that the realised diagram has the same cell counts as the pre-diagram.
fn check_sizes(pd: &PreDiagram, diagram: &Diagram) -> Result<(), Error> {
    let pd_sizes = pd.shape.sizes();
    let d_sizes = diagram.shape_sizes();
    if pd_sizes != d_sizes {
        return Err(Error::new(format!(
            "reconstruction size mismatch: pre-diagram has {:?}, realised has {:?}",
            pd_sizes, d_sizes,
        )));
    }
    Ok(())
}

/// Build a candidate paste tree for a pre-diagram.
fn build_paste_tree(
    pd: &PreDiagram,
    complex: &Complex,
) -> Result<PasteTree, Error> {
    let k = pd.shape.layering_dimension();

    // Base case: layering dimension -1 means a single top element.
    if k == -1 {
        return leaf_for_top_element(pd);
    }

    let k = k as usize;
    let (mf_graph, node_map) = graph::maximal_flow_graph(&pd.shape, k);

    // Enumerate topological sorts.
    let sorts = match graph::all_topological_sorts(&mf_graph, Some(10_000)) {
        graph::TopoSortResult::Sorts(s) if !s.is_empty() => s,
        graph::TopoSortResult::HasCycle => {
            return Err(Error::new("reconstruction failed: maxflow graph has a cycle"));
        }
        graph::TopoSortResult::LimitExceeded => {
            return Err(Error::new("reconstruction failed: too many topological sorts"));
        }
        _ => {
            return Err(Error::new("reconstruction failed: no topological sorts"));
        }
    };

    if pd.shape.dim > 3 {
        // dim > 3: try each topological sort; on failure, try the next.
        for sort in &sorts {
            let tree = try_sort(pd, complex, &node_map, sort, k);
            if let Ok(t) = tree {
                match Diagram::realise_tree(&t, complex) {
                    Ok(d) if check_sizes(pd, &d).is_ok() => return Ok(t),
                    _ => continue,
                }
            }
        }
        Err(Error::new("reconstruction failed: all topological sorts exhausted"))
    } else {
        // dim <= 3: the topological sort is unique up to rewriting, so no backtracking.
        let sort = &sorts[0];
        try_sort(pd, complex, &node_map, sort, k)
    }
}

/// Try to build a paste tree from a specific topological sort.
fn try_sort(
    pd: &PreDiagram,
    complex: &Complex,
    node_map: &[(usize, usize)],
    sort: &[usize],
    k: usize,
) -> Result<PasteTree, Error> {
    let m = sort.len();
    if m == 0 {
        return Err(Error::new("reconstruction failed: empty topological sort"));
    }

    // Build layers and recursively compute paste trees.
    let layers = build_layers(pd, node_map, sort, k)?;
    let mut trees: Vec<PasteTree> = Vec::with_capacity(m);
    for layer in &layers {
        trees.push(build_paste_tree(layer, complex)?);
    }

    // Left-associate: paste(k, t1, paste(k, t2, paste(k, t3, ...)))
    // Actually: paste(k, paste(k, paste(k, t1, t2), t3), ...)
    let mut combined = trees.remove(0);
    for t in trees {
        combined = PasteTree::Node {
            dim: k,
            left: Arc::new(combined),
            right: Arc::new(t),
        };
    }

    Ok(combined)
}

/// Build a paste tree leaf for a pre-diagram with a single top element.
fn leaf_for_top_element(pd: &PreDiagram) -> Result<PasteTree, Error> {
    if pd.shape.dim < 0 {
        return Err(Error::new("reconstruction failed: empty ogposet"));
    }
    let d = pd.shape.dim as usize;
    let sizes = pd.shape.sizes();
    let n_top = sizes.get(d).copied().unwrap_or(0);
    // There should be exactly one maximal element at the top dimension,
    // but the layering_dimension == -1 condition guarantees at most 1 maximal
    // element at dim > 0. The top element might be the only top-dim cell.
    if n_top == 0 {
        return Err(Error::new("reconstruction failed: no top-dimensional cells"));
    }
    // Find the single maximal element (could be at any dimension >= 1).
    // With layering_dimension -1, there's at most 1 maximal element at dim >= 1.
    // Check if it's at the top dimension.
    let maximal_top = pd.shape.maximal(d);
    if maximal_top.len() == 1 {
        let pos = maximal_top[0];
        let tag = pd.labels.get(d)
            .and_then(|row| row.get(pos))
            .ok_or_else(|| Error::new("reconstruction failed: missing label for top element"))?;
        return Ok(PasteTree::Leaf(tag.clone()));
    }
    // The maximal element might be at a lower dimension (non-pure ogposet).
    for dim in (1..d).rev() {
        let maximal_d = pd.shape.maximal(dim);
        if maximal_d.len() == 1 {
            let pos = maximal_d[0];
            let tag = pd.labels.get(dim)
                .and_then(|row| row.get(pos))
                .ok_or_else(|| Error::new("reconstruction failed: missing label"))?;
            return Ok(PasteTree::Leaf(tag.clone()));
        }
    }
    // Dimension 0: single point
    if sizes[0] == 1 {
        let tag = pd.labels.get(0)
            .and_then(|row| row.first())
            .ok_or_else(|| Error::new("reconstruction failed: missing label for point"))?;
        return Ok(PasteTree::Leaf(tag.clone()));
    }
    Err(Error::new("reconstruction failed: could not find top element"))
}

/// Build the m layers from a topological sort of the maxflow graph.
///
/// - layer_1 = input_k_boundary(u) ∪ downset(x_1)
/// - layer_i = output_k_boundary(layer_{i-1}) ∪ downset(x_i)  for i > 1
fn build_layers(
    pd: &PreDiagram,
    node_map: &[(usize, usize)],
    sort: &[usize],
    k: usize,
) -> Result<Vec<PreDiagram>, Error> {
    let g = &pd.shape;
    let sizes = g.sizes();
    let max_dim = if g.dim < 0 { 0 } else { g.dim as usize };

    // Precompute downsets for each node in the sort.
    let downsets: Vec<Vec<BitSet>> = sort.iter().map(|&ni| {
        let (dim, pos) = node_map[ni];
        ogposet::closure(g, &[(dim, &[pos])])
    }).collect();

    // Compute the input k-boundary cell set.
    let (_, in_bd_emb) = ogposet::boundary(Sign::Input, k, g);
    let in_bd_cells = embedding_to_bitsets(&in_bd_emb, &sizes);

    let mut layers = Vec::with_capacity(sort.len());

    // Track the "running output k-boundary" — starts as input k-boundary of u.
    let mut prev_boundary_cells = in_bd_cells;

    for downset in &downsets {
        // Union: previous boundary ∪ downset of x_i
        let layer_cells = union_bitsets(&prev_boundary_cells, downset, max_dim);

        // Restrict the ogposet and labels to this cell set.
        let (layer_shape, layer_emb) = restrict_ogposet(g, &layer_cells);
        let layer_labels = pullback_labels(&pd.labels, &layer_emb);
        let layer_pd = PreDiagram { shape: layer_shape.clone(), labels: layer_labels };

        // Compute the output k-boundary of this layer for the next iteration.
        let (_, out_bd_emb) = ogposet::boundary(Sign::Output, k, &layer_shape);
        // Map layer-local boundary indices back to u-indices via composition.
        prev_boundary_cells = compose_embedding_to_bitsets(&out_bd_emb, &layer_emb, &sizes);

        layers.push(layer_pd);
    }

    Ok(layers)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Convert an embedding's map to a Vec<BitSet> of the codomain cells that are hit.
fn embedding_to_bitsets(emb: &Embedding, cod_sizes: &[usize]) -> Vec<BitSet> {
    let mut result: Vec<BitSet> = cod_sizes.iter()
        .map(|&n| BitSet::new(n))
        .collect();
    for (dim, row) in emb.map.iter().enumerate() {
        if dim < result.len() {
            for &pos in row {
                result[dim].insert(pos);
            }
        }
    }
    result
}

/// Compose two embeddings (A → B → C) to get the C-indices of cells in A,
/// returned as BitSets over C's sizes.
fn compose_embedding_to_bitsets(
    inner: &Embedding,    // A → B
    outer: &Embedding,    // B → C
    c_sizes: &[usize],
) -> Vec<BitSet> {
    let mut result: Vec<BitSet> = c_sizes.iter()
        .map(|&n| BitSet::new(n))
        .collect();
    for (dim, inner_row) in inner.map.iter().enumerate() {
        if let Some(outer_row) = outer.map.get(dim) {
            for &b_pos in inner_row {
                if let Some(&c_pos) = outer_row.get(b_pos) {
                    if dim < result.len() {
                        result[dim].insert(c_pos);
                    }
                }
            }
        }
    }
    result
}

/// Union two Vec<BitSet>s, extending to cover up to max_dim.
fn union_bitsets(a: &[BitSet], b: &[BitSet], max_dim: usize) -> Vec<BitSet> {
    (0..=max_dim).map(|d| {
        match (a.get(d), b.get(d)) {
            (Some(x), Some(y)) => x.union(y),
            (Some(x), None) => x.clone(),
            (None, Some(y)) => y.clone(),
            (None, None) => BitSet::new(0),
        }
    }).collect()
}

/// Restrict an ogposet to the cells indicated by the BitSets.
///
/// Returns the sub-ogposet and an embedding mapping sub-indices to parent indices.
pub(super) fn restrict_ogposet(g: &Arc<Ogposet>, keep: &[BitSet]) -> (Arc<Ogposet>, Embedding) {
    if g.dim < 0 {
        return (Arc::new(Ogposet::empty()), Embedding::empty(Arc::clone(g)));
    }
    let gd = g.dim as usize;
    let sizes_g = g.sizes();

    // Find the effective top dimension of the restriction.
    let mut top_dim: isize = -1;
    for d in (0..=gd).rev() {
        if d < keep.len() && keep[d].len() > 0 {
            top_dim = d as isize;
            break;
        }
    }
    if top_dim < 0 {
        return (Arc::new(Ogposet::empty()), Embedding::empty(Arc::clone(g)));
    }
    let td = top_dim as usize;

    // Build forward map (new index → old index) and inverse map.
    let dims = td + 1;
    let mut forward: Vec<Vec<usize>> = Vec::with_capacity(dims);
    let mut inv: Vec<Vec<usize>> = Vec::with_capacity(sizes_g.len());
    for d in 0..sizes_g.len() {
        inv.push(vec![NO_PREIMAGE; sizes_g[d]]);
    }

    for d in 0..dims {
        let mut fwd_d = Vec::new();
        if d < keep.len() {
            for old in keep[d].iter() {
                let new_idx = fwd_d.len();
                inv[d][old] = new_idx;
                fwd_d.push(old);
            }
        }
        forward.push(fwd_d);
    }

    // Build face/coface tables for the restricted ogposet.
    let mut faces_in: Vec<Vec<IntSet>> = Vec::with_capacity(dims);
    let mut faces_out: Vec<Vec<IntSet>> = Vec::with_capacity(dims);
    let mut cofaces_in: Vec<Vec<IntSet>> = Vec::with_capacity(dims);
    let mut cofaces_out: Vec<Vec<IntSet>> = Vec::with_capacity(dims);

    for d in 0..dims {
        let n = forward[d].len();
        let mut fi_d = Vec::with_capacity(n);
        let mut fo_d = Vec::with_capacity(n);

        for &old in &forward[d] {
            if d > 0 {
                fi_d.push(remap_set(&g.faces_in[d][old], &inv[d - 1]));
                fo_d.push(remap_set(&g.faces_out[d][old], &inv[d - 1]));
            } else {
                fi_d.push(vec![]);
                fo_d.push(vec![]);
            }
        }
        faces_in.push(fi_d);
        faces_out.push(fo_d);

        // Cofaces: map from g's cofaces, but only keep those in the restriction.
        let mut ci_d = Vec::with_capacity(n);
        let mut co_d = Vec::with_capacity(n);
        for &old in &forward[d] {
            if d < td {
                ci_d.push(remap_set(&g.cofaces_in[d][old], &inv[d + 1]));
                co_d.push(remap_set(&g.cofaces_out[d][old], &inv[d + 1]));
            } else {
                ci_d.push(vec![]);
                co_d.push(vec![]);
            }
        }
        cofaces_in.push(ci_d);
        cofaces_out.push(co_d);
    }

    let sub = Arc::new(Ogposet::make(
        top_dim, faces_in, faces_out, cofaces_in, cofaces_out,
    ));

    // Build full-size inv for the embedding (pad with NO_PREIMAGE for dims above td).
    let emb = Embedding::make(Arc::clone(&sub), Arc::clone(g), forward, inv);
    (sub, emb)
}

/// Remap an IntSet through an inverse map, dropping entries that map to NO_PREIMAGE.
fn remap_set(set: &IntSet, inv: &[usize]) -> IntSet {
    intset::collect_sorted(
        set.iter().filter_map(|&x| {
            let y = inv.get(x).copied().unwrap_or(NO_PREIMAGE);
            (y != NO_PREIMAGE).then_some(y)
        })
    )
}

/// Pull back labels through an embedding.
fn pullback_labels(labels: &[Vec<Tag>], emb: &Embedding) -> Vec<Vec<Tag>> {
    emb.map.iter().enumerate().map(|(dim, row)| {
        row.iter().map(|&old_pos| {
            labels.get(dim)
                .and_then(|r| r.get(old_pos))
                .cloned()
                .unwrap_or_else(|| Tag::Local("?".into()))
        }).collect()
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::loader::Loader;
    use crate::interpreter::InterpretedFile;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn load_type(path: &str, type_name: &str) -> Arc<Complex> {
        let loader = Loader::default(vec![]);
        let file = InterpretedFile::load(&loader, path).ok().expect("fixture should load");
        let store = Arc::clone(&file.state);
        let module = store.find_module(&file.path).expect("module should exist");
        let (tag, _) = module.find_generator(type_name).expect("type not found");
        let gid = match tag { Tag::Global(gid) => *gid, _ => panic!("expected global tag") };
        store.find_type(gid).expect("type entry not found").complex.clone()
    }

    /// Reconstruct a diagram from its ogposet + labels and verify isomorphism.
    fn assert_reconstruct(diagram: &Diagram, complex: &Complex, label: &str) {
        let result = reconstruct(&diagram.shape, &diagram.labels, complex);
        match result {
            Ok(reconstructed) => {
                assert!(
                    Diagram::isomorphic(diagram, &reconstructed),
                    "{}: reconstructed diagram is not isomorphic to original\n  \
                     original sizes: {:?}\n  reconstructed sizes: {:?}",
                    label,
                    diagram.shape_sizes(),
                    reconstructed.shape_sizes(),
                );
            }
            Err(e) => panic!("{}: reconstruction failed: {}", label, e),
        }
    }

    /// Reconstruct every generator classifier and named diagram in a type.
    fn assert_reconstruct_all_in_type(path: &str, type_name: &str) {
        let complex = load_type(path, type_name);
        for (name, _, _) in complex.generators_iter() {
            if let Some(diag) = complex.classifier(name) {
                assert_reconstruct(diag, &complex, &format!("{}.{}", type_name, name));
            }
        }
        for (name, diag) in complex.diagrams_iter() {
            assert_reconstruct(diag, &complex, &format!("{}.let.{}", type_name, name));
        }
    }

    // ── Individual tests (targeted) ──────────────────────────────────────

    #[test]
    fn reconstruct_0cell() {
        let complex = load_type(&fixture("Idem.ali"), "Idem");
        let ob = complex.classifier("ob").expect("ob classifier");
        assert_reconstruct(ob, &complex, "ob");
    }

    #[test]
    fn reconstruct_single_cell_dim1() {
        let complex = load_type(&fixture("Idem.ali"), "Idem");
        let id_diag = complex.classifier("id").expect("id classifier");
        assert_reconstruct(id_diag, &complex, "id");
    }

    #[test]
    fn reconstruct_composite_dim1() {
        let complex = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = complex.find_diagram("lhs").expect("lhs diagram");
        assert_reconstruct(lhs, &complex, "lhs");
    }

    #[test]
    fn reconstruct_single_cell_dim2() {
        let complex = load_type(&fixture("Idem.ali"), "Idem");
        let idem = complex.classifier("idem").expect("idem classifier");
        assert_reconstruct(idem, &complex, "idem");
    }

    #[test]
    fn reconstruct_composite_dim2() {
        let complex = load_type(&fixture("Assoc.ali"), "Assoc");
        let lhs2 = complex.find_diagram("lhs2").expect("lhs2 diagram");
        assert_reconstruct(lhs2, &complex, "lhs2");
    }

    #[test]
    fn reconstruct_single_cell_dim3() {
        let complex = load_type(&fixture("Assoc.ali"), "Assoc");
        let beta = complex.classifier("beta").expect("beta classifier");
        assert_reconstruct(beta, &complex, "beta");
    }

    #[test]
    fn reconstruct_generator_with_composite_boundary() {
        let complex = load_type(&fixture("Magma.ali"), "Magma");
        let m = complex.classifier("m").expect("m classifier");
        assert_reconstruct(m, &complex, "m");
    }

    // ── Exhaustive per-type tests ────────────────────────────────────────

    #[test]
    fn reconstruct_all_idem() {
        assert_reconstruct_all_in_type(&fixture("Idem.ali"), "Idem");
    }

    #[test]
    fn reconstruct_all_assoc() {
        assert_reconstruct_all_in_type(&fixture("Assoc.ali"), "Assoc");
    }

    #[test]
    fn reconstruct_all_magma() {
        assert_reconstruct_all_in_type(&fixture("Magma.ali"), "Magma");
    }

    fn example(name: &str) -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn load_all_types(path: &str) -> Vec<(String, Arc<Complex>)> {
        let loader = Loader::default(vec![]);
        let file = InterpretedFile::load(&loader, path).ok().expect("should load");
        let store = Arc::clone(&file.state);
        let norm = store.normalize();
        let mut result = Vec::new();
        for module in &norm.modules {
            if let Some(mc) = store.find_module(&module.path) {
                for ty in &module.types {
                    if ty.name.is_empty() { continue; }
                    if let Some((tag, _)) = mc.find_generator(&ty.name) {
                        if let Tag::Global(gid) = tag {
                            if let Some(entry) = store.find_type(*gid) {
                                result.push((ty.name.clone(), Arc::clone(&entry.complex)));
                            }
                        }
                    }
                }
            }
        }
        result
    }

    #[test]
    fn reconstruct_all_category() {
        for (type_name, complex) in load_all_types(&example("Category.ali")) {
            for (name, _, _) in complex.generators_iter() {
                if let Some(diag) = complex.classifier(name) {
                    assert_reconstruct(diag, &complex, &format!("{}.{}", type_name, name));
                }
            }
            for (name, diag) in complex.diagrams_iter() {
                assert_reconstruct(diag, &complex, &format!("{}.let.{}", type_name, name));
            }
        }
    }

    #[test]
    fn reconstruct_all_semigroup() {
        for (type_name, complex) in load_all_types(&example("Semigroup.ali")) {
            for (name, _, _) in complex.generators_iter() {
                if let Some(diag) = complex.classifier(name) {
                    assert_reconstruct(diag, &complex, &format!("{}.{}", type_name, name));
                }
            }
            for (name, diag) in complex.diagrams_iter() {
                assert_reconstruct(diag, &complex, &format!("{}.let.{}", type_name, name));
            }
        }
    }

    #[test]
    fn reconstruct_all_total() {
        for (type_name, complex) in load_all_types(&example("Total.ali")) {
            for (name, _, _) in complex.generators_iter() {
                if let Some(diag) = complex.classifier(name) {
                    assert_reconstruct(diag, &complex, &format!("{}.{}", type_name, name));
                }
            }
            for (name, diag) in complex.diagrams_iter() {
                assert_reconstruct(diag, &complex, &format!("{}.let.{}", type_name, name));
            }
        }
    }
}
