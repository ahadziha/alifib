//! Core rewrite logic: finding candidate rewrites and applying them.

use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram};
use crate::interpreter::GlobalStore;

/// A single candidate rewrite at the current diagram.
pub struct CandidateRewrite {
    /// Name of the (n+1)-generator being used as a rule.
    pub rule_name: String,
    /// The source boundary of the rule (the pattern being matched).
    pub source_boundary: Diagram,
    /// The target boundary of the rule (the replacement).
    pub target_boundary: Diagram,
    /// Positions of the pattern's top-dim cells within the current diagram.
    /// Used as a sort key for deterministic ordering and to locate the
    /// rewrite site for partial matches.
    pub image_positions: Vec<usize>,
}

/// Find all candidate rewrites applicable to `current` using the generators
/// of `type_complex` at dimension `current.top_dim() + 1`.
///
/// Results are sorted by `(rule_name, image_positions)` for
/// deterministic indexing across CLI calls.
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
        let CellData::Boundary { boundary_in, boundary_out } = cell_data else {
            continue;
        };

        let Ok(embeddings) = Diagram::find_subdiagrams(&boundary_in, current) else {
            continue;
        };

        for emb in embeddings {
            let image_positions = emb.map.last().cloned().unwrap_or_default();
            candidates.push(CandidateRewrite {
                rule_name: name.clone(),
                source_boundary: (*boundary_in).clone(),
                target_boundary: (*boundary_out).clone(),
                image_positions,
            });
        }
    }

    // Stable deterministic sort: by rule name, then by image positions
    // (lexicographic). This ensures that choice indices stored in session
    // files are stable across CLI invocations.
    candidates.sort_by(|a, b| {
        a.rule_name
            .cmp(&b.rule_name)
            .then_with(|| a.image_positions.cmp(&b.image_positions))
    });

    candidates
}

/// Apply a rewrite to the current diagram, returning the new current diagram.
///
/// For whole-diagram rewrites (`image_positions` covers all top-dim cells),
/// the result is simply the target boundary of the rule.
///
/// For partial rewrites on 1-dim diagrams, the current diagram is split into
/// a prefix, the rule's target boundary, and a suffix, then pasted sequentially.
///
/// Returns `Err` for unsupported cases (dim > 1 partial rewrites).
pub fn apply_rewrite(
    store: &GlobalStore,
    type_complex: &Complex,
    current: &Diagram,
    candidate: &CandidateRewrite,
) -> Result<Diagram, String> {
    let n = current.top_dim();
    let labels = current.labels_at(n).unwrap_or(&[]);

    // Fast path: whole-diagram match.
    if candidate.image_positions.len() == labels.len() {
        return Ok(candidate.target_boundary.clone());
    }

    // Partial rewrite: only supported for 1-dim diagrams.
    if n == 0 {
        return Err("partial rewrite on a 0-dim diagram is not possible".to_string());
    }
    if n > 1 {
        return Err(format!(
            "partial rewrites for {}-dim diagrams are not yet supported \
             (requires flow graph decomposition)",
            n
        ));
    }

    // 1-dim partial rewrite: split current into prefix + T + suffix, paste at dim 0.
    let start = *candidate.image_positions.iter().min().unwrap();
    let end = *candidate.image_positions.iter().max().unwrap() + 1;

    // Build single-cell diagrams for each atom outside the match.
    let mut parts: Vec<Diagram> = Vec::new();

    for tag in labels[..start].iter() {
        let data = store
            .cell_data_for_tag(type_complex, tag)
            .ok_or_else(|| format!("cell data not found for prefix tag {:?}", tag))?;
        let atom = Diagram::cell(tag.clone(), &data)
            .map_err(|e| format!("failed to reconstruct prefix atom: {:?}", e))?;
        parts.push(atom);
    }

    parts.push(candidate.target_boundary.clone());

    for tag in labels[end..].iter() {
        let data = store
            .cell_data_for_tag(type_complex, tag)
            .ok_or_else(|| format!("cell data not found for suffix tag {:?}", tag))?;
        let atom = Diagram::cell(tag.clone(), &data)
            .map_err(|e| format!("failed to reconstruct suffix atom: {:?}", e))?;
        parts.push(atom);
    }

    // Sequential paste of all parts at dim 0.
    let mut result = parts.remove(0);
    for next in parts {
        result = Diagram::paste(0, &result, &next)
            .map_err(|e| format!("paste failed during partial rewrite: {:?}", e))?;
    }

    Ok(result)
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

    fn load_type(
        path: &str,
        type_name: &str,
    ) -> (Arc<crate::interpreter::GlobalStore>, Arc<crate::core::complex::Complex>) {
        let loader = Loader::default(vec![]);
        let file = InterpretedFile::load(&loader, path).ok().expect("fixture should load");
        let store = Arc::clone(&file.state);

        let module = store.find_module(&file.path).expect("module should exist");
        let (type_tag, _) = module
            .find_generator(type_name)
            .unwrap_or_else(|| panic!("type '{}' not found in module", type_name));

        let gid = match type_tag {
            Tag::Global(gid) => *gid,
            Tag::Local(_) => panic!("expected global tag"),
        };

        let complex = store
            .find_type(gid)
            .unwrap_or_else(|| panic!("type entry '{}' not found", type_name))
            .complex
            .clone();

        (store, complex)
    }

    fn gen_boundaries(
        store: &crate::interpreter::GlobalStore,
        complex: &crate::core::complex::Complex,
        name: &str,
    ) -> (Diagram, Diagram) {
        let (tag, _) = complex
            .find_generator(name)
            .unwrap_or_else(|| panic!("generator '{}' not found", name));
        match store.cell_data_for_tag(complex, tag).expect("cell data should exist") {
            CellData::Boundary { boundary_in, boundary_out } => {
                ((*boundary_in).clone(), (*boundary_out).clone())
            }
            CellData::Zero => panic!("generator '{}' is 0-dim, no boundaries", name),
        }
    }

    fn load_diagram(
        store: &crate::interpreter::GlobalStore,
        complex: &crate::core::complex::Complex,
        name: &str,
    ) -> Diagram {
        complex
            .find_diagram(name)
            .cloned()
            .unwrap_or_else(|| panic!("diagram '{}' not found", name))
    }

    // ── Phase 1 (whole-diagram) tests ────────────────────────────────────────

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
        // no embedding exists — so zero candidates.
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

        let result = apply_rewrite(&store, &complex, &idem_src, &candidates[0]).unwrap();
        assert!(Diagram::equal(&result, &idem_tgt));
    }

    #[test]
    fn after_rewrite_no_further_matches() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, idem_tgt) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(&store, &complex, &idem_src);
        let after = apply_rewrite(&store, &complex, &idem_src, &candidates[0]).unwrap();
        assert!(Diagram::equal(&after, &idem_tgt));

        // `id` (1 cell) has no subdiagram matching `id id` (2 cells).
        let next_candidates = find_candidate_rewrites(&store, &complex, &after);
        assert_eq!(next_candidates.len(), 0);
    }

    // ── Phase 2 (partial / whiskered) tests ──────────────────────────────────

    #[test]
    fn find_rewrites_partial_match_two_candidates() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        // lhs = id id id  (3 cells)
        let lhs = load_diagram(&store, &complex, "lhs");

        let candidates = find_candidate_rewrites(&store, &complex, &lhs);

        // idem (source: id id, 2 cells) can match at positions [0,1] or [1,2].
        assert_eq!(candidates.len(), 2, "expected two partial-match candidates");
        assert_eq!(candidates[0].rule_name, "idem");
        assert_eq!(candidates[0].image_positions, vec![0, 1]);
        assert_eq!(candidates[1].rule_name, "idem");
        assert_eq!(candidates[1].image_positions, vec![1, 2]);
    }

    #[test]
    fn apply_partial_rewrite_prefix() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");
        let (_, idem_tgt) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(&store, &complex, &lhs);
        // choice 0: image_positions = [0, 1]  →  result should be `idem_tgt ; id` = id id
        let result = apply_rewrite(&store, &complex, &lhs, &candidates[0]).unwrap();

        // Result has 2 top-dim cells.
        assert_eq!(result.top_dim(), 1);
        assert_eq!(result.labels_at(1).map(|l| l.len()), Some(2));

        // It is still rewritable (idem matches the resulting id id).
        let next = find_candidate_rewrites(&store, &complex, &result);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn apply_partial_rewrite_suffix() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");

        let candidates = find_candidate_rewrites(&store, &complex, &lhs);
        // choice 1: image_positions = [1, 2]  →  result should be `id ; idem_tgt` = id id
        let result = apply_rewrite(&store, &complex, &lhs, &candidates[1]).unwrap();

        assert_eq!(result.top_dim(), 1);
        assert_eq!(result.labels_at(1).map(|l| l.len()), Some(2));

        let next = find_candidate_rewrites(&store, &complex, &result);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn apply_partial_rewrite_chain_to_rhs() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");
        let rhs = load_diagram(&store, &complex, "rhs");

        // Step 1: apply idem at [0,1] on id id id → id id
        let candidates1 = find_candidate_rewrites(&store, &complex, &lhs);
        let after1 = apply_rewrite(&store, &complex, &lhs, &candidates1[0]).unwrap();

        // Step 2: apply idem at [0,1] on id id → id
        let candidates2 = find_candidate_rewrites(&store, &complex, &after1);
        assert_eq!(candidates2.len(), 1);
        let after2 = apply_rewrite(&store, &complex, &after1, &candidates2[0]).unwrap();

        // Result equals rhs (= id).
        assert!(Diagram::equal(&after2, &rhs));

        // No further rewrites.
        let final_cands = find_candidate_rewrites(&store, &complex, &after2);
        assert_eq!(final_cands.len(), 0);
    }
}
