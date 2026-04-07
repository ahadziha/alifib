use super::inference::{BdSlot, Constraint, ConstraintOrigin, HoleId};
use super::resolve::resolve_map_domain_complex;
use super::types::{
    Component, Context, EvalMap, HoleInfo, InterpResult, Step,
    Term, TermPair, fail, make_error, make_error_from_core, sorted_generators,
};
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    partial_map::PartialMap,
};
use crate::language::ast::{self, DComponent, DExpr, PartialMapBasic, Span, Spanned};
use std::sync::Arc;

// ---- Helpers ----

/// Extract a `Diagram` from an `Option<Term>`, recording an error if it is a map.
fn require_diagram_term(
    term: Option<Term>,
    mut result: InterpResult,
    span: Span,
) -> (Option<Diagram>, InterpResult) {
    match term {
        None => (None, result),
        Some(Term::Diag(d)) => (Some(d), result),
        Some(Term::Map(_)) => {
            result.add_error(make_error(span, "Not a diagram"));
            (None, result)
        }
    }
}

/// Parse a dimension string into a `usize`, failing with an error on invalid input.
fn parse_paste_dim(context: &Context, dim: &Spanned<String>) -> Step<usize> {
    dim.inner
        .parse::<usize>()
        .map(|k| (Some(k), InterpResult::ok(context.clone())))
        .unwrap_or_else(|_| fail(context, dim.span, format!("Invalid paste dimension: {}", dim.inner)))
}


/// Emit `DimEq` and principal `BoundaryEq` constraints for `hole_id` parallel to `companion`.
///
/// Pushes `DimEq(hole, n)` and, when `n > 0`, `BoundaryEq` at the two principal
/// slots `(Source, n-1)` and `(Target, n-1)`.  Lower-dimensional slots are derived
/// afterwards by `globular_propagate` in the solver.
pub(super) fn push_parallel_constraints(
    hole_id: HoleId,
    companion: &Diagram,
    scope: &Arc<Complex>,
    origin: ConstraintOrigin,
    constraints: &mut Vec<Constraint>,
) {
    let n = companion.top_dim();
    constraints.push(Constraint::DimEq { hole: hole_id, dim: n, origin: origin.clone() });
    if n > 0 {
        for &sign in &[DiagramSign::Source, DiagramSign::Target] {
            if let Ok(bd) = Diagram::boundary_normal(sign, n - 1, companion) {
                constraints.push(Constraint::BoundaryEq {
                    hole: hole_id,
                    slot: BdSlot { sign, dim: n - 1 },
                    diagram: bd,
                    scope: scope.clone(),
                    origin: origin.clone(),
                });
            }
        }
    }
}

/// Returns `true` if `diagram` is a single un-parenthesized or parenthesized `?`.
pub(super) fn is_pure_hole_diagram(diagram: &ast::Diagram) -> bool {
    match diagram {
        ast::Diagram::PrincipalPaste(exprs) if exprs.len() == 1 => {
            is_pure_hole_dexpr(&exprs[0].inner)
        }
        _ => false,
    }
}

pub(super) fn is_pure_hole_dexpr(expr: &ast::DExpr) -> bool {
    match expr {
        ast::DExpr::Component(ast::DComponent::Hole) => true,
        ast::DExpr::Component(ast::DComponent::Paren(inner)) => {
            is_pure_hole_diagram(&inner.inner)
        }
        _ => false,
    }
}

// ---- Diagram interpretation ----

/// Compute a boundary term from a diagram at one dimension below its top.
fn boundary_term_from_diagram(
    diagram: &Diagram,
    sign: DiagramSign,
    span: Span,
    mut result: InterpResult,
) -> (Option<Term>, InterpResult) {
    let boundary_dim = diagram.top_dim().saturating_sub(1);
    match Diagram::boundary(sign, boundary_dim, diagram) {
        Ok(boundary) => (Some(Term::Diag(boundary)), result),
        Err(error) => {
            result.add_error(make_error_from_core(span, error));
            (None, result)
        }
    }
}

/// Apply a partial map to a component, producing the image term.
///
/// Diagrams are mapped by `PartialMap::apply`; inner maps are composed.
/// A hole produces a recorded `HoleInfo`; a boundary direction is an error.
fn apply_map_component(
    eval_map: &EvalMap,
    component: Component,
    span: Span,
    mut result: InterpResult,
) -> (Option<Term>, InterpResult) {
    match component {
        Component::Hole => {
            result.add_hole(HoleInfo::new(span));
            (None, result)
        }
        Component::Bd(_) => {
            result.add_error(make_error(span, "Not a diagram or map"));
            (None, result)
        }
        Component::Value(Term::Diag(diagram)) => match PartialMap::apply(&eval_map.map, &diagram) {
            Ok(image_diagram) => (Some(Term::Diag(image_diagram)), result),
            Err(error) => {
                result.add_error(make_error_from_core(span, error));
                (None, result)
            }
        },
        Component::Value(Term::Map(inner_map)) => {
            let composed = PartialMap::compose(&eval_map.map, &inner_map.map);
            (
                Some(Term::Map(EvalMap {
                    map: composed,
                    domain: inner_map.domain,
                })),
                result,
            )
        }
    }
}

/// Interpret a diagram expression, rejecting partial maps.
///
/// Delegates to [`interpret_diagram_as_term`] and extracts the `Diagram`.
pub fn interpret_diagram(
    context: &Context,
    scope: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Diagram>, InterpResult) {
    let (term_opt, mut result) = interpret_diagram_as_term(context, scope, diagram);
    let Some(term) = term_opt else { return (None, result); };
    match term {
        Term::Diag(d) => (Some(d), result),
        Term::Map(_) => {
            result.add_error(make_error(diagram.span, "Expected a diagram, not a partial map"));
            (None, result)
        }
    }
}

/// Interpret a diagram expression, which may be a component or a dot-access chain.
pub fn interpret_dexpr(
    context: &Context,
    scope: &Complex,
    d_expr: &Spanned<DExpr>,
) -> (Option<Term>, InterpResult) {
    match &d_expr.inner {
        DExpr::Component(comp) => {
            let (comp_opt, mut result) = interpret_dcomponent(context, scope, comp, d_expr.span);
            match comp_opt {
                None => (None, result),
                Some(Component::Hole) => {
                    result.add_hole(HoleInfo::new(d_expr.span));
                    (None, result)
                }
                Some(Component::Bd(_)) => {
                    result.add_error(make_error(d_expr.span, "Not a diagram or map"));
                    (None, result)
                }
                Some(Component::Value(t)) => (Some(t), result),
            }
        }
        DExpr::Dot { base, field } => interpret_dot_access(context, scope, base, field),
    }
}

/// Interpret a dot-access expression: `expr.field`.
///
/// If `expr` evaluates to a diagram, `field` must be `.in` or `.out` (a boundary selector).
/// If `expr` evaluates to a partial map, `field` is applied as a map component.
fn interpret_dot_access(
    context: &Context,
    scope: &Complex,
    base: &Spanned<DExpr>,
    field: &Spanned<DComponent>,
) -> (Option<Term>, InterpResult) {
    let (left_opt, left_result) = interpret_dexpr(context, scope, base);
    match left_opt {
        None => (None, left_result),
        Some(Term::Diag(diagram)) => {
            let (comp_opt, comp_result) =
                interpret_dcomponent(&left_result.context, scope, &field.inner, field.span);
            let combined = left_result.merge(comp_result);
            match comp_opt {
                None => (None, combined),
                Some(Component::Bd(sign)) => {
                    boundary_term_from_diagram(&diagram, sign, field.span, combined)
                }
                Some(Component::Hole) => {
                    let mut r = combined;
                    r.add_hole(HoleInfo::new(field.span));
                    (None, r)
                }
                Some(Component::Value(_)) => {
                    let mut r = combined;
                    r.add_error(make_error(field.span, "Not a well-formed diagram expression"));
                    (None, r)
                }
            }
        }
        Some(Term::Map(eval_map)) => {
            let (comp_opt, comp_result) = interpret_dcomponent(
                &left_result.context,
                &eval_map.domain,
                &field.inner,
                field.span,
            );
            let combined = left_result.merge(comp_result);
            match comp_opt {
                None => (None, combined),
                Some(component) => apply_map_component(&eval_map, component, field.span, combined),
            }
        }
    }
}

/// Interpret a single diagram component: name lookup, anonymous map, parenthesized
/// subexpression, `.in`/`.out` boundary selector, or a hole `?`.
pub fn interpret_dcomponent(
    context: &Context,
    scope: &Complex,
    d_comp: &DComponent,
    span: Span,
) -> (Option<Component>, InterpResult) {
    match d_comp {
        DComponent::PartialMap(PartialMapBasic::Name(name)) => {
            if let Some(diagram) = scope.find_diagram(name) {
                return (Some(Component::Value(Term::Diag(diagram.clone()))), InterpResult::ok(context.clone()));
            }
            if let Some((map, domain)) = scope.find_map(name) {
                let (domain_opt, result) = resolve_map_domain_complex(context, domain, span);
                let Some(domain) = domain_opt else { return (None, result); };
                return (Some(Component::Value(Term::Map(EvalMap { map: map.clone(), domain }))), InterpResult::ok(context.clone()));
            }
            fail(context, span, format!("Name `{}` not found", name))
        }
        DComponent::PartialMap(PartialMapBasic::AnonMap { def, target }) => {
            let (eval_map_opt, result) = super::partial_map::interpret_anon_map_component(context, scope, target, def);
            (eval_map_opt.map(|em| Component::Value(Term::Map(em))), result)
        }
        DComponent::PartialMap(PartialMapBasic::Paren(inner_pmap)) => {
            let (eval_map_opt, result) = super::partial_map::interpret_partial_map(context, scope, scope, inner_pmap);
            (eval_map_opt.map(|em| Component::Value(Term::Map(em))), result)
        }
        DComponent::In => (Some(Component::Bd(DiagramSign::Source)), InterpResult::ok(context.clone())),
        DComponent::Out => (Some(Component::Bd(DiagramSign::Target)), InterpResult::ok(context.clone())),
        DComponent::Paren(inner_diag) => {
            let (d_opt, result) = interpret_diagram(context, scope, inner_diag);
            (d_opt.map(|d| Component::Value(Term::Diag(d))), result)
        }
        DComponent::Hole => (Some(Component::Hole), InterpResult::ok(context.clone())),
    }
}

// ---- Assert ----

/// Evaluate both sides of an assertion statement and pair them for equality checking.
///
/// Both sides are always evaluated even when one fails, so that holes are detected in
/// a single pass.  When one side is a concrete diagram and the other has holes, those
/// holes are enriched with the concrete diagram's source/target boundaries: the hole
/// must be a diagram equal to the concrete side, so it must share its boundaries.
pub fn interpret_assert(
    context: &Context,
    scope: &Complex,
    assert_stmt: &crate::language::ast::AssertStmt,
) -> (Option<TermPair>, InterpResult) {
    let (left_opt, left_result) = interpret_diagram_as_term(context, scope, &assert_stmt.lhs);
    let lhs_hole_count = left_result.holes.len();

    // Always evaluate RHS even if LHS failed.
    let (right_opt, right_result) =
        interpret_diagram_as_term(&left_result.context, scope, &assert_stmt.rhs);
    let mut combined = left_result.merge(right_result);

    // Constraint system: when the entire other side is a single `?`, pin the hole's
    // exact value and emit parallel boundary constraints.  For embedded holes (e.g.,
    // `f ? g = h`) the paste context already emits BoundaryEq; claiming Value(hole, h)
    // would be wrong.
    let scope_arc = Arc::new(scope.clone());
    if let Some(Term::Diag(ref d)) = right_opt {
        if is_pure_hole_diagram(&assert_stmt.lhs.inner) {
            for hole in &combined.holes[..lhs_hole_count] {
                combined.constraints.push(Constraint::Value {
                    hole: hole.id,
                    diagram: d.clone(),
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Assertion,
                });
                // DimEq and principal BoundaryEq are derived by the solver from Value.
            }
        }
    }
    if let Some(Term::Diag(ref d)) = left_opt {
        if is_pure_hole_diagram(&assert_stmt.rhs.inner) {
            for hole in &combined.holes[lhs_hole_count..] {
                combined.constraints.push(Constraint::Value {
                    hole: hole.id,
                    diagram: d.clone(),
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Assertion,
                });
                // DimEq and principal BoundaryEq are derived by the solver from Value.
            }
        }
    }

    match (left_opt, right_opt) {
        (Some(Term::Diag(d1)), Some(Term::Diag(d2))) => {
            (Some(TermPair::Diagrams { fst: d1, snd: d2 }), combined)
        }
        (Some(Term::Map(mc1)), Some(Term::Map(mc2))) => (
            Some(TermPair::Maps { fst: mc1.map, snd: mc2.map, domain: mc1.domain }),
            combined,
        ),
        (Some(_), Some(_)) => {
            combined.add_error(make_error(assert_stmt.lhs.span, "The two sides of the equation are incomparable"));
            (None, combined)
        }
        _ => (None, combined),
    }
}

/// Interpret a diagram AST node as a term (diagram or map).
///
/// Dispatches on whether the node is an explicit paste (`*k`) or an implicit
/// juxtaposition sequence.
pub fn interpret_diagram_as_term(
    context: &Context,
    scope: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Term>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::PrincipalPaste(exprs) => {
            interpret_sequence_as_term(context, scope, exprs, diagram.span)
        }
        ast::Diagram::Paste { lhs, dim, rhs } => {
            interpret_paste(context, scope, lhs, dim, rhs, diagram.span)
        }
    }
}

/// Interpret an explicit paste `lhs *k rhs`. The right-hand side is evaluated first
/// (it determines the context for the left), then both are pasted at dimension k.
///
/// Both sides are evaluated even when one has a hole or error, so that holes are
/// reported in a single pass and each side's holes can be enriched with the other
/// side's k-dimensional boundary.
fn interpret_paste(
    context: &Context,
    scope: &Complex,
    lhs: &Spanned<ast::Diagram>,
    dim: &Spanned<String>,
    rhs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Term>, InterpResult) {
    let (k_opt, k_result) = parse_paste_dim(context, dim);
    let Some(k) = k_opt else { return (None, k_result); };

    // Evaluate RHS first; record how many holes it produced before evaluating LHS.
    let (right_term, right_result) = interpret_sequence_as_term(context, scope, rhs, span);
    let (d_right_opt, right_result) = require_diagram_term(right_term, right_result, span);
    let rhs_hole_count = right_result.holes.len();

    // Always evaluate LHS, even if RHS has holes or errors.
    let (left_term, left_result) = interpret_diagram_as_term(&right_result.context, scope, lhs);
    let mut combined = right_result.merge(left_result);
    let d_left_opt = match left_term {
        Some(Term::Diag(d)) => Some(d),
        Some(Term::Map(_)) => {
            combined.add_error(make_error(span, "Not a diagram"));
            None
        }
        None => None,
    };

    // Constraint system: BoundaryEq and DimEq from each concrete side.
    // LHS holes (index rhs_hole_count..) get boundary_out from RHS.source_k.
    // RHS holes (index ..rhs_hole_count) get boundary_in from LHS.target_k.
    // Both sides also learn their dimension from the concrete partner.
    let scope_arc = Arc::new(scope.clone());
    if let Some(ref d_right) = d_right_opt {
        if let Ok(in_bd) = Diagram::boundary_normal(DiagramSign::Source, k, d_right) {
            for hole in &combined.holes[rhs_hole_count..] {
                combined.constraints.push(Constraint::BoundaryEq {
                    hole: hole.id,
                    slot: BdSlot { sign: DiagramSign::Target, dim: k },
                    diagram: in_bd.clone(),
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Paste { paste_dim: k },
                });
            }
        }
        let n = d_right.top_dim();
        for hole in &combined.holes[rhs_hole_count..] {
            combined.constraints.push(Constraint::DimEq {
                hole: hole.id,
                dim: n,
                origin: ConstraintOrigin::Paste { paste_dim: k },
            });
        }
    }
    if let Some(ref d_left) = d_left_opt {
        if let Ok(out_bd) = Diagram::boundary_normal(DiagramSign::Target, k, d_left) {
            for hole in &combined.holes[..rhs_hole_count] {
                combined.constraints.push(Constraint::BoundaryEq {
                    hole: hole.id,
                    slot: BdSlot { sign: DiagramSign::Source, dim: k },
                    diagram: out_bd.clone(),
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Paste { paste_dim: k },
                });
            }
        }
        let n = d_left.top_dim();
        for hole in &combined.holes[..rhs_hole_count] {
            combined.constraints.push(Constraint::DimEq {
                hole: hole.id,
                dim: n,
                origin: ConstraintOrigin::Paste { paste_dim: k },
            });
        }
    }

    match (d_left_opt, d_right_opt) {
        (Some(d_left), Some(d_right)) => match Diagram::paste(k, &d_left, &d_right) {
            Ok(d) => (Some(Term::Diag(d)), combined),
            Err(e) => {
                combined.add_error(make_error(span, format!("Failed to paste diagrams: {}", e)));
                (None, combined)
            }
        },
        _ => (None, combined),
    }
}

/// Interpret a juxtaposition sequence of diagram expressions.
///
/// A single expression evaluates to its term (diagram or map). Multiple
/// expressions are pasted left-to-right; all must be diagrams.
///
/// Holes do not abort the loop. Instead, evaluation continues past each
/// hole so that both its left and right boundary can be inferred from the
/// concrete diagrams immediately adjacent to it. Two paste accumulators are
/// maintained:
///
/// * `left_acc` — paste of concrete diagrams before the first hole.
/// * `right_acc` — paste of concrete diagrams since the most recent hole;
///   reset to `None` whenever a new hole block begins.
///
/// `last_hole_block_start` records the first index (into `result.holes`) of
/// the current run of consecutive holes so that the next concrete diagram can
/// enrich the right boundary of all of them.
///
/// Source-boundary constraints are *deferred* until the right neighbour is
/// known so that both source and target constraints use a consistent paste
/// dimension `k = min(left.dim, right.dim) - 1`, matching the behaviour of
/// concrete paste.  Trailing holes (no right neighbour) have their source
/// constraint emitted after the loop using only the left neighbour.
fn interpret_sequence_as_term(
    context: &Context,
    scope: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Term>, InterpResult) {
    if exprs.is_empty() {
        return fail(context, span, "Empty diagram expression");
    }

    // Single expression: evaluate directly (may be a diagram or a map).
    if exprs.len() == 1 {
        return interpret_dexpr(context, scope, &exprs[0]);
    }

    let mut left_acc: Option<Diagram> = None;
    let mut right_acc: Option<Diagram> = None;
    // First index in `result.holes` of the current hole block.
    let mut last_hole_block_start: Option<usize> = None;
    // The concrete diagram immediately to the left of the current hole block.
    // Captured when the block opens; used (deferred) to emit the source constraint.
    let mut hole_block_left: Option<Diagram> = None;
    let mut has_holes = false;
    let mut result = InterpResult::ok(context.clone());

    for expr in exprs {
        let prev_hole_count = result.holes.len();
        let (term_opt, expr_result) = interpret_dexpr(&result.context, scope, expr);
        result = result.merge(expr_result);
        let new_holes = result.holes.len() > prev_hole_count;

        match term_opt {
            None if new_holes => {
                // Start a new hole block (or extend the current one).
                // Source-boundary emission is deferred until the right neighbour arrives.
                if last_hole_block_start.is_none() {
                    last_hole_block_start = Some(prev_hole_count);
                    hole_block_left = right_acc.clone().or_else(|| left_acc.clone());
                }
                right_acc = None;
                has_holes = true;
            }
            None => {
                // An error occurred (not a hole): abort.
                return (None, result);
            }
            Some(Term::Map(_)) => {
                result.add_error(make_error(expr.span, "Not a diagram"));
                return (None, result);
            }
            Some(Term::Diag(d_right)) => {
                // Enrich the current hole block now that we have a right neighbour.
                if let Some(start) = last_hole_block_start {
                    let scope_arc = Arc::new(scope.clone());
                    let block_hole_ids: Vec<_> =
                        result.holes[start..prev_hole_count].iter().map(|h| h.id).collect();

                    // Consistent paste dimension from both neighbours.
                    let k = match hole_block_left.as_ref() {
                        Some(left_diag) => left_diag.top_dim().min(d_right.top_dim()).saturating_sub(1),
                        None => d_right.top_dim().saturating_sub(1),
                    };

                    // Deferred source constraint: only the *first* hole in the block
                    // has its source determined by the left neighbour.  For a block of
                    // multiple holes `a ?₁ ?₂ … ?ₙ b`, target(?ₙ₋₁) = source(?ₙ) is
                    // unknown; over-constraining the inner holes produces false
                    // inconsistencies.
                    if let (Some(left_diag), Some(&first_id)) =
                        (&hole_block_left, block_hole_ids.first())
                    {
                        if let Ok(out_bd) = Diagram::boundary_normal(DiagramSign::Target, k, left_diag) {
                            result.constraints.push(Constraint::BoundaryEq {
                                hole: first_id,
                                slot: BdSlot { sign: DiagramSign::Source, dim: k },
                                diagram: out_bd,
                                scope: scope_arc.clone(),
                                origin: ConstraintOrigin::Paste { paste_dim: k },
                            });
                        }
                    }

                    // Target constraint: only the *last* hole in the block has its
                    // target determined by the right neighbour.
                    if let Some(&last_id) = block_hole_ids.last() {
                        if let Ok(in_bd) = Diagram::boundary_normal(DiagramSign::Source, k, &d_right) {
                            result.constraints.push(Constraint::BoundaryEq {
                                hole: last_id,
                                slot: BdSlot { sign: DiagramSign::Target, dim: k },
                                diagram: in_bd,
                                scope: scope_arc.clone(),
                                origin: ConstraintOrigin::Paste { paste_dim: k },
                            });
                        }
                    }

                    // Dimension constraints: holes must match both neighbours' dimensions.
                    // Emitting from both sides means a left/right dimension mismatch (which
                    // would make the paste ill-typed) is caught as a DimEq inconsistency.
                    let n_right = d_right.top_dim();
                    for &id in &block_hole_ids {
                        result.constraints.push(Constraint::DimEq {
                            hole: id,
                            dim: n_right,
                            origin: ConstraintOrigin::Paste { paste_dim: k },
                        });
                    }
                    if let Some(ref left_diag) = hole_block_left {
                        let n_left = left_diag.top_dim();
                        for &id in &block_hole_ids {
                            result.constraints.push(Constraint::DimEq {
                                hole: id,
                                dim: n_left,
                                origin: ConstraintOrigin::Paste { paste_dim: k },
                            });
                        }
                    }

                    last_hole_block_start = None;
                    hole_block_left = None;
                }
                // Paste into the appropriate accumulator.
                let acc = if has_holes { &mut right_acc } else { &mut left_acc };
                let next = match acc.take() {
                    None => Ok(d_right),
                    Some(prev) => {
                        let k = prev.top_dim().min(d_right.top_dim()).saturating_sub(1);
                        Diagram::paste(k, &prev, &d_right)
                    }
                };
                match next {
                    Ok(d) => *acc = Some(d),
                    Err(e) => {
                        result.add_error(make_error(span, format!("Failed to paste diagrams: {}", e)));
                        return (None, result);
                    }
                }
            }
        }
    }

    // Emit deferred source constraint for trailing holes (no right neighbour).
    // Only the *first* trailing hole has its source determined by the left
    // neighbour; inner holes in the block are unconstrained on that side.
    if let Some(start) = last_hole_block_start {
        if let Some(ref left_diag) = hole_block_left {
            let k = left_diag.top_dim().saturating_sub(1);
            let n = left_diag.top_dim();
            let scope_arc = Arc::new(scope.clone());
            let trailing_ids: Vec<HoleId> = result.holes[start..].iter().map(|h| h.id).collect();
            if let (Ok(out_bd), Some(&first_id)) =
                (Diagram::boundary_normal(DiagramSign::Target, k, left_diag), trailing_ids.first())
            {
                result.constraints.push(Constraint::BoundaryEq {
                    hole: first_id,
                    slot: BdSlot { sign: DiagramSign::Source, dim: k },
                    diagram: out_bd,
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Paste { paste_dim: k },
                });
            }
            // Dimension constraint from left neighbour for trailing holes.
            for &id in &trailing_ids {
                result.constraints.push(Constraint::DimEq {
                    hole: id,
                    dim: n,
                    origin: ConstraintOrigin::Paste { paste_dim: k },
                });
            }
        }
    }

    if has_holes {
        (None, result)
    } else {
        match left_acc {
            Some(d) => (Some(Term::Diag(d)), result),
            None => fail(&result.context, span, "Empty diagram expression"),
        }
    }
}

// ---- Boundaries ----

/// Interpret a source/target boundary pair and wrap the result as `CellData::Boundary`.
///
/// Both sides are always evaluated so that holes in either boundary are detected in a
/// single pass.  Holes in the source boundary are enriched with the target diagram as
/// their `boundary_out` (and vice versa), so the reported context reads naturally as
/// `? -> target` or `source -> ?`.
pub fn interpret_boundaries(
    context: &Context,
    scope: &Complex,
    boundaries: &Spanned<ast::Boundary>,
) -> (Option<CellData>, InterpResult) {
    let (source_opt, source_result) = interpret_diagram(context, scope, &boundaries.inner.source);
    let pre_target_holes = source_result.holes.len();
    // Always evaluate the target even if the source has a hole or error.
    let (target_opt, target_result) =
        interpret_diagram(&source_result.context, scope, &boundaries.inner.target);
    let mut combined = source_result.merge(target_result);

    // Constraint system: a hole in source position must be parallel to the target,
    // and vice versa.  Decomposed eagerly into DimEq + BoundaryEq at principal slots.
    let scope_arc = Arc::new(scope.clone());
    if let Some(ref tgt) = target_opt {
        for hole in &combined.holes[..pre_target_holes] {
            push_parallel_constraints(hole.id, tgt, &scope_arc, ConstraintOrigin::Declaration, &mut combined.constraints);
        }
    }
    if let Some(ref src) = source_opt {
        for hole in &combined.holes[pre_target_holes..] {
            push_parallel_constraints(hole.id, src, &scope_arc, ConstraintOrigin::Declaration, &mut combined.constraints);
        }
    }

    match (source_opt, target_opt) {
        (Some(boundary_in), Some(boundary_out)) => (
            Some(CellData::Boundary {
                boundary_in: Arc::new(boundary_in),
                boundary_out: Arc::new(boundary_out),
            }),
            combined,
        ),
        _ => (None, combined),
    }
}

// ---- Diagram naming ----

/// Interpret a `let` diagram binding, producing a `(name, diagram)` pair.
pub fn interpret_let_diag(
    context: &Context,
    scope: &Complex,
    ld: &crate::language::ast::LetDiag,
) -> (Option<(String, Diagram)>, InterpResult) {
    let (diag_opt, result) = interpret_diagram(context, scope, &ld.value);
    (diag_opt.map(|d| (ld.name.inner.clone(), d)), result)
}

// ---- Assert checking ----

/// Check that two evaluated terms are equal: diagrams up to isomorphism,
/// maps pointwise on all generators in the domain.
pub fn check_assert(pair: &TermPair) -> Result<(), String> {
    match pair {
        TermPair::Diagrams { fst, snd } => {
            if Diagram::isomorphic(fst, snd) {
                Ok(())
            } else {
                Err("The diagrams are not equal".into())
            }
        }
        TermPair::Maps { fst, snd, domain } => {
            for (_, gen_name, tag) in sorted_generators(domain) {
                match (fst.is_defined_at(&tag), snd.is_defined_at(&tag)) {
                    (true, false) => return Err(format!(
                        "`{}` is in the domain of the first map but not the second",
                        gen_name
                    )),
                    (false, true) => return Err(format!(
                        "`{}` is in the domain of the second map but not the first",
                        gen_name
                    )),
                    (true, true) => {
                        let img1 = fst.image(&tag).map_err(|e| e.to_string())?;
                        let img2 = snd.image(&tag).map_err(|e| e.to_string())?;
                        if !Diagram::isomorphic(img1, img2) {
                            return Err(format!("The maps differ on `{}`", gen_name));
                        }
                    }
                    (false, false) => {}
                }
            }
            Ok(())
        }
    }
}
