//! Subdiagram matching: find rewrite applications of rules inside a target.
//!
//! The shared primitive is [`for_each_candidate`], which lazily iterates
//! candidate matches for a single rule.  Candidates are confirmed (pushout +
//! reconstruct) via [`confirm_candidate`], or grouped into maximal compatible
//! families via [`find_compatible_families`].

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

/// A lightweight candidate match: isomorphism confirmed but no step constructed.
#[derive(Clone)]
pub(crate) struct CandidateMatch {
    pub(crate) rule_name: String,
    pub(crate) image_positions: Vec<usize>,
    pub(crate) iso_emb: Embedding,
}

/// A confirmed rewrite: a compatible family of matches applied simultaneously.
/// Individual (non-parallel) rewrites are singleton families.
pub struct MatchResult {
    /// The (n+1)-dimensional step diagram (single or parallel rewrite).
    pub step: Diagram,
    /// The family members: rule name and matched positions for each constituent match.
    pub members: Vec<FamilyMember>,
    /// Union of all family members' matched top-dim positions.
    pub image_positions: Vec<usize>,
}

/// One constituent match in a rewrite family.
#[derive(Clone)]
pub struct FamilyMember {
    pub rule_name: String,
    pub match_positions: Vec<usize>,
    pub(crate) iso_emb: Embedding,
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

/// Find and confirm all rewrite applications of `rewrite` inside `target`.
///
/// Goes through the production confirmation path (`confirm_candidate` →
/// `construct_parallel_step`), so tests exercise the same code as the engine.
#[cfg(test)]
fn find_matches(
    complex: &Complex,
    rewrite: &Diagram,
    target: &Diagram,
    rule_name: &str,
) -> Vec<MatchResult> {
    let mut rule_patterns = HashMap::new();
    rule_patterns.insert(
        rule_name.to_owned(),
        RulePattern::new(rewrite).expect("RulePattern::new"),
    );
    let rp = rule_patterns.get(rule_name).unwrap();
    let mut results = Vec::new();
    for_each_candidate(rewrite, rp, target, rule_name, |cand| {
        if let Some(mr) = confirm_candidate(&cand, complex, target, &rule_patterns) {
            results.push(mr);
        }
        false
    }).expect("for_each_candidate");
    results
}

/// Confirm a single candidate by constructing it as a singleton family.
///
/// This is the unified confirmation path: every candidate — whether it will
/// become an individual rewrite or part of a parallel family — is confirmed
/// through [`construct_parallel_step`] and [`reconstruct`].
pub(crate) fn confirm_candidate(
    cand: &CandidateMatch,
    complex: &Complex,
    target: &Diagram,
    rule_patterns: &HashMap<String, RulePattern>,
) -> Option<MatchResult> {
    try_family(std::slice::from_ref(cand), complex, target, rule_patterns, &[0])
}

/// Iterate over candidate matches for a single rule, calling `f` for each.
/// Stops early if `f` returns `true`. Returns whether the callback stopped early.
///
/// Only flow matches whose `image_positions` are disjoint from `occupied` get
/// their (expensive) isomorphism check.  Others are silently skipped.
fn for_each_candidate_disjoint<F>(
    rewrite: &Diagram,
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
    occupied: &intset::IntSet,
    mut f: F,
) -> Result<bool, Error>
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
        return for_each_candidate_dim0(rule, target, rule_name, occupied, &mut f);
    }

    let k = n - 1;
    let (p_flow, p_node_map) = graph::flow_graph(&pattern.shape, k);
    let (t_flow, t_node_map) = graph::flow_graph(&target.shape, k);

    let p_labels: Vec<&crate::aux::Tag> = p_node_map.iter()
        .map(|&(dim, pos)| &pattern.labels[dim][pos])
        .collect();
    let t_labels: Vec<&crate::aux::Tag> = t_node_map.iter()
        .map(|&(dim, pos)| &target.labels[dim][pos])
        .collect();

    let flow_matches = find_path_induced_matches(&p_flow, &t_flow, &p_labels, &t_labels);

    for vertex_match in &flow_matches {
        let matched_cells: Vec<(usize, usize)> = vertex_match.iter()
            .map(|&ti| t_node_map[ti])
            .collect();

        let mut image_positions: Vec<usize> = matched_cells.iter()
            .filter(|(dim, _)| *dim == n)
            .map(|(_, pos)| *pos)
            .collect();
        image_positions.sort_unstable();

        if !intset::is_disjoint(&image_positions, occupied) {
            continue;
        }

        let iso_emb = match check_match_isomorphism(pattern, target, &matched_cells) {
            Some(e) => e,
            None => continue,
        };

        if f(CandidateMatch {
            rule_name: rule_name.to_owned(),
            image_positions,
            iso_emb,
        }) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Iterate over candidate matches for a single rule (no position filter).
#[cfg(test)]
pub(crate) fn for_each_candidate<F>(
    rewrite: &Diagram,
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
    f: F,
) -> Result<bool, Error>
where
    F: FnMut(CandidateMatch) -> bool,
{
    for_each_candidate_disjoint(rewrite, rule, target, rule_name, &Vec::new(), f)
}

fn for_each_candidate_dim0<F>(
    rule: &RulePattern,
    target: &Diagram,
    rule_name: &str,
    occupied: &intset::IntSet,
    f: &mut F,
) -> Result<bool, Error>
where
    F: FnMut(CandidateMatch) -> bool,
{
    let pattern = &rule.pattern;
    let pat_sizes = pattern.shape.sizes();
    let tgt_sizes = target.shape.sizes();
    if pat_sizes.is_empty() || pat_sizes[0] != 1 {
        return Ok(false);
    }
    let pat_tag = &pattern.labels[0][0];
    for pos in 0..tgt_sizes[0] {
        if &target.labels[0][pos] != pat_tag { continue; }
        if !intset::is_disjoint(&[pos], occupied) { continue; }
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
            return Ok(true);
        }
    }
    Ok(false)
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

// ---- Rewrite step construction ----

/// Construct the rewrite step for a compatible family of matches.
///
/// Each element of `match_data` is a `(rule_name, iso_emb)` pair — borrowed
/// from either `CandidateMatch` or `FamilyMember`, so no cloning is needed
/// in the hot path.
///
/// Delegates the ogposet-level colimit to [`pushout::multi_pushout`], then
/// merges labels from the target and rewrite diagrams. For singleton families,
/// this is equivalent to a single pushout.
fn construct_parallel_step(
    complex: &Complex,
    target: &Diagram,
    match_data: &[(&str, &Embedding)],
    rule_patterns: &HashMap<String, RulePattern>,
) -> Result<Diagram, Error> {
    let mut rewrites: Vec<&Diagram> = Vec::with_capacity(match_data.len());
    let mut spans: Vec<pushout::Span> = Vec::with_capacity(match_data.len());
    for &(rule_name, iso_emb) in match_data {
        let rp = rule_patterns.get(rule_name).ok_or_else(|| {
            Error::new(format!("rule pattern for '{}' not found", rule_name))
        })?;
        let rewrite = complex.classifier(rule_name).ok_or_else(|| {
            Error::new(format!("classifier for '{}' not found", rule_name))
        })?;
        spans.push(pushout::Span {
            into_base: iso_emb,
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

/// Find maximal compatible families of matches (parallel rewrite candidates).
///
/// A compatible family is a subset of matches whose `image_positions` are
/// pairwise disjoint. Families are enumerated lazily in size-descending order
/// (including size 1) and verified by constructing the iterated pushout and
/// running reconstruction. Confirmed families prune all their sub-families.
///
/// If `first_only` is set, returns as soon as the first family is verified.
/// Otherwise, returns all maximal verified families.
#[allow(dead_code)]
pub(crate) fn find_compatible_families(
    matches: &[CandidateMatch],
    complex: &Complex,
    target: &Diagram,
    rule_patterns: &HashMap<String, RulePattern>,
    first_only: bool,
) -> Vec<MatchResult> {
    let n = matches.len();
    if n == 0 { return vec![]; }

    let conflicts: Vec<Vec<bool>> = (0..n).map(|i| {
        (0..n).map(|j| {
            if i == j { return false; }
            !intset::is_disjoint(&matches[i].image_positions, &matches[j].image_positions)
        }).collect()
    }).collect();

    let k_start = if first_only {
        max_independent_set_size(&conflicts, n)
    } else {
        n
    };

    let mut results: Vec<MatchResult> = Vec::new();
    let mut confirmed: Vec<Vec<usize>> = Vec::new();

    for k in (1..=k_start).rev() {
        let mut prefix: Vec<usize> = Vec::new();
        enumerate_independent_sets_of_size(
            &conflicts, n, &mut prefix, 0, k,
            &mut |family| {
                let dominated = confirmed.iter().any(|kept| {
                    family.iter().all(|idx| kept.contains(idx))
                });
                if dominated { return false; }
                if let Some(r) = try_family(matches, complex, target, rule_patterns, family) {
                    results.push(r);
                    confirmed.push(family.to_vec());
                    if first_only { return true; }
                }
                false
            },
        );
        if first_only && !results.is_empty() { return results; }
    }

    results
}

fn try_family(
    matches: &[CandidateMatch],
    complex: &Complex,
    target: &Diagram,
    rule_patterns: &HashMap<String, RulePattern>,
    family: &[usize],
) -> Option<MatchResult> {
    let match_data: Vec<(&str, &Embedding)> = family.iter()
        .map(|&i| (matches[i].rule_name.as_str(), &matches[i].iso_emb))
        .collect();
    let step = match construct_parallel_step(complex, target, &match_data, rule_patterns) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let members: Vec<FamilyMember> = family.iter().map(|&i| FamilyMember {
        rule_name: matches[i].rule_name.clone(),
        match_positions: matches[i].image_positions.clone(),
        iso_emb: matches[i].iso_emb.clone(),
    }).collect();
    let image_positions = {
        let mut all: Vec<usize> = members.iter()
            .flat_map(|m| m.match_positions.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    };
    Some(MatchResult { step, members, image_positions })
}

/// Try to construct a parallel rewrite step from pre-built family members.
pub(crate) fn try_family_from_members(
    members: Vec<FamilyMember>,
    complex: &Complex,
    target: &Diagram,
    rule_patterns: &HashMap<String, RulePattern>,
) -> Option<MatchResult> {
    let match_data: Vec<(&str, &Embedding)> = members.iter()
        .map(|m| (m.rule_name.as_str(), &m.iso_emb))
        .collect();
    let step = match construct_parallel_step(complex, target, &match_data, rule_patterns) {
        Ok(s) => s,
        Err(_) => return None,
    };
    let image_positions = {
        let mut all: Vec<usize> = members.iter()
            .flat_map(|m| m.match_positions.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    };
    Some(MatchResult { step, members, image_positions })
}

/// Iterate over all rules and their candidates (no position filter).
pub(crate) fn for_each_rule_candidate<F>(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
    f: F,
) -> Result<bool, String>
where
    F: FnMut(CandidateMatch) -> bool,
{
    for_each_rule_candidate_disjoint(type_complex, rule_patterns, current, &Vec::new(), f)
}

/// Iterate over all rules' candidates whose positions are disjoint from `occupied`.
fn for_each_rule_candidate_disjoint<F>(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
    occupied: &intset::IntSet,
    mut f: F,
) -> Result<bool, String>
where
    F: FnMut(CandidateMatch) -> bool,
{
    let n = current.top_dim();
    for (name, _tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 { continue; }
        let Some(rewrite) = type_complex.classifier(name) else { continue; };
        let Some(rp) = rule_patterns.get(name) else { continue; };
        if for_each_candidate_disjoint(rewrite, rp, current, name, occupied, &mut f)
            .map_err(|e| format!("failed to match rule '{}': {}", name, e))?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---- Greedy parallel auto step ----

/// Build a greedy candidate family in a single pass over all rules.
///
/// Iterates every rule once, computing each flow graph once.  Within each
/// rule's flow matches, candidates whose `image_positions` are disjoint from
/// the (growing) `positions` set get an isomorphism check; accepted candidates
/// are added to the family and their positions merged into the set.
fn build_greedy_family(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
    occupied: &intset::IntSet,
) -> Result<(Vec<CandidateMatch>, intset::IntSet), String> {
    let mut family: Vec<CandidateMatch> = Vec::new();
    let mut positions = occupied.clone();
    let n = current.top_dim();

    if n == 0 {
        let tgt_sizes = current.shape.sizes();
        if tgt_sizes.is_empty() { return Ok((family, positions)); }
        for (name, _tag, dim) in type_complex.generators_iter() {
            if dim != 1 { continue; }
            let Some(rp) = rule_patterns.get(name) else { continue; };
            let pattern = &rp.pattern;
            let pat_sizes = pattern.shape.sizes();
            if pat_sizes.is_empty() || pat_sizes[0] != 1 { continue; }
            let pat_tag = &pattern.labels[0][0];
            for pos in 0..tgt_sizes[0] {
                if &current.labels[0][pos] != pat_tag { continue; }
                if !intset::is_disjoint(&[pos], &positions) { continue; }
                let map = vec![vec![pos]];
                let mut inv = vec![vec![NO_PREIMAGE; tgt_sizes[0]]];
                inv[0][pos] = 0;
                let emb = Embedding::make(
                    Arc::clone(&pattern.shape), Arc::clone(&current.shape), map, inv,
                );
                positions = intset::union(&positions, &vec![pos]);
                family.push(CandidateMatch {
                    rule_name: name.to_owned(),
                    image_positions: vec![pos],
                    iso_emb: emb,
                });
            }
        }
        return Ok((family, positions));
    }

    let k = n - 1;
    let (t_flow, t_node_map) = graph::flow_graph(&current.shape, k);
    let t_labels: Vec<&crate::aux::Tag> = t_node_map.iter()
        .map(|&(dim, pos)| &current.labels[dim][pos])
        .collect();

    for (name, _tag, dim) in type_complex.generators_iter() {
        if dim != n + 1 { continue; }
        if type_complex.classifier(name).is_none() { continue; }
        let Some(rp) = rule_patterns.get(name) else { continue; };
        let pattern = &rp.pattern;
        if pattern.top_dim() != n { continue; }

        let (p_flow, p_node_map) = graph::flow_graph(&pattern.shape, k);
        let p_labels: Vec<&crate::aux::Tag> = p_node_map.iter()
            .map(|&(dim, pos)| &pattern.labels[dim][pos])
            .collect();

        let flow_matches = find_path_induced_matches(&p_flow, &t_flow, &p_labels, &t_labels);

        for vertex_match in &flow_matches {
            let matched_cells: Vec<(usize, usize)> = vertex_match.iter()
                .map(|&ti| t_node_map[ti])
                .collect();

            let mut image_positions: Vec<usize> = matched_cells.iter()
                .filter(|(d, _)| *d == n)
                .map(|(_, pos)| *pos)
                .collect();
            image_positions.sort_unstable();

            if !intset::is_disjoint(&image_positions, &positions) {
                continue;
            }

            let iso_emb = match check_match_isomorphism(pattern, current, &matched_cells) {
                Some(e) => e,
                None => continue,
            };

            positions = intset::union(&positions, &image_positions);
            family.push(CandidateMatch {
                rule_name: name.to_owned(),
                image_positions,
                iso_emb,
            });
        }
    }

    Ok((family, positions))
}

/// Find a verified parallel rewrite family using a greedy-with-peel-back strategy.
///
/// For each rewrite rule in order, finds the first candidate match, then again
/// for each rule in order finds the first candidate disjoint from the previous,
/// and so on until no more disjoint matches remain.  Isomorphism checks are only
/// performed on candidates that pass the disjointness filter.  The resulting
/// family is verified (pushout + reconstruct); on failure, the last member is
/// dropped and the process repeats with the shorter prefix.
pub(crate) fn greedy_parallel_auto_step(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Option<MatchResult>, String> {
    let (family, _) = build_greedy_family(type_complex, rule_patterns, current, &Vec::new())?;
    if family.is_empty() { return Ok(None); }
    try_or_shrink(type_complex, rule_patterns, current, family)
}

/// Try `family`, then peel back one member at a time.
/// Returns `None` when even the singleton fails (not a real match).
fn try_or_shrink(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    target: &Diagram,
    family: Vec<CandidateMatch>,
) -> Result<Option<MatchResult>, String> {
    let indices: Vec<usize> = (0..family.len()).collect();
    if let Some(r) = try_family(&family, type_complex, target, rule_patterns, &indices) {
        return Ok(Some(r));
    }
    if family.len() <= 1 {
        return Ok(None);
    }

    // Drop the last member and rebuild from the prefix.
    let mut prefix = family;
    prefix.pop();
    let prefix_positions: intset::IntSet = prefix.iter()
        .fold(Vec::new(), |acc, c| intset::union(&acc, &c.image_positions));

    // Extend the prefix with fresh disjoint matches.
    let (mut extended, _) = build_greedy_family(
        type_complex, rule_patterns, target, &prefix_positions,
    )?;
    if !extended.is_empty() {
        let mut full = prefix.clone();
        full.append(&mut extended);
        let indices: Vec<usize> = (0..full.len()).collect();
        if let Some(r) = try_family(&full, type_complex, target, rule_patterns, &indices) {
            return Ok(Some(r));
        }
    }

    try_or_shrink(type_complex, rule_patterns, target, prefix)
}

/// Find the size of the maximum independent set in the conflict graph.
#[allow(dead_code)]
fn max_independent_set_size(conflicts: &[Vec<bool>], n: usize) -> usize {
    let mut max_size = 0;
    let mut prefix: Vec<usize> = Vec::new();
    max_is_dfs(conflicts, n, &mut prefix, 0, &mut max_size);
    max_size
}

#[allow(dead_code)]
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
#[allow(dead_code)]
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

        fn get_diagram(complex: &Complex, name: &str) -> Diagram {
            complex.find_diagram(name).cloned()
                .unwrap_or_else(|| complex.classifier(name).cloned()
                    .unwrap_or_else(|| panic!("diagram '{}' not found", name)))
        }

        #[test]
        fn idem_whole_match() {
            // idem : id id -> id. Pattern = id id.
            // In "id id id" there should be 2 matches: at positions [0,1] and [1,2].
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let target = get_diagram(&complex, "lhs"); // lhs = id id id
            let matches = find_matches(&complex, &rewrite, &target, &rname);
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
            let matches = find_matches(&complex, &rewrite, &idem_src, &rname);
            assert_eq!(matches.len(), 1, "expected 1 match of idem in its own source");
        }

        #[test]
        fn idem_no_match() {
            // Target = id (single cell). Pattern = id id (two cells).
            // No match should be found.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let target = get_diagram(&complex, "rhs"); // rhs = id
            let matches = find_matches(&complex, &rewrite, &target, &rname);
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
            let matches = find_matches(&complex, &rewrite, &target, &rname);
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
            let matches1 = find_matches(&complex, &rewrite, &lhs, &rname);
            assert!(!matches1.is_empty());
            let step1 = &matches1[0].step;
            let after1 = Diagram::boundary(Sign::Target, n, step1).unwrap();

            // Second application.
            let matches2 = find_matches(&complex, &rewrite, &after1, &rname);
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

            // Build "id id id id" by pasting lhs (id id id) with one more id.
            let lhs = get_diagram(&complex, "lhs"); // id id id
            let id_diag = get_diagram(&complex, "id"); // classifier of id: single id cell
            let target = Diagram::paste(0, &lhs, &id_diag).unwrap(); // id id id id

            let mut rule_patterns = HashMap::new();
            rule_patterns.insert(rname.clone(), super::super::RulePattern::new(&rewrite).unwrap());
            let rp = rule_patterns.get(&rname).unwrap();

            let mut candidates = Vec::new();
            super::super::for_each_candidate(&rewrite, rp, &target, &rname, |cand| {
                candidates.push(cand);
                false
            }).unwrap();
            assert_eq!(candidates.len(), 3, "expected 3 candidate matches of idem in id id id id");

            let positions: Vec<&Vec<usize>> = candidates.iter()
                .map(|m| &m.image_positions)
                .collect();
            assert!(positions.contains(&&vec![0, 1]));
            assert!(positions.contains(&&vec![1, 2]));
            assert!(positions.contains(&&vec![2, 3]));

            let families = super::super::find_compatible_families(
                &candidates, &complex, &target, &rule_patterns, false,
            );
            // One family of size 2 ({m0,m2}) plus m1 as an uncovered size-1 family.
            assert_eq!(families.len(), 2);
            let fam = &families[0];
            assert_eq!(fam.members.len(), 2);
            assert_eq!(families[1].members.len(), 1);

            // The family should be {m0, m2} (positions [0,1] and [2,3]).
            let fam_positions: Vec<Vec<usize>> = fam.members.iter()
                .map(|m| m.match_positions.clone())
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
            // These overlap, so no family of size >= 2; the two maximal
            // families are the individual matches themselves (size 1 each).
            use std::collections::HashMap;
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let target = get_diagram(&complex, "lhs"); // id id id

            let mut rule_patterns = HashMap::new();
            rule_patterns.insert(rname.clone(), super::super::RulePattern::new(&rewrite).unwrap());
            let rp = rule_patterns.get(&rname).unwrap();

            let mut candidates = Vec::new();
            super::super::for_each_candidate(&rewrite, rp, &target, &rname, |cand| {
                candidates.push(cand);
                false
            }).unwrap();
            assert_eq!(candidates.len(), 2);

            let families = super::super::find_compatible_families(
                &candidates, &complex, &target, &rule_patterns, false,
            );
            assert_eq!(families.len(), 2, "two maximal families of size 1");
            assert!(families.iter().all(|f| f.members.len() == 1));
        }

        #[test]
        fn greedy_parallel_in_four_chain() {
            // Same setup as idem_parallel_in_four_chain: "id id id id" with
            // 3 candidate matches m0=[0,1], m1=[1,2], m2=[2,3].
            // The greedy algorithm should find {m0, m2} as the first family.
            let (_store, complex) = load_type(&fixture("Idem.ali"), "Idem");
            let (rewrite, rname) = get_rewrite(&complex, "idem");
            let lhs = get_diagram(&complex, "lhs");
            let id_diag = get_diagram(&complex, "id");
            let target = Diagram::paste(0, &lhs, &id_diag).unwrap();

            let mut rule_patterns = std::collections::HashMap::new();
            rule_patterns.insert(rname.clone(), super::super::RulePattern::new(&rewrite).unwrap());

            let result = super::super::greedy_parallel_auto_step(
                &complex, &rule_patterns, &target,
            ).unwrap();
            let result = result.expect("should find a parallel family");

            assert_eq!(result.members.len(), 2, "greedy family should have 2 members");
            let positions: Vec<Vec<usize>> = result.members.iter()
                .map(|m| m.match_positions.clone())
                .collect();
            assert!(positions.contains(&vec![0, 1]));
            assert!(positions.contains(&vec![2, 3]));
            assert_eq!(result.image_positions, vec![0, 1, 2, 3]);

            let n = target.top_dim();
            let after = Diagram::boundary(Sign::Target, n, &result.step).unwrap();
            let id_id = Diagram::paste(0, &id_diag, &id_diag).unwrap();
            assert!(Diagram::isomorphic(&after, &id_id),
                "parallel idem on id id id id should yield id id");
        }

    }
}
