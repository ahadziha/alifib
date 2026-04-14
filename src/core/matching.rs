//! Subdiagram matching: find all rewrite applications of a rule inside a target.
//!
//! The public entry point is [`find_matches`], which takes a complex, a rewrite
//! cell, and a target diagram, and returns a list of [`MatchResult`]s — each
//! containing the step diagram, rule name, and match positions.

use std::sync::Arc;
use crate::aux::Error;
use super::complex::Complex;
use super::diagram::Diagram;
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::graph::{self, DiGraph};
use super::ogposet::{self, Sign};
use super::pushout;
use super::reconstruct;

/// A confirmed match: a rewrite application producing a step diagram.
pub struct MatchResult {
    /// The (n+1)-dimensional step diagram (the "whiskered" rewrite).
    pub step: Diagram,
    /// Name of the rewrite rule (generator name).
    pub rule_name: String,
    /// Positions of the matched top-dim cells within the target diagram.
    pub image_positions: Vec<usize>,
}

/// Find all valid rewrite applications of `rewrite` inside `target`.
///
/// `rewrite` must be a cell — an (n+1)-dimensional round diagram with a single
/// top-dimensional cell — and `target` an n-dimensional diagram in `complex`.
///
/// Returns a list of [`MatchResult`]s, each containing the step diagram,
/// rule name, and match positions. The step diagram is (n+1)-dimensional:
/// its source n-boundary equals `target` and its target n-boundary is
/// `target` with the matched region replaced.
///
/// Results are sorted by `(rule_name, image_positions)` for deterministic indexing.
pub fn find_matches(
    complex: &Complex,
    rewrite: &Diagram,
    target: &Diagram,
    rule_name: &str,
) -> Result<Vec<MatchResult>, Error> {
    let n = target.top_dim();
    if rewrite.top_dim() != n + 1 {
        return Err(Error::new(format!(
            "find_matches: rewrite dim {} != target dim {} + 1",
            rewrite.top_dim(), n,
        )));
    }

    // Pattern = input n-boundary of rewrite.
    let pattern = Diagram::boundary(super::diagram::Sign::Source, n, rewrite)?;

    if pattern.top_dim() != n {
        return Err(Error::new("find_matches: pattern dimension mismatch"));
    }

    // Step 1: (n-1)-flow graphs.
    let k = if n == 0 { return find_matches_dim0(complex, rewrite, &pattern, target, rule_name); } else { n - 1 };
    let (p_flow, p_node_map) = graph::flow_graph(&pattern.shape, k);
    let (t_flow, t_node_map) = graph::flow_graph(&target.shape, k);

    // Build label arrays for the flow graph vertices (tags of the top-dim cells).
    let p_labels: Vec<&crate::aux::Tag> = p_node_map.iter()
        .map(|&(dim, pos)| &pattern.labels[dim][pos])
        .collect();
    let t_labels: Vec<&crate::aux::Tag> = t_node_map.iter()
        .map(|&(dim, pos)| &target.labels[dim][pos])
        .collect();

    // Step 2: Find all path-induced labelled subgraph matches P → T.
    let flow_matches = find_path_induced_matches(&p_flow, &t_flow, &p_labels, &t_labels);

    let mut results = Vec::new();

    for vertex_match in &flow_matches {
        // vertex_match[i] = index in t_flow that p_flow node i maps to.

        // Step 3: Restrict target to closure of matched top-cells; check isomorphism.
        let matched_cells: Vec<(usize, usize)> = vertex_match.iter()
            .map(|&ti| t_node_map[ti])
            .collect();

        // Extract image positions (top-dim positions in the target).
        let mut image_positions: Vec<usize> = matched_cells.iter()
            .filter(|(dim, _)| *dim == n)
            .map(|(_, pos)| *pos)
            .collect();
        image_positions.sort_unstable();

        let iso_emb = match check_match_isomorphism(
            &pattern, target, &matched_cells,
        ) {
            Some(e) => e,
            None => continue,
        };

        // Step 4: Pushout to build the pre-rewrite.
        let (_, pattern_to_rewrite) = ogposet::boundary(
            Sign::Input, n, &rewrite.shape,
        );

        let pushout::Pushout { tip, inl, inr } = pushout::pushout(
            &iso_emb,             // pattern → target
            &pattern_to_rewrite,  // pattern → rewrite
        );

        // Compute the induced labelling on the pushout.
        let tip_sizes = tip.sizes();
        let pre_labels = merge_pushout_labels(
            &tip_sizes, &inl, &inr, &target.labels, &rewrite.labels,
        );

        // Step 5: Reconstruct.
        match reconstruct::reconstruct(&tip, &pre_labels, complex) {
            Ok(diagram) => {
                results.push(MatchResult {
                    step: diagram,
                    rule_name: rule_name.to_owned(),
                    image_positions,
                });
            }
            Err(_) => continue,
        }
    }

    // Deterministic sort by image positions.
    results.sort_by(|a, b| a.image_positions.cmp(&b.image_positions));

    Ok(results)
}

/// Special case for dim-0 targets (points): pattern matching is trivial.
fn find_matches_dim0(
    complex: &Complex,
    rewrite: &Diagram,
    pattern: &Diagram,
    target: &Diagram,
    rule_name: &str,
) -> Result<Vec<MatchResult>, Error> {
    // A 0-dim pattern has a single point. Check label compatibility.
    let pat_sizes = pattern.shape.sizes();
    let tgt_sizes = target.shape.sizes();
    if pat_sizes.is_empty() || pat_sizes[0] != 1 {
        return Ok(vec![]);
    }
    let pat_tag = &pattern.labels[0][0];
    let mut results = Vec::new();
    for pos in 0..tgt_sizes[0] {
        if &target.labels[0][pos] != pat_tag { continue; }
        // Build the embedding and pushout.
        let map = vec![vec![pos]];
        let mut inv = vec![vec![NO_PREIMAGE; tgt_sizes[0]]];
        inv[0][pos] = 0;
        let emb = Embedding::make(
            Arc::clone(&pattern.shape), Arc::clone(&target.shape), map, inv,
        );
        let (_, pat_to_rew) = ogposet::boundary(Sign::Input, 0, &rewrite.shape);
        let pushout::Pushout { tip, inl, inr } = pushout::pushout(&emb, &pat_to_rew);
        let tip_sizes = tip.sizes();
        let pre_labels = merge_pushout_labels(
            &tip_sizes, &inl, &inr, &target.labels, &rewrite.labels,
        );
        if let Ok(diagram) = reconstruct::reconstruct(&tip, &pre_labels, complex) {
            results.push(MatchResult {
                step: diagram,
                rule_name: rule_name.to_owned(),
                image_positions: vec![pos],
            });
        }
    }
    Ok(results)
}

// ---- Path-induced labelled subgraph matching ----

/// Find all injections f: V(P) → V(T) such that:
/// - Labels match: label(f(v)) == label(v) for all v.
/// - Path-induced: for all u, v in V(P), P has edge u→v iff T has edge f(u)→f(v).
///
/// Uses backtracking with label-based candidate filtering.
fn find_path_induced_matches<L: PartialEq>(
    pattern: &DiGraph,
    target: &DiGraph,
    p_labels: &[L],
    t_labels: &[L],
) -> Vec<Vec<usize>> {
    let pn = pattern.node_count();
    let tn = target.node_count();
    if pn == 0 { return vec![vec![]]; }
    if pn > tn { return vec![]; }

    // Precompute candidate sets: for each pattern node, which target nodes
    // have the same label.
    let candidates: Vec<Vec<usize>> = (0..pn)
        .map(|pi| {
            (0..tn).filter(|&ti| t_labels[ti] == p_labels[pi]).collect()
        })
        .collect();

    // Choose a search order: start with the most constrained pattern node
    // (fewest candidates), break ties by highest degree.
    let mut order: Vec<usize> = (0..pn).collect();
    order.sort_by(|&a, &b| {
        candidates[a].len().cmp(&candidates[b].len())
            .then_with(|| {
                let deg_a = pattern.successors[a].len() + pattern.predecessors[a].len();
                let deg_b = pattern.successors[b].len() + pattern.predecessors[b].len();
                deg_b.cmp(&deg_a)
            })
    });

    let mut assignment = vec![usize::MAX; pn]; // pattern node → target node
    let mut used = vec![false; tn];
    let mut results = Vec::new();

    backtrack_subgraph(
        pattern, target, &candidates, &order, 0,
        &mut assignment, &mut used, &mut results,
    );

    results
}

/// Backtracking search for path-induced subgraph matches.
fn backtrack_subgraph(
    pattern: &DiGraph,
    target: &DiGraph,
    candidates: &[Vec<usize>],
    order: &[usize],
    depth: usize,
    assignment: &mut Vec<usize>,
    used: &mut Vec<bool>,
    results: &mut Vec<Vec<usize>>,
) {
    if depth == order.len() {
        results.push(assignment.clone());
        return;
    }

    let pi = order[depth];

    for &ti in &candidates[pi] {
        if used[ti] { continue; }

        // Check path-induced constraint against all previously assigned nodes.
        let mut ok = true;
        for d in 0..depth {
            let pj = order[d];
            let tj = assignment[pj];
            // P has edge pi→pj iff T has edge ti→tj
            let p_fwd = pattern.successors[pi].contains(&pj);
            let t_fwd = target.successors[ti].contains(&tj);
            if p_fwd != t_fwd { ok = false; break; }
            // P has edge pj→pi iff T has edge tj→ti
            let p_bwd = pattern.successors[pj].contains(&pi);
            let t_bwd = target.successors[tj].contains(&ti);
            if p_bwd != t_bwd { ok = false; break; }
        }
        if !ok { continue; }

        assignment[pi] = ti;
        used[ti] = true;

        backtrack_subgraph(pattern, target, candidates, order, depth + 1,
            assignment, used, results);

        used[ti] = false;
        assignment[pi] = usize::MAX;
    }
}

// ---- Step 3: Restrict + isomorphism check ----

/// Check that the closure of `matched_cells` in `target` is isomorphic to
/// `pattern`, including labels. Returns the composed embedding pattern → target
/// on success, or None on failure.
fn check_match_isomorphism(
    pattern: &Diagram,
    target: &Diagram,
    matched_cells: &[(usize, usize)],
) -> Option<Embedding> {
    let target_sizes = target.shape.sizes();

    // Compute the closure of matched cells in the target ogposet.
    // Group by dimension to avoid one-element vecs per cell.
    let mut by_dim: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for &(dim, pos) in matched_cells {
        by_dim.entry(dim).or_default().push(pos);
    }
    let seeds_owned: Vec<(usize, Vec<usize>)> = by_dim.into_iter().collect();
    let seeds_ref: Vec<(usize, &[usize])> = seeds_owned.iter()
        .map(|(dim, v)| (*dim, v.as_slice()))
        .collect();

    let dc = ogposet::closure(&target.shape, &seeds_ref);

    // Quick size check: the closure must have exactly the same cell counts as the pattern.
    let pat_sizes = pattern.shape.sizes();
    for (d, ps) in pat_sizes.iter().enumerate() {
        let dc_count = dc.get(d).map(|bs| bs.len()).unwrap_or(0);
        if dc_count != *ps { return None; }
    }
    // Check that pattern doesn't have more dimensions than the closure.
    for d in dc.len()..pat_sizes.len() {
        if pat_sizes[d] != 0 { return None; }
    }

    // Build the restricted sub-ogposet.
    let (sub_shape, sub_to_target) = super::reconstruct::restrict_ogposet(&target.shape, &dc);

    // Check shape isomorphism.
    let iso = ogposet::find_isomorphism(&pattern.shape, &sub_shape).ok()?;

    // Compose: pattern --iso--> sub --sub_to_target--> target.
    // Check label compatibility along the way.
    let dims = iso.map.len();
    let mut composed_map: Vec<Vec<usize>> = Vec::with_capacity(dims);
    let mut composed_inv: Vec<Vec<usize>> = target_sizes.iter()
        .map(|&s| vec![NO_PREIMAGE; s])
        .collect();

    for d in 0..dims {
        let mut row = Vec::with_capacity(iso.map[d].len());
        for (pat_pos, &sub_pos) in iso.map[d].iter().enumerate() {
            let tgt_pos = if d < sub_to_target.map.len() && sub_pos < sub_to_target.map[d].len() {
                sub_to_target.map[d][sub_pos]
            } else {
                return None;
            };
            // Label check.
            let pat_tag = pattern.labels.get(d).and_then(|r| r.get(pat_pos));
            let tgt_tag = target.labels.get(d).and_then(|r| r.get(tgt_pos));
            if pat_tag != tgt_tag { return None; }
            row.push(tgt_pos);
            if d < composed_inv.len() {
                composed_inv[d][tgt_pos] = pat_pos;
            }
        }
        composed_map.push(row);
    }

    Some(Embedding::make(
        Arc::clone(&pattern.shape),
        Arc::clone(&target.shape),
        composed_map,
        composed_inv,
    ))
}

// ---- Step 4: Merge pushout labels ----

/// Merge labels from two diagrams through a pushout.
///
/// For each cell in the pushout tip, prefer the label from the left (target)
/// injection; fall back to the right (rewrite) injection.
fn merge_pushout_labels(
    tip_sizes: &[usize],
    inl: &Embedding,     // target → tip
    inr: &Embedding,     // rewrite → tip
    left_labels: &[Vec<crate::aux::Tag>],   // target labels
    right_labels: &[Vec<crate::aux::Tag>],  // rewrite labels
) -> Vec<Vec<crate::aux::Tag>> {
    tip_sizes.iter().enumerate().map(|(d, &n)| {
        (0..n).map(|tip_pos| {
            // Try left (target) first.
            if d < inl.inv.len() && tip_pos < inl.inv[d].len() {
                let left_pos = inl.inv[d][tip_pos];
                if left_pos != NO_PREIMAGE {
                    if let Some(tag) = left_labels.get(d).and_then(|r| r.get(left_pos)) {
                        return tag.clone();
                    }
                }
            }
            // Fall back to right (rewrite).
            if d < inr.inv.len() && tip_pos < inr.inv[d].len() {
                let right_pos = inr.inv[d][tip_pos];
                if right_pos != NO_PREIMAGE {
                    if let Some(tag) = right_labels.get(d).and_then(|r| r.get(right_pos)) {
                        return tag.clone();
                    }
                }
            }
            crate::aux::Tag::Local("?".into())
        }).collect()
    }).collect()
}

#[cfg(test)]
mod tests {
    mod find_matches_tests {
        use crate::aux::{loader::Loader, Tag};
        use crate::core::complex::Complex;
        use crate::core::diagram::{CellData, Diagram, Sign};
        use crate::interpreter::InterpretedFile;
        use super::super::find_matches;
        use std::path::PathBuf;
        use std::sync::Arc;

        fn fixture(name: &str) -> String {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name)
                .to_string_lossy()
                .into_owned()
        }

        fn load_type(path: &str, type_name: &str) -> (Arc<crate::interpreter::GlobalStore>, Arc<Complex>) {
            let loader = Loader::default(vec![]);
            let file = InterpretedFile::load(&loader, path).ok().expect("fixture should load");
            let store = Arc::clone(&file.state);
            let module = store.find_module(&file.path).expect("module should exist");
            let (tag, _) = module.find_generator(type_name).expect("type not found");
            let gid = match tag { Tag::Global(gid) => *gid, _ => panic!("expected global tag") };
            let complex = store.find_type(gid).expect("type entry not found").complex.clone();
            (store, complex)
        }

        fn get_rewrite(complex: &Complex, name: &str) -> (Diagram, String) {
            let diag = complex.classifier(name)
                .unwrap_or_else(|| panic!("classifier for '{}' not found", name))
                .clone();
            (diag, name.to_owned())
        }

        fn get_diagram(complex: &Complex, name: &str) -> Diagram {
            complex.find_diagram(name).cloned()
                .unwrap_or_else(|| complex.classifier(name).cloned()
                    .unwrap_or_else(|| panic!("diagram '{}' not found", name)))
        }

        #[test]
        fn idem_whole_match() {
            // idem : id id -> id. Pattern = id id. Target = id id.
            // Should find exactly 1 match.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let target = get_diagram(&complex, "lhs"); // lhs = id id id
            // Pattern = source boundary of idem = id id.
            // In "id id id" there should be 2 matches: at positions [0,1] and [1,2].
            let matches = find_matches(&complex, &rewrite, &target, &rname).unwrap();
            assert_eq!(matches.len(), 2, "expected 2 matches of idem in id id id");
            for m in &matches {
                assert_eq!(m.step.top_dim(), target.top_dim() + 1, "each match is (n+1)-dimensional");
            }
        }

        #[test]
        fn idem_self_match() {
            // idem : id id -> id. Pattern = id id. Target = id id.
            // Should find exactly 1 match.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let n = rewrite.top_dim().saturating_sub(1);
            let idem_src = Diagram::boundary(Sign::Source, n, &rewrite).unwrap();
            let matches = find_matches(&complex, &rewrite, &idem_src, &rname).unwrap();
            assert_eq!(matches.len(), 1, "expected 1 match of idem in its own source");
        }

        #[test]
        fn idem_no_match() {
            // Target = id (single cell). Pattern = id id (two cells).
            // No match should be found.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let target = get_diagram(&complex, "rhs"); // rhs = id
            let matches = find_matches(&complex, &rewrite, &target, &rname).unwrap();
            assert_eq!(matches.len(), 0, "no match of id id in id");
        }

        #[test]
        fn assoc_dim2_matches() {
            // beta : alpha alpha -> alpha.
            // Target = lhs2 = alpha alpha alpha.
            // Should find 2 matches.
            let (_store, complex) = load_type(&fixture("Assoc.ali"), "Assoc");
            let (rewrite, rname) = get_rewrite(&complex, "beta");
            let target = get_diagram(&complex, "lhs2");
            let matches = find_matches(&complex, &rewrite, &target, &rname).unwrap();
            assert_eq!(matches.len(), 2, "expected 2 matches of beta in alpha alpha alpha");
            for m in &matches {
                assert_eq!(m.step.top_dim(), target.top_dim() + 1);
            }
        }

        #[test]
        fn idem_chain_reaches_rhs() {
            // Apply idem twice to lhs = id id id, reaching rhs = id.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let lhs = get_diagram(&complex, "lhs");
            let rhs = get_diagram(&complex, "rhs");
            let n = lhs.top_dim();

            // First application: pick first match.
            let matches1 = find_matches(&complex, &rewrite, &lhs, &rname).unwrap();
            assert!(!matches1.is_empty());
            let step1 = &matches1[0].step;
            let after1 = Diagram::boundary(Sign::Target, n, step1).unwrap();

            // Second application.
            let matches2 = find_matches(&complex, &rewrite, &after1, &rname).unwrap();
            assert_eq!(matches2.len(), 1);
            let step2 = &matches2[0].step;
            let after2 = Diagram::boundary(Sign::Target, n, step2).unwrap();

            assert!(Diagram::isomorphic(&after2, &rhs), "after two rewrites, should reach rhs");
        }
    }
}
