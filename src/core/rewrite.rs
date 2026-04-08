//! Core rewrite logic: finding candidate rewrites and applying them.
//!
//! This module provides the pure mathematical operations for finding and
//! applying rewrite steps. It depends only on `core` and `aux` — the
//! `GlobalStore` dependency is abstracted away via a closure, avoiding a
//! `core` → `interpreter` dependency cycle.

use crate::aux::Tag;
use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram};
use crate::core::Embedding;

/// A single candidate rewrite at the current diagram.
pub struct CandidateRewrite {
    /// Name of the (n+1)-generator being used as a rule.
    pub rule_name: String,
    /// Tag of the (n+1)-generator.
    pub rule_tag: Tag,
    /// The source boundary of the rule (the pattern being matched).
    pub source_boundary: Diagram,
    /// The target boundary of the rule (the replacement).
    pub target_boundary: Diagram,
    /// Positions of the pattern's top-dim cells within the current diagram.
    /// Used as a sort key for deterministic ordering.
    pub image_positions: Vec<usize>,
    /// Full embedding ι: source_boundary.shape → current.shape, from subdiagram matching.
    pub match_embedding: Embedding,
}

/// Find all candidate rewrites applicable to `current` using the generators
/// of `type_complex` at dimension `current.top_dim() + 1`.
///
/// The `cell_data_for_tag` closure resolves a generator tag to its `CellData`
/// within the given complex. Pass `|cx, tag| store.cell_data_for_tag(cx, tag)`.
///
/// Results are sorted by `(rule_name, image_positions)` for deterministic
/// indexing across CLI calls and daemon sessions.
pub fn find_candidate_rewrites(
    cell_data_for_tag: impl Fn(&Complex, &Tag) -> Option<CellData>,
    type_complex: &Complex,
    current: &Diagram,
) -> Vec<CandidateRewrite> {
    let n = current.top_dim();
    let mut candidates = Vec::new();

    for (name, tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 {
            continue;
        }
        let Some(cell_data) = cell_data_for_tag(type_complex, tag) else {
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
                rule_tag: tag.clone(),
                source_boundary: (*boundary_in).clone(),
                target_boundary: (*boundary_out).clone(),
                image_positions,
                match_embedding: emb,
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

/// Apply a rewrite to the current diagram, returning the whiskered (n+1)-dimensional step.
///
/// The result S has:
/// - Source n-boundary = `current`
/// - Target n-boundary = `current` with the matched subdiagram replaced by the rule's target
/// - One interior (n+1)-cell: the whiskered rule application
///
/// Works uniformly for all dimensions.
pub fn apply_rewrite(
    current: &Diagram,
    candidate: &CandidateRewrite,
) -> Result<Diagram, String> {
    Diagram::whisker_rewrite(
        current,
        &candidate.match_embedding,
        &candidate.rule_tag,
        &candidate.source_boundary,
        &candidate.target_boundary,
    )
    .map_err(|e| format!("whisker rewrite failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::{loader::Loader, Tag};
    use crate::core::diagram::{CellData, Sign};
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
        _store: &crate::interpreter::GlobalStore,
        complex: &crate::core::complex::Complex,
        name: &str,
    ) -> Diagram {
        complex
            .find_diagram(name)
            .cloned()
            .unwrap_or_else(|| panic!("diagram '{}' not found", name))
    }

    fn step_boundaries(step: &Diagram) -> (Diagram, Diagram) {
        let n = step.top_dim() - 1;
        let src = Diagram::boundary(Sign::Source, n, step)
            .expect("source boundary extraction failed");
        let tgt = Diagram::boundary(Sign::Target, n, step)
            .expect("target boundary extraction failed");
        (src, tgt)
    }

    fn cell_data_fn<'a>(
        store: &'a crate::interpreter::GlobalStore,
    ) -> impl Fn(&Complex, &Tag) -> Option<CellData> + 'a {
        |cx, tag| store.cell_data_for_tag(cx, tag)
    }

    // ── Phase 1 (whole-diagram) tests ────────────────────────────────────────

    #[test]
    fn find_rewrites_whole_diagram_match() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, _) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &idem_src);

        assert_eq!(candidates.len(), 1, "expected exactly one candidate");
        assert_eq!(candidates[0].rule_name, "idem");
        assert!(Diagram::isomorphic(&candidates[0].source_boundary, &idem_src));
    }

    #[test]
    fn find_rewrites_no_match_when_source_differs() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let id_classifier = complex.classifier("id").expect("id classifier").clone();
        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &id_classifier);
        assert_eq!(candidates.len(), 0, "no match when source boundary ≇ current");
    }

    #[test]
    fn apply_rewrite_returns_step_with_correct_boundaries() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, idem_tgt) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &idem_src);
        assert_eq!(candidates.len(), 1);

        let step = apply_rewrite(&idem_src, &candidates[0]).unwrap();

        assert_eq!(step.top_dim(), idem_src.top_dim() + 1);

        let (src, tgt) = step_boundaries(&step);
        assert!(Diagram::isomorphic(&src, &idem_src), "source boundary should equal current");
        assert!(Diagram::isomorphic(&tgt, &idem_tgt), "target boundary should equal rule target");
    }

    #[test]
    fn after_rewrite_no_further_matches() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let (idem_src, idem_tgt) = gen_boundaries(&store, &complex, "idem");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &idem_src);
        let step = apply_rewrite(&idem_src, &candidates[0]).unwrap();
        let n = idem_src.top_dim();
        let after = Diagram::boundary(Sign::Target, n, &step).unwrap();

        assert!(Diagram::isomorphic(&after, &idem_tgt));

        let next_candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &after);
        assert_eq!(next_candidates.len(), 0);
    }

    // ── Phase 2 (partial / whiskered) tests ──────────────────────────────────

    #[test]
    fn find_rewrites_partial_match_two_candidates() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs);

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

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs);
        let step = apply_rewrite(&lhs, &candidates[0]).unwrap();

        assert_eq!(step.top_dim(), lhs.top_dim() + 1);

        let (src, tgt) = step_boundaries(&step);
        assert!(Diagram::isomorphic(&src, &lhs), "source = lhs");

        let n = lhs.top_dim();
        assert_eq!(tgt.top_dim(), n);
        assert_eq!(tgt.labels_at(n).map(|l| l.len()), Some(2));

        let next = find_candidate_rewrites(cell_data_fn(&store), &complex, &tgt);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn apply_partial_rewrite_suffix() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs);
        let step = apply_rewrite(&lhs, &candidates[1]).unwrap();

        let (src, tgt) = step_boundaries(&step);
        assert!(Diagram::isomorphic(&src, &lhs));

        let n = lhs.top_dim();
        assert_eq!(tgt.labels_at(n).map(|l| l.len()), Some(2));

        let next = find_candidate_rewrites(cell_data_fn(&store), &complex, &tgt);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn apply_partial_rewrite_chain_to_rhs() {
        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");
        let rhs = load_diagram(&store, &complex, "rhs");

        let n = lhs.top_dim();

        let candidates1 = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs);
        let step1 = apply_rewrite(&lhs, &candidates1[0]).unwrap();
        let after1 = Diagram::boundary(Sign::Target, n, &step1).unwrap();

        let candidates2 = find_candidate_rewrites(cell_data_fn(&store), &complex, &after1);
        assert_eq!(candidates2.len(), 1);
        let step2 = apply_rewrite(&after1, &candidates2[0]).unwrap();
        let after2 = Diagram::boundary(Sign::Target, n, &step2).unwrap();

        assert!(Diagram::isomorphic(&after2, &rhs));

        let composed = Diagram::paste(n, &step1, &step2)
            .expect("composed steps should be pasteable at dim n");
        assert_eq!(composed.top_dim(), n + 1);

        let final_cands = find_candidate_rewrites(cell_data_fn(&store), &complex, &after2);
        assert_eq!(final_cands.len(), 0);
    }

    // ── Phase 3 (2-dim partial rewrites producing 3-dim cells) ───────────────

    #[test]
    fn find_2dim_rewrites_two_candidates() {
        let (store, complex) = load_type(&fixture("Assoc.ali"), "Assoc");
        let lhs2 = load_diagram(&store, &complex, "lhs2");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs2);

        assert_eq!(candidates.len(), 2, "expected two 2-dim candidates");
        assert_eq!(candidates[0].rule_name, "beta");
        assert_eq!(candidates[0].image_positions, vec![0, 1]);
        assert_eq!(candidates[1].rule_name, "beta");
        assert_eq!(candidates[1].image_positions, vec![1, 2]);
    }

    #[test]
    fn apply_2dim_rewrite_returns_3dim_step() {
        let (store, complex) = load_type(&fixture("Assoc.ali"), "Assoc");
        let lhs2 = load_diagram(&store, &complex, "lhs2");

        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs2);
        assert_eq!(candidates.len(), 2);

        let step = apply_rewrite(&lhs2, &candidates[0]).unwrap();

        assert_eq!(step.top_dim(), lhs2.top_dim() + 1);

        let (src, tgt) = step_boundaries(&step);
        assert!(Diagram::isomorphic(&src, &lhs2), "source = lhs2");

        let n = lhs2.top_dim();
        assert_eq!(tgt.top_dim(), n);
        assert_eq!(tgt.labels_at(n).map(|l| l.len()), Some(2));

        let next = find_candidate_rewrites(cell_data_fn(&store), &complex, &tgt);
        assert_eq!(next.len(), 1);
    }

    #[test]
    fn apply_2dim_rewrite_chain_to_rhs() {
        let (store, complex) = load_type(&fixture("Assoc.ali"), "Assoc");
        let lhs2 = load_diagram(&store, &complex, "lhs2");
        let rhs2 = load_diagram(&store, &complex, "rhs2");

        let n = lhs2.top_dim();

        let candidates1 = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs2);
        let step1 = apply_rewrite(&lhs2, &candidates1[0]).unwrap();
        assert_eq!(step1.top_dim(), n + 1);
        let after1 = Diagram::boundary(Sign::Target, n, &step1).unwrap();

        let candidates2 = find_candidate_rewrites(cell_data_fn(&store), &complex, &after1);
        assert_eq!(candidates2.len(), 1);
        let step2 = apply_rewrite(&after1, &candidates2[0]).unwrap();
        assert_eq!(step2.top_dim(), n + 1);
        let after2 = Diagram::boundary(Sign::Target, n, &step2).unwrap();

        assert!(Diagram::isomorphic(&after2, &rhs2));

        let composed = Diagram::paste(n, &step1, &step2)
            .expect("3-dim steps should be pasteable at dim n");
        assert_eq!(composed.top_dim(), n + 1);

        let final_cands = find_candidate_rewrites(cell_data_fn(&store), &complex, &after2);
        assert_eq!(final_cands.len(), 0);
    }

    // ── Sourcefier tests ─────────────────────────────────────────────────────

    /// Verify that applying idem at positions [0,1] in `id id id` produces a step whose
    /// paste tree sourcefies to "idem #0 id" (rule whiskered with one idle cell on the right).
    #[test]
    fn whisker_rewrite_step_sourcefies_correctly() {
        use crate::output::diagram_to_source;

        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");

        // candidates[0] matches at positions [0, 1] — idem whiskered with id on the right.
        let candidates = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs);
        assert_eq!(candidates[0].image_positions, vec![0, 1]);

        let step = apply_rewrite(&lhs, &candidates[0]).unwrap();
        let source_expr = diagram_to_source(&step, &complex);
        assert_eq!(source_expr, "idem #0 id");
    }

    /// Verify that composing two partial rewrites (lhs → id id → id) produces a proof whose
    /// paste tree sourcefies to "idem #0 id #1 idem" — valid .ali syntax for the Idem proof.
    #[test]
    fn composed_proof_sourcefies_correctly() {
        use crate::output::diagram_to_source;

        let (store, complex) = load_type(&fixture("Idem.ali"), "Idem");
        let lhs = load_diagram(&store, &complex, "lhs");
        let n = lhs.top_dim();

        let candidates1 = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs);
        let step1 = apply_rewrite(&lhs, &candidates1[0]).unwrap();
        let after1 = Diagram::boundary(Sign::Target, n, &step1).unwrap();

        let candidates2 = find_candidate_rewrites(cell_data_fn(&store), &complex, &after1);
        let step2 = apply_rewrite(&after1, &candidates2[0]).unwrap();

        let proof = Diagram::paste(n, &step1, &step2).expect("steps should paste");
        let source_expr = diagram_to_source(&proof, &complex);

        // Left-associative: (idem #0 id) #1 idem
        assert_eq!(source_expr, "idem #0 id #1 idem");
    }

    /// Verify that the 3-dim Assoc proof (lhs2 → alpha alpha → alpha) sourcefies correctly.
    /// beta is a 3-cell: beta : alpha alpha -> alpha.  Applying beta at [0,1] in lhs2 = alpha alpha alpha
    /// gives a step whose source tree is "beta #1 alpha".
    #[test]
    fn assoc_proof_sourcefies_correctly() {
        use crate::output::diagram_to_source;

        let (store, complex) = load_type(&fixture("Assoc.ali"), "Assoc");
        let lhs2 = load_diagram(&store, &complex, "lhs2");
        let n = lhs2.top_dim();

        // candidates[0]: beta at [0,1] — whiskered with alpha on the right
        let candidates1 = find_candidate_rewrites(cell_data_fn(&store), &complex, &lhs2);
        assert_eq!(candidates1[0].image_positions, vec![0, 1]);
        let step1 = apply_rewrite(&lhs2, &candidates1[0]).unwrap();
        let step1_expr = diagram_to_source(&step1, &complex);
        assert_eq!(step1_expr, "beta #1 alpha");

        let after1 = Diagram::boundary(Sign::Target, n, &step1).unwrap();
        let candidates2 = find_candidate_rewrites(cell_data_fn(&store), &complex, &after1);
        let step2 = apply_rewrite(&after1, &candidates2[0]).unwrap();

        let proof = Diagram::paste(n, &step1, &step2).expect("steps should paste");
        let proof_expr = diagram_to_source(&proof, &complex);
        assert_eq!(proof_expr, "beta #1 alpha #2 beta");
    }
}
