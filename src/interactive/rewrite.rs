//! Core rewrite logic: finding candidate rewrites and applying them.

use crate::core::complex::Complex;
use crate::core::diagram::Diagram;
use crate::interpreter::GlobalStore;

/// A single candidate rewrite at the current diagram.
pub struct CandidateRewrite {
    /// Name of the (n+1)-generator being used as a rule.
    pub rule_name: String,
    /// The source boundary of the rule (the pattern being matched).
    pub source_boundary: Diagram,
    /// The target boundary of the rule (the replacement).
    pub target_boundary: Diagram,
    /// Top-dimensional forward map from the embedding, used as a sort key
    /// to ensure deterministic candidate ordering across CLI calls.
    sort_key: Vec<usize>,
}

/// Find all candidate rewrites applicable to `current` using the generators
/// of `type_complex` at dimension `current.top_dim() + 1`.
///
/// Results are sorted by `(rule_name, top-dimensional embedding map)` for
/// deterministic indexing across CLI calls.
///
/// # Phase 1 limitation
/// Only whole-diagram rewrites are returned — i.e. where the source boundary
/// of the rule is isomorphic to the entire current diagram. Partial rewrites
/// (whiskering) are not yet implemented.
pub fn find_candidate_rewrites(
    store: &GlobalStore,
    type_complex: &Complex,
    current: &Diagram,
) -> Vec<CandidateRewrite> {
    let n = current.top_dim();
    let mut candidates = Vec::new();

    for (name, tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 {
            continue;
        }
        let Some(cell_data) = store.cell_data_for_tag(type_complex, tag) else {
            continue;
        };
        let crate::core::diagram::CellData::Boundary { boundary_in, boundary_out } = cell_data
        else {
            continue;
        };

        let Ok(embeddings) = Diagram::find_subdiagrams(&boundary_in, current) else {
            continue;
        };

        // Phase 1: only whole-diagram matches (source boundary ≅ current).
        // TODO: support partial rewrites via whiskering with identity cells.
        if !Diagram::isomorphic(&boundary_in, current) {
            continue;
        }

        for emb in embeddings {
            let sort_key = emb.map.last().cloned().unwrap_or_default();
            candidates.push(CandidateRewrite {
                rule_name: name.clone(),
                source_boundary: (*boundary_in).clone(),
                target_boundary: (*boundary_out).clone(),
                sort_key,
            });
        }
    }

    // Stable deterministic sort: by rule name, then by the top-dimensional
    // embedding map (lexicographic). This ensures that embedding_index values
    // stored in session files are stable across CLI invocations.
    candidates.sort_by(|a, b| {
        a.rule_name.cmp(&b.rule_name).then_with(|| a.sort_key.cmp(&b.sort_key))
    });

    candidates
}

/// Apply a rewrite to the current diagram, returning the new current diagram.
///
/// # Phase 1 limitation
/// Only whole-diagram rewrites are supported: `S_r ≅ current`, so the
/// result is simply the target boundary of the rule.
pub fn apply_rewrite(candidate: &CandidateRewrite) -> Diagram {
    // Phase 1: when the match covers the whole diagram, the next current
    // diagram is simply the target boundary of the rule.
    // TODO: when partial rewrites are supported, build the composite (n+1)-cell
    // by whiskering the rule with identity cells on the context, then extract
    // the target n-boundary.
    candidate.target_boundary.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::{loader::Loader, Tag};
    use crate::core::diagram::CellData;
    use crate::interpreter::InterpretedFile;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn fixture(name: &str) -> String {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    fn load_type(path: &str, type_name: &str) -> (Arc<crate::interpreter::GlobalStore>, Arc<crate::core::complex::Complex>) {
        let loader = Loader::default(vec![]);
        let file = InterpretedFile::load(&loader, path).ok().expect("fixture should load");
        let store = Arc::clone(&file.state);

        let module = store.find_module(&file.path).expect("module should exist");
        let (type_tag, _) = module.find_generator(type_name)
            .unwrap_or_else(|| panic!("type '{}' not found in module", type_name));

        let gid = match type_tag {
            Tag::Global(gid) => *gid,
            Tag::Local(_) => panic!("expected global tag"),
        };

        let complex = store.find_type(gid)
            .unwrap_or_else(|| panic!("type entry '{}' not found", type_name))
            .complex.clone();

        (store, complex)
    }

    fn gen_boundaries(
        store: &crate::interpreter::GlobalStore,
        complex: &crate::core::complex::Complex,
        name: &str,
    ) -> (Diagram, Diagram) {
        let (tag, _) = complex.find_generator(name)
            .unwrap_or_else(|| panic!("generator '{}' not found", name));
        match store.cell_data_for_tag(complex, tag).expect("cell data should exist") {
            CellData::Boundary { boundary_in, boundary_out } => {
                ((*boundary_in).clone(), (*boundary_out).clone())
            }
            CellData::Zero => panic!("generator '{}' is 0-dim, no boundaries", name),
        }
    }

    #[test]
    fn find_rewrites_whole_diagram_match() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, _) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(&store, &complex, &idem_src);

        assert_eq!(candidates.len(), 1, "expected exactly one candidate");
        assert_eq!(candidates[0].rule_name, "idem");
        assert!(Diagram::isomorphic(&candidates[0].source_boundary, &idem_src));
    }

    #[test]
    fn find_rewrites_no_match_when_source_differs() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");

        // `idem` has source `id id` (2 cells). If current is `id` (1 cell),
        // isomorphic check fails → no candidates.
        let id_classifier = complex.classifier("id").expect("id classifier").clone();

        let candidates = find_candidate_rewrites(&store, &complex, &id_classifier);
        assert_eq!(candidates.len(), 0, "no match when source boundary ≇ current");
    }

    #[test]
    fn apply_rewrite_returns_target_boundary() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, idem_tgt) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(&store, &complex, &idem_src);
        assert_eq!(candidates.len(), 1);

        let result = apply_rewrite(&candidates[0]);
        assert!(Diagram::equal(&result, &idem_tgt));
    }

    #[test]
    fn after_rewrite_no_further_matches() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, idem_tgt) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(&store, &complex, &idem_src);
        let after = apply_rewrite(&candidates[0]);
        assert!(Diagram::equal(&after, &idem_tgt));

        // `id` (1 cell) ≇ `id id` (2 cells), so idem no longer matches.
        let next_candidates = find_candidate_rewrites(&store, &complex, &after);
        assert_eq!(next_candidates.len(), 0);
    }
}
