//! Constraint-based boundary inference for holes (`?`).
//!
//! Boundary inference is split into two phases:
//!
//! 1. **Collection**: during interpretation each inference site emits typed
//!    [`Constraint`]s into the [`InterpResult`][super::types::InterpResult].
//!    No enrichment happens in-place.
//!
//! 2. **Solving**: after the entire file is interpreted, [`solve`] processes
//!    the collected constraints in one pass and produces a [`SolvedHole`] for
//!    each registered hole.
//!
//! The solver decomposes composite constraints (`Eq`, `Parallel`) into atomic
//! `BoundaryEq` facts, checks consistency where slots are claimed more than
//! once, and fills remaining slots with weaker `PartialBoundary` evidence from
//! partial-map contexts.

use crate::aux::Tag;
use crate::core::{
    complex::Complex,
    diagram::{Diagram, Sign as DiagramSign},
    partial_map::PartialMap,
};
use crate::language::ast::Span;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// ---- Notation helpers -------------------------------------------------------

fn dim_subscript(n: usize) -> String {
    const SUBS: [char; 10] = ['₀','₁','₂','₃','₄','₅','₆','₇','₈','₉'];
    n.to_string()
        .chars()
        .map(|c| c.to_digit(10).and_then(|d| SUBS.get(d as usize)).copied().unwrap_or(c))
        .collect()
}

// ---- Hole identity ----------------------------------------------------------

static HOLE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A process-unique identifier for a single `?` hole.
///
/// Each time a hole is encountered during interpretation a fresh `HoleId` is
/// allocated; two `?` tokens always produce two distinct ids.  The same atomic-
/// counter pattern is used as for [`crate::aux::GlobalId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HoleId(usize);

impl HoleId {
    /// Allocate a fresh identifier unique within this process.
    pub fn fresh() -> Self {
        Self(HOLE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl std::fmt::Display for HoleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "?{}", self.0)
    }
}

// ---- Boundary slot ----------------------------------------------------------

/// A boundary slot: the combination of a sign and a dimension that identifies
/// one specific boundary of a hole.
///
/// For an n-cell hole the *principal* slot is `(Source, n-1)` and
/// `(Target, n-1)`.  Lower-dimensional slots (e.g. `(Source, 0)`) represent
/// the boundaries of boundaries and are derived by the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BdSlot {
    pub sign: DiagramSign,
    pub dim: usize,
}

// ---- Constraint origin ------------------------------------------------------

/// Records where a constraint came from so diagnostics can explain the
/// inference chain to the user.
#[derive(Debug, Clone)]
pub enum ConstraintOrigin {
    /// Paste or juxtaposition at this dimension: `left *k hole *k right`.
    Paste { paste_dim: usize },
    /// Boundary declaration: `hole -> tgt` or `src -> hole`.
    Declaration,
    /// Assertion: `assert hole = diagram`.
    Assertion,
    /// Partial-map clause: `source_cell => hole`.
    PartialMap { source_tag: Tag },
}

impl std::fmt::Display for ConstraintOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Paste { paste_dim } => write!(f, "paste at dim {}", paste_dim),
            Self::Declaration => write!(f, "boundary declaration"),
            Self::Assertion => write!(f, "assertion"),
            Self::PartialMap { source_tag } => write!(f, "partial map (source: {})", source_tag),
        }
    }
}

// ---- Constraints ------------------------------------------------------------

/// A constraint on a single hole, emitted during interpretation.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// `∂^sign_k(hole) = diagram`.
    ///
    /// Emitted from paste/juxtaposition contexts: the target-k boundary of the
    /// left neighbour must equal the source-k boundary of the hole (and
    /// symmetrically for the right neighbour).
    BoundaryEq {
        hole: HoleId,
        slot: BdSlot,
        diagram: Diagram,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    },

    /// `hole` is parallel to `companion`.
    ///
    /// Equivalent to: `dim(hole) = dim(companion)` and, for every `j <
    /// dim(companion)` and both signs `s`: `∂^s_j(hole) = ∂^s_j(companion)`.
    ///
    /// Emitted when a hole appears in a boundary declaration position:
    /// `hole -> tgt` means the hole must be a diagram parallel to `tgt`.
    Parallel {
        hole: HoleId,
        companion: Diagram,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    },

    /// `hole = diagram` (full equality).
    ///
    /// Subsumes `Parallel` and all `BoundaryEq`.  Emitted from assertions:
    /// `assert hole = d` means the hole must equal `d`.
    Eq {
        hole: HoleId,
        diagram: Diagram,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    },

    /// `dim(hole) = dim`.
    DimEq {
        hole: HoleId,
        dim: usize,
        origin: ConstraintOrigin,
    },

    /// Boundary known only through a partial map that may have unmapped entries.
    ///
    /// This is a *weaker* form of `BoundaryEq`: it fills a slot only when no
    /// `BoundaryEq` (or derived) constraint is available at that slot.
    ///
    /// Emitted from partial-map clause contexts: `source_cell => hole`.
    PartialBoundary {
        hole: HoleId,
        slot: BdSlot,
        boundary: Diagram,
        map: PartialMap,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    },
}

impl Constraint {
    /// The hole this constraint refers to.
    pub fn hole(&self) -> HoleId {
        match self {
            Self::BoundaryEq { hole, .. }
            | Self::Parallel { hole, .. }
            | Self::Eq { hole, .. }
            | Self::DimEq { hole, .. }
            | Self::PartialBoundary { hole, .. } => *hole,
        }
    }
}

// ---- Solved state -----------------------------------------------------------

/// A boundary slot that has been resolved by the solver.
#[derive(Debug, Clone)]
pub enum SolvedBd {
    /// A fully determined boundary diagram.
    Known {
        diagram: Diagram,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    },
    /// A boundary partially determined through a partial map (some entries may
    /// be unmapped).
    Partial {
        boundary: Diagram,
        map: PartialMap,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    },
}

/// The fully solved state of a hole after constraint propagation.
#[derive(Debug, Clone)]
pub struct SolvedHole {
    pub id: HoleId,
    pub span: Span,
    /// Resolved dimension, if any.
    pub dim: Option<usize>,
    /// Resolved boundary slots, keyed by `(sign, dim)`.
    pub boundaries: BTreeMap<BdSlot, SolvedBd>,
    /// If an `Eq` constraint pinned the hole to a specific diagram.
    pub value: Option<(Diagram, Arc<Complex>)>,
    /// Inconsistency messages produced when two constraints on the same slot
    /// disagreed.
    pub inconsistencies: Vec<String>,
}

impl SolvedHole {
    fn new(id: HoleId, span: Span) -> Self {
        Self {
            id,
            span,
            dim: None,
            boundaries: BTreeMap::new(),
            value: None,
            inconsistencies: vec![],
        }
    }

    /// Record `dim = n`, checking consistency with any prior `DimEq`.
    fn set_dim(&mut self, n: usize, origin: &ConstraintOrigin) {
        match self.dim {
            None => self.dim = Some(n),
            Some(existing) if existing == n => {}
            Some(existing) => self.inconsistencies.push(format!(
                "conflicting dimension constraints: {} vs {} (from {})",
                existing, n, origin
            )),
        }
    }

    /// Try to fill `slot` with a `Known` boundary. If the slot is already
    /// `Known` with a *different* diagram, record an inconsistency.
    fn set_known(
        &mut self,
        slot: BdSlot,
        diagram: Diagram,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    ) {
        match self.boundaries.get(&slot) {
            None | Some(SolvedBd::Partial { .. }) => {
                self.boundaries
                    .insert(slot, SolvedBd::Known { diagram, scope, origin });
            }
            Some(SolvedBd::Known { diagram: existing, .. }) => {
                // Consistency check: isomorphic handles normalisation.
                if !Diagram::isomorphic(existing, &diagram) {
                    self.inconsistencies.push(format!(
                        "conflicting ∂{}{}: from {}",
                        match slot.sign { DiagramSign::Source => "⁻", DiagramSign::Target => "⁺" },
                        dim_subscript(slot.dim),
                        origin
                    ));
                }
                // Keep the first (earlier) constraint to preserve its origin.
            }
        }
    }

    /// Fill `slot` with a `Partial` boundary, but only if the slot is
    /// currently empty (weaker than `Known`).
    fn set_partial(
        &mut self,
        slot: BdSlot,
        boundary: Diagram,
        map: PartialMap,
        scope: Arc<Complex>,
        origin: ConstraintOrigin,
    ) {
        if !self.boundaries.contains_key(&slot) {
            self.boundaries.insert(
                slot,
                SolvedBd::Partial { boundary, map, scope, origin },
            );
        }
    }
}

// ---- Hole entry (solver input) ----------------------------------------------

/// A registered hole: the minimum information needed before constraints are
/// collected.
#[derive(Debug, Clone)]
pub struct HoleEntry {
    pub id: HoleId,
    pub span: Span,
}

// ---- Solver -----------------------------------------------------------------

/// Solve a set of constraints and produce one [`SolvedHole`] per registered
/// hole.
///
/// The solver runs in a single pass (no fixpoint iteration needed in the
/// absence of `SharedBoundary` constraints):
///
/// 1. Decompose `Eq` and `Parallel` into `DimEq` + `BoundaryEq` facts.
/// 2. Globular propagation on `BoundaryEq`: from `(s,k)` derive `(s',j)` for j < k.
/// 3. Apply `BoundaryEq` to fill / check boundary slots (`Known`).
/// 4. Apply `DimEq`.
/// 5. Globular propagation on `PartialBoundary`: from `(s,k)` derive `(s',j)` for j < k.
/// 6. Fill remaining empty slots from `PartialBoundary` (`Partial`, weaker than `Known`).
///
/// Constraints for unknown hole ids (not in `entries`) are silently ignored so
/// that callers do not need to filter.
pub fn solve(entries: &[HoleEntry], constraints: &[Constraint]) -> Vec<SolvedHole> {
    // Build an index: HoleId -> position in the output vec.
    let mut holes: Vec<SolvedHole> =
        entries.iter().map(|e| SolvedHole::new(e.id, e.span)).collect();
    let index: std::collections::HashMap<HoleId, usize> =
        entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();

    // Helpers to look up a hole mutably by id.
    macro_rules! get_mut {
        ($id:expr) => {
            index.get(&$id).map(|&i| &mut holes[i])
        };
    }

    // ---- pass 1: expand Eq and Parallel into atomic constraints, then apply ----
    // Collect atomic BoundaryEq, DimEq, and PartialBoundary into separate vecs
    // so we can apply them in deterministic order.
    let mut boundary_eqs: Vec<(HoleId, BdSlot, Diagram, Arc<Complex>, ConstraintOrigin)> = vec![];
    let mut dim_eqs: Vec<(HoleId, usize, ConstraintOrigin)> = vec![];
    let mut partial_bds: Vec<(HoleId, BdSlot, Diagram, PartialMap, Arc<Complex>, ConstraintOrigin)> =
        vec![];

    for constraint in constraints {
        match constraint {
            Constraint::Eq { hole, diagram, scope, origin } => {
                // Eq: record value, then expand like Parallel.
                if let Some(h) = get_mut!(*hole) {
                    h.value = Some((diagram.clone(), scope.clone()));
                }
                expand_parallel(*hole, diagram, scope, origin, &mut dim_eqs, &mut boundary_eqs);
            }
            Constraint::Parallel { hole, companion, scope, origin } => {
                expand_parallel(*hole, companion, scope, origin, &mut dim_eqs, &mut boundary_eqs);
            }
            Constraint::BoundaryEq { hole, slot, diagram, scope, origin } => {
                boundary_eqs.push((*hole, *slot, diagram.clone(), scope.clone(), origin.clone()));
            }
            Constraint::DimEq { hole, dim, origin } => {
                dim_eqs.push((*hole, *dim, origin.clone()));
            }
            Constraint::PartialBoundary { hole, slot, boundary, map, scope, origin } => {
                partial_bds.push((
                    *hole,
                    *slot,
                    boundary.clone(),
                    map.clone(),
                    scope.clone(),
                    origin.clone(),
                ));
            }
        }
    }

    // ---- globular propagation ----
    // For each BoundaryEq at (s, k) with k > 0, derive BoundaryEq at every (s', j) for j < k.
    // Justified by the globular identity: for well-formed diagrams (which are always round),
    // ∂^{s'}_j(∂^s_k(H)) = ∂^{s'}_j(H), so knowing ∂^s_k(H) = D implies ∂^{s'}_j(H) = ∂^{s'}_j(D).
    // Process entries as we extend the vec (new entries have strictly smaller dim, so no cycle).
    {
        let mut i = 0;
        while i < boundary_eqs.len() {
            let (hole, slot, diagram, scope, origin) = boundary_eqs[i].clone();
            if slot.dim > 0 {
                for j in 0..slot.dim {
                    for &sign2 in &[DiagramSign::Source, DiagramSign::Target] {
                        if let Ok(bd) = Diagram::boundary_normal(sign2, j, &diagram) {
                            boundary_eqs.push((
                                hole,
                                BdSlot { sign: sign2, dim: j },
                                bd,
                                scope.clone(),
                                origin.clone(),
                            ));
                        }
                    }
                }
            }
            i += 1;
        }
    }

    // ---- pass 2: apply BoundaryEq ----
    for (hole, slot, diagram, scope, origin) in boundary_eqs {
        if let Some(h) = get_mut!(hole) {
            h.set_known(slot, diagram, scope, origin);
        }
    }

    // ---- pass 3: apply DimEq ----
    for (hole, dim, origin) in dim_eqs {
        if let Some(h) = get_mut!(hole) {
            h.set_dim(dim, &origin);
        }
    }

    // ---- globular propagation for PartialBoundary ----
    // For each PartialBoundary at (s, k) with k > 0, derive PartialBoundary at every (s', j) for j < k.
    // Same globular identity as for BoundaryEq: ∂^{s'}_j(∂^s_k(H)) = ∂^{s'}_j(H).
    // The same `map` applies because globular propagation is taken in the source scope.
    {
        let mut i = 0;
        while i < partial_bds.len() {
            let (hole, slot, boundary, map, scope, origin) = partial_bds[i].clone();
            if slot.dim > 0 {
                for j in 0..slot.dim {
                    for &sign2 in &[DiagramSign::Source, DiagramSign::Target] {
                        if let Ok(bd) = Diagram::boundary_normal(sign2, j, &boundary) {
                            partial_bds.push((
                                hole,
                                BdSlot { sign: sign2, dim: j },
                                bd,
                                map.clone(),
                                scope.clone(),
                                origin.clone(),
                            ));
                        }
                    }
                }
            }
            i += 1;
        }
    }

    // ---- pass 4: fill remaining slots from PartialBoundary ----
    for (hole, slot, boundary, map, scope, origin) in partial_bds {
        if let Some(h) = get_mut!(hole) {
            h.set_partial(slot, boundary, map, scope, origin);
        }
    }

    holes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: HoleId) -> HoleEntry {
        HoleEntry { id, span: crate::language::ast::Span { start: 0, end: 0 } }
    }

    #[test]
    fn solve_empty() {
        let result = solve(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn solve_no_constraints() {
        let id = HoleId::fresh();
        let result = solve(&[entry(id)], &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, id);
        assert!(result[0].dim.is_none());
        assert!(result[0].boundaries.is_empty());
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_dim_eq() {
        let id = HoleId::fresh();
        let c = Constraint::DimEq { hole: id, dim: 3, origin: ConstraintOrigin::Declaration };
        let result = solve(&[entry(id)], &[c]);
        assert_eq!(result[0].dim, Some(3));
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_conflicting_dim_eq() {
        let id = HoleId::fresh();
        let c1 = Constraint::DimEq { hole: id, dim: 2, origin: ConstraintOrigin::Declaration };
        let c2 = Constraint::DimEq { hole: id, dim: 3, origin: ConstraintOrigin::Assertion };
        let result = solve(&[entry(id)], &[c1, c2]);
        assert_eq!(result[0].dim, Some(2)); // first one wins
        assert_eq!(result[0].inconsistencies.len(), 1);
    }

    #[test]
    fn solve_unknown_hole_ignored() {
        let id = HoleId::fresh();
        let unknown = HoleId::fresh();
        let c = Constraint::DimEq { hole: unknown, dim: 1, origin: ConstraintOrigin::Declaration };
        let result = solve(&[entry(id)], &[c]);
        assert_eq!(result.len(), 1);
        assert!(result[0].dim.is_none()); // id was not constrained
    }

    #[test]
    fn solve_multiple_holes_independent() {
        let id1 = HoleId::fresh();
        let id2 = HoleId::fresh();
        let c1 = Constraint::DimEq { hole: id1, dim: 1, origin: ConstraintOrigin::Declaration };
        let c2 = Constraint::DimEq { hole: id2, dim: 2, origin: ConstraintOrigin::Assertion };
        let result = solve(&[entry(id1), entry(id2)], &[c1, c2]);
        assert_eq!(result.len(), 2);
        let h1 = result.iter().find(|h| h.id == id1).unwrap();
        let h2 = result.iter().find(|h| h.id == id2).unwrap();
        assert_eq!(h1.dim, Some(1));
        assert_eq!(h2.dim, Some(2));
    }
}

// ---- Internal helpers -------------------------------------------------------

/// Expand a `Parallel` (or `Eq`) constraint into `DimEq` + `BoundaryEq` atoms.
///
/// For a companion of dimension n, emits `DimEq(hole, n)` and, for every `j`
/// in `0..n` and both signs, `BoundaryEq(hole, (s, j), ∂^s_j(companion))`.
///
/// Uses `Diagram::boundary_normal` so that equality checking later uses
/// canonical forms.
fn expand_parallel(
    hole: HoleId,
    companion: &Diagram,
    scope: &Arc<Complex>,
    origin: &ConstraintOrigin,
    dim_eqs: &mut Vec<(HoleId, usize, ConstraintOrigin)>,
    boundary_eqs: &mut Vec<(HoleId, BdSlot, Diagram, Arc<Complex>, ConstraintOrigin)>,
) {
    let n = companion.top_dim();
    dim_eqs.push((hole, n, origin.clone()));
    for j in 0..n {
        for &sign in &[DiagramSign::Source, DiagramSign::Target] {
            if let Ok(bd) = Diagram::boundary_normal(sign, j, companion) {
                boundary_eqs.push((
                    hole,
                    BdSlot { sign, dim: j },
                    bd,
                    scope.clone(),
                    origin.clone(),
                ));
            }
        }
    }
}

