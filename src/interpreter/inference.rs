//! Constraint-based boundary inference for holes (`?`).
//!
//! Boundary inference is split into two phases:
//!
//! 1. **Collection**: during interpretation each inference site emits typed
//!    [`Constraint`]s into the [`InterpResult`][super::types::InterpResult].
//!    No enrichment happens in-place.
//!
//! 2. **Solving**: after the entire file is interpreted, [`solve`] processes
//!    the collected constraints using a work-queue fixpoint algorithm, iterating
//!    until no new information can be derived.
//!
//! The solver is fixpoint-correct: each successful constraint application may
//! derive follow-up constraints (globular sub-boundaries, `Value`→`BoundaryEq`,
//! etc.) that are enqueued and processed in turn.  Termination is guaranteed
//! because each hole's state transitions monotonically (`Empty → Partial →
//! Known`) and `dim`/`value` are set at most once.

use crate::aux::{self, Tag};
use crate::core::{
    complex::Complex,
    diagram::{Diagram, Sign as DiagramSign},
    partial_map::PartialMap,
};
use crate::language::ast::Span;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
///
/// Composite information (parallelism, equality) is decomposed into atomic
/// `BoundaryEq` and `DimEq` facts at the emission site; the solver only ever
/// processes the four variants below.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// `∂^sign_k(hole) = diagram`.
    ///
    /// Emitted from paste/juxtaposition contexts and boundary declarations.
    BoundaryEq {
        hole: HoleId,
        slot: BdSlot,
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

    /// `hole = diagram` (exact value).
    ///
    /// Emitted from assertions (`assert ? = d`).  Records the hole's exact
    /// value; the boundary constraints implied by equality are derived
    /// automatically by the solver and enqueued as `BoundaryEq`/`DimEq`.
    Value {
        hole: HoleId,
        diagram: Diagram,
        scope: Arc<Complex>,
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
            | Self::DimEq { hole, .. }
            | Self::Value { hole, .. }
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
    /// Returns `true` if `dim` was newly set (i.e. was `None` before).
    fn set_dim(&mut self, n: usize, origin: &ConstraintOrigin) -> bool {
        match self.dim {
            None => {
                self.dim = Some(n);
                true
            }
            Some(existing) if existing == n => false,
            Some(existing) => {
                self.inconsistencies.push(format!(
                    "conflicting dimension constraints: {} vs {} (from {})",
                    existing, n, origin
                ));
                false
            }
        }
    }

    /// Record `value = diagram`, checking consistency with any prior `Value`.
    /// Returns `true` if the value was newly set (i.e. was `None` before).
    fn set_value(&mut self, diagram: &Diagram, scope: &Arc<Complex>, origin: &ConstraintOrigin) -> bool {
        match self.value {
            None => {
                self.value = Some((diagram.clone(), scope.clone()));
                true
            }
            Some((ref existing, _)) => {
                if !Diagram::isomorphic(existing, diagram) {
                    self.inconsistencies.push(format!(
                        "conflicting Value constraints: from {}",
                        origin
                    ));
                }
                false
            }
        }
    }

    /// Try to fill `slot` with a `Known` boundary.
    ///
    /// Returns `true` if the slot gained new information (was `Empty` or
    /// `Partial`; the caller should then derive globular sub-constraints).
    /// Returns `false` if the slot was already `Known` (consistent duplicate)
    /// or an inconsistency was recorded (conflicting duplicate).
    fn set_known(
        &mut self,
        slot: BdSlot,
        diagram: &Diagram,
        scope: &Arc<Complex>,
        origin: &ConstraintOrigin,
    ) -> bool {
        match self.boundaries.get(&slot) {
            None | Some(SolvedBd::Partial { .. }) => {
                self.boundaries.insert(slot, SolvedBd::Known {
                    diagram: diagram.clone(),
                    scope: scope.clone(),
                    origin: origin.clone(),
                });
                true
            }
            Some(SolvedBd::Known { diagram: existing, .. }) => {
                if !Diagram::isomorphic(existing, diagram) {
                    self.inconsistencies.push(format!(
                        "conflicting ∂{}{}: from {}",
                        match slot.sign { DiagramSign::Source => "⁻", DiagramSign::Target => "⁺" },
                        aux::dim_subscript(slot.dim),
                        origin
                    ));
                }
                // Keep the first (earlier) constraint to preserve its origin.
                false
            }
        }
    }

    /// Fill `slot` with a `Partial` boundary.
    ///
    /// Returns `true` if the slot was newly created (was `Empty`); the caller
    /// should then derive globular sub-constraints.  Returns `false` if the slot
    /// was already `Partial` (the maps are merged in-place; no re-propagation
    /// needed) or already `Known` (`Known` takes priority over `Partial`).
    fn set_partial(
        &mut self,
        slot: BdSlot,
        boundary: &Diagram,
        map: &PartialMap,
        scope: &Arc<Complex>,
        origin: &ConstraintOrigin,
    ) -> bool {
        match self.boundaries.get(&slot) {
            None => {
                self.boundaries.insert(slot, SolvedBd::Partial {
                    boundary: boundary.clone(),
                    map: map.clone(),
                    scope: scope.clone(),
                    origin: origin.clone(),
                });
                true
            }
            Some(SolvedBd::Partial { .. }) => {
                // Take ownership via remove to avoid cloning the existing entry.
                let SolvedBd::Partial { map: existing_map, boundary: existing_bd, scope: existing_scope, origin: existing_origin }
                    = self.boundaries.remove(&slot).unwrap() else { unreachable!() };
                let merged = PartialMap::merge(&existing_map, map);
                self.boundaries.insert(slot, SolvedBd::Partial {
                    boundary: existing_bd,
                    map: merged,
                    scope: existing_scope,
                    origin: existing_origin,
                });
                // No new slot opened: sub-slots were already propagated at first insertion.
                false
            }
            Some(SolvedBd::Known { .. }) => false,
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
/// The solver runs a work-queue fixpoint loop:
///
/// 1. All initial constraints are loaded into a queue.
/// 2. Each constraint is applied to its hole.  When a constraint produces new
///    information (a slot changes state, `dim` or `value` is set for the first
///    time), derived constraints are computed and enqueued:
///    - `BoundaryEq` at `(s, k)` → globular `BoundaryEq` at `(s', j)` for `j < k`.
///    - `Value` → `DimEq` + `BoundaryEq` at the two principal slots.
///    - `PartialBoundary` at `(s, k)` → globular `PartialBoundary` at `(s', j)` for `j < k`.
/// 3. The loop ends when the queue is empty (no new information).
///
/// Termination: slot states transition monotonically (`Empty → Partial →
/// Known`), and `dim`/`value` are set at most once per hole, so the total
/// number of state changes is bounded.
///
/// Constraints for unknown hole ids (not in `entries`) are silently ignored.
pub fn solve(entries: &[HoleEntry], constraints: &[Constraint]) -> Vec<SolvedHole> {
    // Note: `scope` fields on constraints are pass-through — the solver never
    // inspects them; they are stored on `SolvedBd` for downstream rendering only.

    // Build an index: HoleId -> position in the output vec.
    let mut holes: Vec<SolvedHole> =
        entries.iter().map(|e| SolvedHole::new(e.id, e.span)).collect();
    let index: HashMap<HoleId, usize> =
        entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();

    // Seed the work queue with all initial constraints.
    let mut queue: VecDeque<Constraint> = constraints.iter().cloned().collect();

    while let Some(constraint) = queue.pop_front() {
        let hole_id = constraint.hole();
        let Some(&idx) = index.get(&hole_id) else { continue; };
        let derived = process_constraint(&mut holes[idx], hole_id, constraint);
        queue.extend(derived);
    }

    holes
}

// ---- Internal helpers -------------------------------------------------------

/// Apply one constraint to a hole and return any derived constraints to enqueue.
///
/// Returns an empty vec when the constraint produced no new information
/// (duplicate, inconsistency-only, or `DimEq` which never derives further
/// constraints on its own).
fn process_constraint(hole: &mut SolvedHole, hole_id: HoleId, constraint: Constraint) -> Vec<Constraint> {
    match constraint {
        Constraint::Value { hole: _, diagram, scope, origin } => {
            if !hole.set_value(&diagram, &scope, &origin) {
                return vec![];
            }
            // Derive: DimEq from the value's dimension, then BoundaryEq at
            // the two principal slots.  Globular sub-constraints will be
            // derived in turn when those BoundaryEq constraints are processed.
            let n = diagram.top_dim();
            let mut derived = vec![
                Constraint::DimEq { hole: hole_id, dim: n, origin: origin.clone() },
            ];
            if n > 0 {
                // Emit only the two principal slots (sign, n-1); their processing
                // will derive all globular sub-slots via globular_sub_boundaries.
                for &sign in &[DiagramSign::Source, DiagramSign::Target] {
                    if let Ok(bd) = Diagram::boundary_normal(sign, n - 1, &diagram) {
                        derived.push(Constraint::BoundaryEq {
                            hole: hole_id,
                            slot: BdSlot { sign, dim: n - 1 },
                            diagram: bd,
                            scope: scope.clone(),
                            origin: origin.clone(),
                        });
                    }
                }
            }
            derived
        }

        Constraint::DimEq { hole: _, dim, origin } => {
            hole.set_dim(dim, &origin);
            // No constraints are derived from DimEq alone.
            vec![]
        }

        Constraint::BoundaryEq { hole: _, slot, diagram, scope, origin } => {
            if !hole.set_known(slot, &diagram, &scope, &origin) {
                return vec![];
            }
            // Derive: globular BoundaryEq at all sub-slots.
            globular_sub_boundaries(slot, &diagram)
                .into_iter()
                .map(|(sub_slot, bd)| Constraint::BoundaryEq {
                    hole: hole_id,
                    slot: sub_slot,
                    diagram: bd,
                    scope: scope.clone(),
                    origin: origin.clone(),
                })
                .collect()
        }

        Constraint::PartialBoundary { hole: _, slot, boundary, map, scope, origin } => {
            if !hole.set_partial(slot, &boundary, &map, &scope, &origin) {
                return vec![];
            }
            // Derive: globular PartialBoundary at all sub-slots.
            // Only done on first insertion (Empty → Partial); Partial → Partial
            // merges do not re-propagate since sub-slots were already handled.
            globular_sub_boundaries(slot, &boundary)
                .into_iter()
                .map(|(sub_slot, bd)| Constraint::PartialBoundary {
                    hole: hole_id,
                    slot: sub_slot,
                    boundary: bd,
                    map: map.clone(),
                    scope: scope.clone(),
                    origin: origin.clone(),
                })
                .collect()
        }
    }
}

/// Compute the globular sub-boundaries of `diagram` below `slot`.
///
/// For each `j < slot.dim` and both signs `s'`, computes `∂^{s'}_j(diagram)`.
/// The `slot.sign` is not used; only `slot.dim` determines the range.
/// Failures in `boundary_normal` are silently skipped.
///
/// Used by both `BoundaryEq` and `PartialBoundary` derivation so the globular
/// logic lives in exactly one place.
fn globular_sub_boundaries(slot: BdSlot, diagram: &Diagram) -> Vec<(BdSlot, Diagram)> {
    let mut result = vec![];
    for j in 0..slot.dim {
        for &sign in &[DiagramSign::Source, DiagramSign::Target] {
            if let Ok(bd) = Diagram::boundary_normal(sign, j, diagram) {
                result.push((BdSlot { sign, dim: j }, bd));
            }
        }
    }
    result
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

    // ---- BoundaryEq tests ----

    fn zero_cell(name: &str) -> Diagram {
        let tag = crate::aux::Tag::Local(name.to_string());
        Diagram::cell(tag, &crate::core::diagram::CellData::Zero).unwrap()
    }

    fn empty_scope() -> Arc<crate::core::complex::Complex> {
        Arc::new(crate::core::complex::Complex::empty())
    }

    #[test]
    fn solve_boundary_eq_fills_slot() {
        let id = HoleId::fresh();
        let diag = zero_cell("a");
        let slot = BdSlot { sign: DiagramSign::Source, dim: 0 };
        let c = Constraint::BoundaryEq {
            hole: id,
            slot,
            diagram: diag,
            scope: empty_scope(),
            origin: ConstraintOrigin::Declaration,
        };
        let result = solve(&[entry(id)], &[c]);
        assert!(result[0].boundaries.contains_key(&slot));
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_boundary_eq_consistent_duplicates_no_inconsistency() {
        // Two BoundaryEq at the same slot with isomorphic diagrams: no conflict.
        let id = HoleId::fresh();
        let slot = BdSlot { sign: DiagramSign::Source, dim: 0 };
        let c1 = Constraint::BoundaryEq {
            hole: id, slot, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Declaration,
        };
        let c2 = Constraint::BoundaryEq {
            hole: id, slot, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let result = solve(&[entry(id)], &[c1, c2]);
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_boundary_eq_conflict_records_inconsistency() {
        // Two BoundaryEq at the same slot with *different* diagrams: one inconsistency.
        let id = HoleId::fresh();
        let slot = BdSlot { sign: DiagramSign::Source, dim: 0 };
        let c1 = Constraint::BoundaryEq {
            hole: id, slot, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Declaration,
        };
        let c2 = Constraint::BoundaryEq {
            hole: id, slot, diagram: zero_cell("b"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let result = solve(&[entry(id)], &[c1, c2]);
        assert_eq!(result[0].inconsistencies.len(), 1);
    }

    #[test]
    fn solve_value_sets_value() {
        let id = HoleId::fresh();
        let c = Constraint::Value {
            hole: id,
            diagram: zero_cell("a"),
            scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let result = solve(&[entry(id)], &[c]);
        assert!(result[0].value.is_some());
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_value_conflict_records_inconsistency() {
        // Two Value constraints with different diagrams: inconsistency recorded, value = first.
        let id = HoleId::fresh();
        let c1 = Constraint::Value {
            hole: id, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let c2 = Constraint::Value {
            hole: id, diagram: zero_cell("b"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let result = solve(&[entry(id)], &[c1, c2]);
        assert_eq!(result[0].inconsistencies.len(), 1);
        assert!(result[0].value.is_some(), "value should be set from first Value");
    }

    #[test]
    fn solve_value_consistent_no_inconsistency() {
        // Two Value constraints with the same diagram: no conflict.
        let id = HoleId::fresh();
        let c1 = Constraint::Value {
            hole: id, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let c2 = Constraint::Value {
            hole: id, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let result = solve(&[entry(id)], &[c1, c2]);
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_value_infers_dim() {
        // A Value constraint on a 0-cell should derive DimEq(0).
        let id = HoleId::fresh();
        let c = Constraint::Value {
            hole: id,
            diagram: zero_cell("a"),
            scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let result = solve(&[entry(id)], &[c]);
        assert_eq!(result[0].dim, Some(0), "dim should be derived from Value");
        assert!(result[0].inconsistencies.is_empty());
    }

    #[test]
    fn solve_value_conflict_with_dim_eq_records_inconsistency() {
        // Value(0-cell) → DimEq(0); explicit DimEq(1) conflicts.
        let id = HoleId::fresh();
        let c1 = Constraint::Value {
            hole: id, diagram: zero_cell("a"), scope: empty_scope(),
            origin: ConstraintOrigin::Assertion,
        };
        let c2 = Constraint::DimEq { hole: id, dim: 1, origin: ConstraintOrigin::Declaration };
        // Order: DimEq first, then Value.  Value derives DimEq(0) which conflicts with the
        // already-set dim=1.
        let result = solve(&[entry(id)], &[c2, c1]);
        assert_eq!(result[0].dim, Some(1), "first DimEq wins");
        assert_eq!(result[0].inconsistencies.len(), 1, "derived DimEq(0) conflicts with explicit DimEq(1)");
    }

    #[test]
    fn solve_boundary_eq_upgrades_partial() {
        // A PartialBoundary followed by a BoundaryEq at the same slot:
        // the slot should be upgraded to Known with no inconsistency.
        use crate::core::partial_map::PartialMap;
        let id = HoleId::fresh();
        let slot = BdSlot { sign: DiagramSign::Source, dim: 0 };
        let diag = zero_cell("a");
        let c_partial = Constraint::PartialBoundary {
            hole: id, slot,
            boundary: diag.clone(),
            map: PartialMap::empty(),
            scope: empty_scope(),
            origin: ConstraintOrigin::PartialMap {
                source_tag: crate::aux::Tag::Local("src".into())
            },
        };
        let c_known = Constraint::BoundaryEq {
            hole: id, slot, diagram: diag, scope: empty_scope(),
            origin: ConstraintOrigin::Declaration,
        };
        let result = solve(&[entry(id)], &[c_partial, c_known]);
        assert!(
            matches!(result[0].boundaries.get(&slot), Some(SolvedBd::Known { .. })),
            "BoundaryEq should upgrade the slot from Partial to Known"
        );
        assert!(result[0].inconsistencies.is_empty());
    }
}
