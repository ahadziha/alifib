//! Subdiagram matching: find all rewrite applications of a rule inside a target.
//!
//! The public entry point is [`find_matches`], which takes a complex, a rewrite
//! cell, and a target diagram, and returns a list of [`MatchResult`]s — each
//! containing the step diagram, rule name, and match positions.

use std::collections::HashMap;
use std::sync::Arc;
use crate::aux::Error;
use super::complex::Complex;
use super::diagram::Diagram;
use super::embeddings::{Embedding, NO_PREIMAGE};
use super::graph::{self, DiGraph};
use super::intset;
use super::ogposet::{self, Sign};
use super::pushout;
use super::reconstruct;

/// Common interface for match-like types (full matches and lightweight candidates).
pub(crate) trait MatchLike {
    fn rule_name(&self) -> &str;
    fn image_positions(&self) -> &[usize];
    fn iso_emb(&self) -> &Embedding;
}

/// A confirmed match: a rewrite application producing a step diagram.
pub struct MatchResult {
    /// The (n+1)-dimensional step diagram (the "whiskered" rewrite).
    pub step: Diagram,
    /// Name of the rewrite rule (generator name).
    pub rule_name: String,
    /// Positions of the matched top-dim cells within the target diagram.
    pub image_positions: Vec<usize>,
    /// The embedding `pattern → target` from the isomorphism check, retained
    /// so that parallel rewrite composition can reuse it for iterated pushouts.
    pub(crate) iso_emb: Embedding,
}

impl MatchLike for MatchResult {
    fn rule_name(&self) -> &str { &self.rule_name }
    fn image_positions(&self) -> &[usize] { &self.image_positions }
    fn iso_emb(&self) -> &Embedding { &self.iso_emb }
}

/// A lightweight candidate match: isomorphism confirmed but no step constructed.
pub(crate) struct CandidateMatch {
    pub(crate) rule_name: String,
    pub(crate) image_positions: Vec<usize>,
    pub(crate) iso_emb: Embedding,
}

impl MatchLike for CandidateMatch {
    fn rule_name(&self) -> &str { &self.rule_name }
    fn image_positions(&self) -> &[usize] { &self.image_positions }
    fn iso_emb(&self) -> &Embedding { &self.iso_emb }
}

/// A confirmed parallel rewrite: a compatible family of matches applied simultaneously.
pub struct ParallelMatchResult {
    /// The (n+1)-dimensional parallel step diagram (multiple rewrites composed).
    pub step: Diagram,
    /// Indices into the individual match list that form this family.
    pub family: Vec<usize>,
    /// Union of all family members' matched top-dim positions.
    pub image_positions: Vec<usize>,
}

/// Precomputed, reusable data about a rewrite rule's pattern side.
///
/// Builds once per rule and reused across every [`find_matches`] call so that
/// long-running sessions don't redo the rule-side boundary/normalisation work
/// on every step.  The pattern's shape is kept in canonical form, so the
/// isomorphism check inside matching becomes a normalise-and-compare where the
/// rule side is already normal.
pub struct RulePattern {
    /// The normalised input (source) n-boundary of the rule, as a diagram.
    pub pattern: Diagram,
    /// Embedding of [`pattern.shape`] into the rule's full (n+1) shape.
    /// Used as the right injection in the pushout that builds each step.
    pub(crate) pattern_to_rewrite: Embedding,
}

impl RulePattern {
    /// Build a [`RulePattern`] from a rewrite rule's classifier diagram.
    ///
    /// The rule must be at least 1-dimensional (i.e. `rewrite.top_dim() >= 1`);
    /// the pattern is the rule's input boundary at dimension `rewrite.top_dim() - 1`.
    pub fn new(rewrite: &Diagram) -> Result<Self, Error> {
        let top = rewrite.top_dim();
        if rewrite.dim() < 1 {
            return Err(Error::new(
                "RulePattern::new: rewrite must be at least 1-dimensional",
            ));
        }
        let n = top - 1;
        // Normalised input boundary diagram (pattern.shape is in canonical form).
        let pattern = Diagram::boundary_normal(super::diagram::Sign::Source, n, rewrite)?;
        // Embedding from the normalised boundary sub-shape into the rule's shape.
        // `boundary_traverse` is deterministic so calling it again yields the
        // same cell ordering that `boundary_normal` used above.
        let (_, pattern_to_rewrite) =
            ogposet::boundary_traverse(Sign::Input, n, &rewrite.shape);
        Ok(Self { pattern, pattern_to_rewrite })
    }
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
pub fn find_matches(
    complex: &Complex,
    rewrite: &Diagram,
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
) -> Result<Vec<MatchResult>, Error> {
    find_matches_impl(complex, rewrite, rule, target, rule_name, None)
}

/// Find up to `limit` matches (`None` = all).
pub(crate) fn find_matches_impl(
    complex: &Complex,
    rewrite: &Diagram,
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
    limit: Option<usize>,
) -> Result<Vec<MatchResult>, Error> {
    let mut results = Vec::new();
    for_each_candidate(rewrite, rule, target, rule_name, |cand| {
        let pushout::Pushout { tip, inl, inr } = pushout::pushout(
            &cand.iso_emb, &rule.pattern_to_rewrite,
        );
        let tip_sizes = tip.sizes();
        let pre_labels = merge_pushout_labels(
            &tip_sizes, &inl, &inr, &target.labels, &rewrite.labels,
        );
        if let Ok(diagram) = reconstruct::reconstruct(&tip, &pre_labels, complex) {
            results.push(MatchResult {
                step: diagram,
                rule_name: cand.rule_name,
                image_positions: cand.image_positions,
                iso_emb: cand.iso_emb,
            });
            if limit.is_some_and(|l| results.len() >= l) { return true; }
        }
        false
    })?;
    Ok(results)
}

/// Find all candidate matches (isomorphism only, no pushout/reconstruct).
pub(crate) fn find_candidate_matches(
    rewrite: &Diagram,
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
) -> Result<Vec<CandidateMatch>, Error> {
    let mut results = Vec::new();
    for_each_candidate(rewrite, rule, target, rule_name, |cand| {
        results.push(cand);
        false
    })?;
    Ok(results)
}

/// Iterate over candidate matches, calling `f` for each. Stops early if `f`
/// returns `true`.  Shared implementation for both full and candidate matching.
fn for_each_candidate<F>(
    rewrite: &Diagram,
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
    mut f: F,
) -> Result<(), Error>
where
    F: FnMut(CandidateMatch) -> bool,
{
    let n = target.top_dim();
    if rewrite.top_dim() != n + 1 {
        return Err(Error::new(format!(
            "find_matches: rewrite dim {} != target dim {} + 1",
            rewrite.top_dim(), n,
        )));
    }

    let pattern = &rule.pattern;

    if pattern.top_dim() != n {
        return Err(Error::new("find_matches: pattern dimension mismatch"));
    }

    if n == 0 {
        return for_each_candidate_dim0(rule, target, rule_name, &mut f);
    }

    // Step 1: (n-1)-flow graphs.
    let k = n - 1;
    let (p_flow, p_node_map) = graph::flow_graph(&pattern.shape, k);
    let (t_flow, t_node_map) = graph::flow_graph(&target.shape, k);

    let p_labels: Vec<&crate::aux::Tag> = p_node_map.iter()
        .map(|&(dim, pos)| &pattern.labels[dim][pos])
        .collect();
    let t_labels: Vec<&crate::aux::Tag> = t_node_map.iter()
        .map(|&(dim, pos)| &target.labels[dim][pos])
        .collect();

    // Step 2: Find all path-induced labelled subgraph matches P → T.
    let flow_matches = find_path_induced_matches(&p_flow, &t_flow, &p_labels, &t_labels);

    for vertex_match in &flow_matches {
        // Step 3: Restrict target to closure of matched top-cells; check isomorphism.
        let matched_cells: Vec<(usize, usize)> = vertex_match.iter()
            .map(|&ti| t_node_map[ti])
            .collect();

        let mut image_positions: Vec<usize> = matched_cells.iter()
            .filter(|(dim, _)| *dim == n)
            .map(|(_, pos)| *pos)
            .collect();
        image_positions.sort_unstable();

        let iso_emb = match check_match_isomorphism(pattern, target, &matched_cells) {
            Some(e) => e,
            None => continue,
        };

        if f(CandidateMatch {
            rule_name: rule_name.to_owned(),
            image_positions,
            iso_emb,
        }) {
            return Ok(());
        }
    }

    Ok(())
}

/// Dim-0 candidate iteration: label matching only.
fn for_each_candidate_dim0<F>(
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
    f: &mut F,
) -> Result<(), Error>
where
    F: FnMut(CandidateMatch) -> bool,
{
    let pattern = &rule.pattern;
    let pat_sizes = pattern.shape.sizes();
    let tgt_sizes = target.shape.sizes();
    if pat_sizes.is_empty() || pat_sizes[0] != 1 {
        return Ok(());
    }
    let pat_tag = &pattern.labels[0][0];
    for pos in 0..tgt_sizes[0] {
        if &target.labels[0][pos] != pat_tag { continue; }
        let map = vec![vec![pos]];
        let mut inv = vec![vec![NO_PREIMAGE; tgt_sizes[0]]];
        inv[0][pos] = 0;
        let emb = Embedding::make(
            Arc::clone(&pattern.shape), Arc::clone(&target.shape), map, inv,
        );
        if f(CandidateMatch {
            rule_name: rule_name.to_owned(),
            image_positions: vec![pos],
            iso_emb: emb,
        }) {
            return Ok(());
        }
    }
    Ok(())
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
    inl: &Embedding,
    inr: &Embedding,
    left_labels: &[Vec<crate::aux::Tag>],
    right_labels: &[Vec<crate::aux::Tag>],
) -> Vec<Vec<crate::aux::Tag>> {
    tip_sizes.iter().enumerate().map(|(d, &n)| {
        (0..n).map(|tip_pos| {
            if d < inl.inv.len() && tip_pos < inl.inv[d].len() {
                let left_pos = inl.inv[d][tip_pos];
                if left_pos != NO_PREIMAGE {
                    if let Some(tag) = left_labels.get(d).and_then(|r| r.get(left_pos)) {
                        return tag.clone();
                    }
                }
            }
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

// ---- Parallel rewrite matching ----

/// Construct the parallel rewrite step for a compatible family of matches.
///
/// Delegates the ogposet-level colimit to [`pushout::multi_pushout`], then
/// merges labels from the target and rewrite diagrams.
pub(crate) fn construct_parallel_step<M: MatchLike>(
    complex: &Complex,
    target: &Diagram,
    matches: &[M],
    family: &[usize],
    rule_patterns: &HashMap<String, RulePattern>,
) -> Result<Diagram, Error> {
    let mut rewrites: Vec<&Diagram> = Vec::with_capacity(family.len());
    let mut spans: Vec<pushout::Span> = Vec::with_capacity(family.len());
    for &idx in family {
        let m = &matches[idx];
        let rp = rule_patterns.get(m.rule_name()).ok_or_else(|| {
            Error::new(format!("rule pattern for '{}' not found", m.rule_name()))
        })?;
        let rewrite = complex.classifier(m.rule_name()).ok_or_else(|| {
            Error::new(format!("classifier for '{}' not found", m.rule_name()))
        })?;
        spans.push(pushout::Span {
            into_base: m.iso_emb(),
            into_ext: &rp.pattern_to_rewrite,
        });
        rewrites.push(rewrite);
    }

    let mp = pushout::multi_pushout(&target.shape, &spans);
    let tip_sizes = mp.tip.sizes();
    let base_sizes = target.shape.sizes();

    let labels: Vec<Vec<crate::aux::Tag>> = tip_sizes.iter().enumerate().map(|(d, &n)| {
        (0..n).map(|pos| {
            if pos < base_sizes.get(d).copied().unwrap_or(0) {
                return target.labels.get(d)
                    .and_then(|r| r.get(pos))
                    .cloned()
                    .unwrap_or_else(|| crate::aux::Tag::Local("?".into()));
            }
            for (i, inr) in mp.inrs.iter().enumerate() {
                if d < inr.inv.len() && pos < inr.inv[d].len() {
                    let ext_pos = inr.inv[d][pos];
                    if ext_pos != NO_PREIMAGE {
                        if let Some(tag) = rewrites[i].labels.get(d).and_then(|r| r.get(ext_pos)) {
                            return tag.clone();
                        }
                    }
                }
            }
            crate::aux::Tag::Local("?".into())
        }).collect()
    }).collect();

    reconstruct::reconstruct(&mp.tip, &labels, complex)
}

/// Find all compatible families of matches (parallel rewrite candidates).
///
/// A candidate compatible family is a subset of matches whose `image_positions`
/// are pairwise disjoint (top-dim cells only; lower-dim boundaries may overlap).
/// Families are ordered by size descending, then lexicographically on indices.
///
/// Each candidate is verified by constructing the iterated pushout and running
/// reconstruction. If `first_only` is set, enumerates lazily in size-descending
/// order and returns as soon as the first compatible family (of size ≥ 2) is
/// verified, avoiding the exponential cost of materialising all families.
pub(crate) fn find_compatible_families<M: MatchLike>(
    matches: &[M],
    complex: &Complex,
    target: &Diagram,
    rule_patterns: &HashMap<String, RulePattern>,
    first_only: bool,
) -> Vec<ParallelMatchResult> {
    let n = matches.len();
    if n < 2 { return vec![]; }

    // Build conflict relation: matches i,j conflict iff image_positions overlap.
    let conflicts: Vec<Vec<bool>> = (0..n).map(|i| {
        (0..n).map(|j| {
            if i == j { return false; }
            !intset::is_disjoint(matches[i].image_positions(), matches[j].image_positions())
        }).collect()
    }).collect();

    if first_only {
        let k_max = max_independent_set_size(&conflicts, n);
        if k_max < 2 { return vec![]; }

        for k in (2..=k_max).rev() {
            let mut prefix: Vec<usize> = Vec::new();
            let mut result: Option<ParallelMatchResult> = None;
            enumerate_independent_sets_of_size(
                &conflicts, n, &mut prefix, 0, k,
                &mut |family| {
                    match try_family(matches, complex, target, rule_patterns, family) {
                        Some(r) => { result = Some(r); true }
                        None => false,
                    }
                },
            );
            if let Some(r) = result {
                return vec![r];
            }
        }
        return vec![];
    }

    // Full enumeration for interactive mode.
    let mut candidates: Vec<Vec<usize>> = Vec::new();
    let mut prefix: Vec<usize> = Vec::new();
    enumerate_independent_sets(&conflicts, n, &mut prefix, 0, &mut candidates);

    // Order: size descending, then lexicographic.
    candidates.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    let mut results = Vec::new();
    for family in &candidates {
        match try_family(matches, complex, target, rule_patterns, family) {
            Some(r) => results.push(r),
            None => continue,
        }
    }

    results
}

fn try_family<M: MatchLike>(
    matches: &[M],
    complex: &Complex,
    target: &Diagram,
    rule_patterns: &HashMap<String, RulePattern>,
    family: &[usize],
) -> Option<ParallelMatchResult> {
    let image_positions = {
        let mut all: Vec<usize> = family.iter()
            .flat_map(|&i| matches[i].image_positions().iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    };
    match construct_parallel_step(complex, target, matches, family, rule_patterns) {
        Ok(step) => Some(ParallelMatchResult {
            step,
            family: family.to_vec(),
            image_positions,
        }),
        Err(_) => None,
    }
}

/// Find the size of the maximum independent set in the conflict graph.
fn max_independent_set_size(conflicts: &[Vec<bool>], n: usize) -> usize {
    let mut max_size = 0;
    let mut prefix: Vec<usize> = Vec::new();
    max_is_dfs(conflicts, n, &mut prefix, 0, &mut max_size);
    max_size
}

fn max_is_dfs(
    conflicts: &[Vec<bool>],
    n: usize,
    prefix: &mut Vec<usize>,
    start: usize,
    max_size: &mut usize,
) {
    if prefix.len() > *max_size {
        *max_size = prefix.len();
    }
    let remaining = (start..n)
        .filter(|&i| !prefix.iter().any(|&j| conflicts[i][j]))
        .count();
    if prefix.len() + remaining <= *max_size { return; }
    for i in start..n {
        if prefix.iter().any(|&j| conflicts[i][j]) { continue; }
        prefix.push(i);
        max_is_dfs(conflicts, n, prefix, i + 1, max_size);
        prefix.pop();
    }
}

/// Enumerate independent sets of exactly `target_size` in lex order, calling
/// `callback` for each. Stops early if `callback` returns `true`.
fn enumerate_independent_sets_of_size(
    conflicts: &[Vec<bool>],
    n: usize,
    prefix: &mut Vec<usize>,
    start: usize,
    target_size: usize,
    callback: &mut impl FnMut(&[usize]) -> bool,
) -> bool {
    if prefix.len() == target_size {
        return callback(prefix);
    }
    let remaining_needed = target_size - prefix.len();
    for i in start..n {
        if n - i < remaining_needed { break; }
        if prefix.iter().any(|&j| conflicts[i][j]) { continue; }
        prefix.push(i);
        if enumerate_independent_sets_of_size(conflicts, n, prefix, i + 1, target_size, callback) {
            return true;
        }
        prefix.pop();
    }
    false
}

/// Recursively enumerate independent sets of size >= 2 in the conflict graph.
fn enumerate_independent_sets(
    conflicts: &[Vec<bool>],
    n: usize,
    prefix: &mut Vec<usize>,
    start: usize,
    results: &mut Vec<Vec<usize>>,
) {
    for i in start..n {
        if prefix.iter().any(|&j| conflicts[i][j]) { continue; }
        prefix.push(i);
        if prefix.len() >= 2 {
            results.push(prefix.clone());
        }
        enumerate_independent_sets(conflicts, n, prefix, i + 1, results);
        prefix.pop();
    }
}

#[cfg(test)]
mod tests {
    mod find_matches_tests {
        use crate::aux::{loader::Loader, Tag};
        use crate::core::complex::Complex;
        use crate::core::diagram::{Diagram, Sign};
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

        fn pattern_for(rewrite: &Diagram) -> super::super::RulePattern {
            super::super::RulePattern::new(rewrite).expect("RulePattern::new")
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
            let rp = pattern_for(&rewrite);
            let matches = find_matches(&complex, &rewrite, &rp, &target, &rname).unwrap();
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
            let rp = pattern_for(&rewrite);
            let matches = find_matches(&complex, &rewrite, &rp, &idem_src, &rname).unwrap();
            assert_eq!(matches.len(), 1, "expected 1 match of idem in its own source");
        }

        #[test]
        fn idem_no_match() {
            // Target = id (single cell). Pattern = id id (two cells).
            // No match should be found.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let target = get_diagram(&complex, "rhs"); // rhs = id
            let rp = pattern_for(&rewrite);
            let matches = find_matches(&complex, &rewrite, &rp, &target, &rname).unwrap();
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
            let rp = pattern_for(&rewrite);
            let matches = find_matches(&complex, &rewrite, &rp, &target, &rname).unwrap();
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
            let rp = pattern_for(&rewrite);
            let matches1 = find_matches(&complex, &rewrite, &rp, &lhs, &rname).unwrap();
            assert!(!matches1.is_empty());
            let step1 = &matches1[0].step;
            let after1 = Diagram::boundary(Sign::Target, n, step1).unwrap();

            // Second application.
            let matches2 = find_matches(&complex, &rewrite, &rp, &after1, &rname).unwrap();
            assert_eq!(matches2.len(), 1);
            let step2 = &matches2[0].step;
            let after2 = Diagram::boundary(Sign::Target, n, step2).unwrap();

            assert!(Diagram::isomorphic(&after2, &rhs), "after two rewrites, should reach rhs");
        }

        #[test]
        fn idem_parallel_in_four_chain() {
            // idem : id id -> id. In "id id id id" (4 cells), there are 3 matches:
            //   m0 = [0,1], m1 = [1,2], m2 = [2,3].
            // Disjoint pairs: {m0, m2} = {[0,1], [2,3]}.
            // So there should be exactly one compatible family of size 2.
            use std::collections::HashMap;
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let rp = super::super::RulePattern::new(&rewrite).unwrap();

            // Build "id id id id" by pasting lhs (id id id) with one more id.
            let lhs = get_diagram(&complex, "lhs"); // id id id
            let id_diag = get_diagram(&complex, "id"); // classifier of id: single id cell
            let target = Diagram::paste(0, &lhs, &id_diag).unwrap(); // id id id id

            let matches = find_matches(&complex, &rewrite, &rp, &target, &rname).unwrap();
            assert_eq!(matches.len(), 3, "expected 3 individual matches of idem in id id id id");

            // Check the image positions.
            let positions: Vec<&Vec<usize>> = matches.iter()
                .map(|m| &m.image_positions)
                .collect();
            assert!(positions.contains(&&vec![0, 1]));
            assert!(positions.contains(&&vec![1, 2]));
            assert!(positions.contains(&&vec![2, 3]));

            let mut rule_patterns = HashMap::new();
            rule_patterns.insert(rname.clone(), rp);

            let families = super::super::find_compatible_families(
                &matches, &complex, &target, &rule_patterns, false,
            );
            assert_eq!(families.len(), 1, "expected exactly one compatible family of size 2");
            let fam = &families[0];
            assert_eq!(fam.family.len(), 2);

            // The family should be {m0, m2} (positions [0,1] and [2,3]).
            let fam_positions: Vec<Vec<usize>> = fam.family.iter()
                .map(|&i| matches[i].image_positions.clone())
                .collect();
            assert!(fam_positions.contains(&vec![0, 1]));
            assert!(fam_positions.contains(&vec![2, 3]));

            // Union of positions should be [0,1,2,3].
            assert_eq!(fam.image_positions, vec![0, 1, 2, 3]);

            // The parallel step should be (n+1)-dimensional.
            assert_eq!(fam.step.top_dim(), target.top_dim() + 1);

            // Its target boundary should be "id id" (two applications of idem in parallel).
            let n = target.top_dim();
            let after = Diagram::boundary(Sign::Target, n, &fam.step).unwrap();
            let id_id = Diagram::paste(0, &id_diag, &id_diag).unwrap();
            assert!(Diagram::isomorphic(&after, &id_id),
                "parallel application of two idem to id id id id should yield id id");
        }

        #[test]
        fn idem_no_parallel_in_three_chain() {
            // In "id id id" (3 cells), matches are at [0,1] and [1,2].
            // These overlap, so no compatible family of size >= 2.
            use std::collections::HashMap;
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let rp = super::super::RulePattern::new(&rewrite).unwrap();
            let target = get_diagram(&complex, "lhs"); // id id id

            let matches = find_matches(&complex, &rewrite, &rp, &target, &rname).unwrap();
            assert_eq!(matches.len(), 2);

            let mut rule_patterns = HashMap::new();
            rule_patterns.insert(rname.clone(), rp);

            let families = super::super::find_compatible_families(
                &matches, &complex, &target, &rule_patterns, false,
            );
            assert!(families.is_empty(), "no compatible families when matches overlap");
        }

    }
}
