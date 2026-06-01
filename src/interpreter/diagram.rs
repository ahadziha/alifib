use super::inference::{BdSlot, Constraint, ConstraintOrigin, HoleId};
use super::resolve::resolve_map_domain_complex;
use super::types::{
    Component, Context, EvalMap, HoleInfo, InterpResult, Step,
    Term, TermPair, fail, make_error, make_error_from_core, sorted_generators,
};
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    matching::{build_rule_patterns, greedy_parallel_auto_step},
    partial_map::PartialMap,
};
use crate::language::ast::{self, DComponent, DExpr, Span, Spanned};
use std::sync::Arc;

// ---- Helpers ----

fn try_dotted_name(expr: &DExpr) -> Option<String> {
    match expr {
        DExpr::Component(DComponent::Name(s)) => Some(s.clone()),
        DExpr::Dot { base, field } => {
            let prefix = try_dotted_name(&base.inner)?;
            match &field.inner {
                DComponent::Name(s) => Some(format!("{}.{}", prefix, s)),
                _ => None,
            }
        }
        _ => None,
    }
}

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
        for &sign in &[DiagramSign::Input, DiagramSign::Output] {
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

/// A structural decomposition of a dotted expression into deferred parts.
///
/// A well-formed dotted expression is a prefix of partial maps, then a single
/// basic diagram, then a suffix of boundary operators.  Rather than composing
/// maps and applying them eagerly while walking the chain, [`decompose`] just
/// collects the pieces — doing only cheap name lookups and map evaluation — and
/// [`execute`] performs the heavy work in the efficient order: take the
/// diagram's boundary *directly* in one call, then apply the maps to that
/// (small) boundary from the innermost outward.  No composite map is ever built.
enum Decomp {
    /// Maps (outermost first; `maps[0]` is applied last) wrapping a diagram,
    /// then boundary operators in source order, each tagged with its span.
    Diagram {
        maps: Vec<EvalMap>,
        diagram: Diagram,
        diagram_span: Span,
        bds: Vec<(DiagramSign, Span)>,
    },
    /// A non-empty map chain denoting the composite `maps[0] ∘ … ∘ maps[last]`.
    Map { maps: Vec<EvalMap> },
    /// The expression contains a hole (`?`), which has been recorded.
    Hole,
}

/// Collect a dotted expression into a [`Decomp`] without composing or applying.
///
/// Mirrors the scoping of the eager reading exactly: the whole-expression
/// fast-path (a qualified name) is retried at every prefix level against the
/// *outer* `scope`, while a `field` following a map prefix is resolved in that
/// map's own domain.  A failed or hole-terminated base swallows the rest.
fn decompose(
    context: &Context,
    scope: &Complex,
    expr: &Spanned<DExpr>,
) -> (Option<Decomp>, InterpResult) {
    match &expr.inner {
        DExpr::Component(comp) => {
            let (comp_opt, mut result) = interpret_dcomponent(context, scope, comp, expr.span);
            match comp_opt {
                None => (None, result),
                Some(Component::Value(Term::Diag(diagram))) => (
                    Some(Decomp::Diagram {
                        maps: Vec::new(),
                        diagram,
                        diagram_span: expr.span,
                        bds: Vec::new(),
                    }),
                    result,
                ),
                Some(Component::Value(Term::Map(m))) => {
                    (Some(Decomp::Map { maps: vec![m] }), result)
                }
                Some(Component::Hole) => {
                    result.add_hole(HoleInfo::new(expr.span));
                    (Some(Decomp::Hole), result)
                }
                Some(Component::Bd(_)) => {
                    result.add_error(make_error(expr.span, "Not a diagram or map"));
                    (None, result)
                }
            }
        }
        DExpr::Dot { base, field } => {
            // Fast path: the entire dotted name is a generator or diagram in scope.
            if let Some(dotted) = try_dotted_name(&expr.inner) {
                if let Some(found) = scope.find_diagram(&dotted).or_else(|| scope.classifier(&dotted)) {
                    return (
                        Some(Decomp::Diagram {
                            maps: Vec::new(),
                            diagram: found.clone(),
                            diagram_span: expr.span,
                            bds: Vec::new(),
                        }),
                        InterpResult::ok(context.clone()),
                    );
                }
            }

            let (base_opt, base_result) = decompose(context, scope, base);
            let base_decomp = match base_opt {
                None => return (None, base_result),
                Some(Decomp::Hole) => return (Some(Decomp::Hole), base_result),
                Some(d) => d,
            };

            match base_decomp {
                // After a diagram, only boundary operators may follow.
                Decomp::Diagram { maps, diagram, diagram_span, mut bds } => {
                    let (comp_opt, comp_result) =
                        interpret_dcomponent(&base_result.context, scope, &field.inner, field.span);
                    let mut combined = base_result.merge(comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Bd(sign)) => {
                            bds.push((sign, field.span));
                            (Some(Decomp::Diagram { maps, diagram, diagram_span, bds }), combined)
                        }
                        Some(Component::Hole) => {
                            combined.add_hole(HoleInfo::new(field.span));
                            (Some(Decomp::Hole), combined)
                        }
                        Some(Component::Value(_)) => {
                            combined.add_error(make_error(field.span, "Not a well-formed diagram expression"));
                            (None, combined)
                        }
                    }
                }
                // The field is resolved in the innermost map's domain.
                Decomp::Map { mut maps } => {
                    let domain = maps.last().expect("Map decomp is non-empty").domain.clone();
                    let (comp_opt, comp_result) =
                        interpret_dcomponent(&base_result.context, &domain, &field.inner, field.span);
                    let mut combined = base_result.merge(comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Value(Term::Map(m))) => {
                            maps.push(m);
                            (Some(Decomp::Map { maps }), combined)
                        }
                        Some(Component::Value(Term::Diag(diagram))) => (
                            Some(Decomp::Diagram { maps, diagram, diagram_span: field.span, bds: Vec::new() }),
                            combined,
                        ),
                        Some(Component::Hole) => {
                            combined.add_hole(HoleInfo::new(field.span));
                            (Some(Decomp::Hole), combined)
                        }
                        Some(Component::Bd(_)) => {
                            combined.add_error(make_error(field.span, "Not a diagram or map"));
                            (None, combined)
                        }
                    }
                }
                Decomp::Hole => unreachable!("hole bases return early"),
            }
        }
    }
}

/// Execute a [`Decomp`]: take the boundary directly, then apply maps inward-out.
///
/// The boundary suffix collapses to a single [`Diagram::boundary`] call — only
/// the last operator's polarity and the number of operators matter.  Because
/// maps preserve boundaries, applying them *after* the boundary (to the small
/// boundary diagram) agrees with the eager reading while touching far fewer
/// cells.  A pure map chain is composed once, from the innermost map outward.
fn execute(decomp: Option<Decomp>, mut result: InterpResult) -> (Option<Term>, InterpResult) {
    match decomp {
        None | Some(Decomp::Hole) => (None, result),
        Some(Decomp::Diagram { maps, diagram, diagram_span, bds }) => {
            let mut current = diagram;
            if let Some(&(last_sign, last_span)) = bds.last() {
                let n = current.top_dim();
                if bds.len() > n {
                    // The first operator that drops below dimension 0.
                    result.add_error(make_error(bds[n].1, "diagram has no principal boundary"));
                    return (None, result);
                }
                match Diagram::boundary(last_sign, n - bds.len(), &current) {
                    Ok(b) => current = b,
                    Err(error) => {
                        result.add_error(make_error_from_core(last_span, error));
                        return (None, result);
                    }
                }
            }
            // Apply maps from the innermost (nearest the diagram) outward.
            for m in maps.iter().rev() {
                match PartialMap::apply(&m.map, &current) {
                    Ok(image) => current = image,
                    Err(error) => {
                        result.add_error(make_error_from_core(diagram_span, error));
                        return (None, result);
                    }
                }
            }
            (Some(Term::Diag(current)), result)
        }
        Some(Decomp::Map { maps }) => {
            let mut inner_to_outer = maps.into_iter().rev();
            let innermost = inner_to_outer.next().expect("Map decomp is non-empty");
            let domain = innermost.domain;
            let mut composed = innermost.map;
            for m in inner_to_outer {
                composed = PartialMap::compose(&m.map, &composed);
            }
            (Some(Term::Map(EvalMap { map: composed, domain })), result)
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
        DExpr::Dot { .. } => {
            let (decomp, result) = decompose(context, scope, d_expr);
            execute(decomp, result)
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
        DComponent::Name(name) => {
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
        DComponent::AnonMap { def, target } => {
            let (eval_map_opt, result) = super::partial_map::interpret_anon_map_component(context, scope, target, def);
            (eval_map_opt.map(|em| Component::Value(Term::Map(em))), result)
        }
        DComponent::In => (Some(Component::Bd(DiagramSign::Input)), InterpResult::ok(context.clone())),
        DComponent::Out => (Some(Component::Bd(DiagramSign::Output)), InterpResult::ok(context.clone())),
        DComponent::Paren(inner_diag) => {
            let (term_opt, result) = interpret_diagram_as_term(context, scope, inner_diag);
            (term_opt.map(Component::Value), result)
        }
        DComponent::Hole => (Some(Component::Hole), InterpResult::ok(context.clone())),
        DComponent::Run { strategy, diagram } => match strategy.inner {
            ast::Strategy::Auto => interpret_run_auto(context, scope, diagram, span),
        },
    }
}

// ---- Strategy application ----

const AUTO_STEP_LIMIT: usize = 1024;

fn interpret_run_auto(
    context: &Context,
    scope: &Complex,
    diagram_ast: &Spanned<ast::Diagram>,
    span: Span,
) -> (Option<Component>, InterpResult) {
    let (diag_opt, result) = interpret_diagram(context, scope, diagram_ast);
    let Some(initial) = diag_opt else { return (None, result); };
    if result.has_errors() { return (None, result); }

    let n = initial.top_dim();
    let rule_patterns = match build_rule_patterns(scope, n, false) {
        Ok(rp) => rp,
        Err(e) => return fail(context, span, format!("run auto: {}", e)),
    };

    let mut current = initial.clone();
    let mut steps: Vec<Diagram> = Vec::new();

    for _ in 0..AUTO_STEP_LIMIT {
        match greedy_parallel_auto_step(scope, &rule_patterns, &current) {
            Ok(Some(pr)) => {
                match Diagram::boundary(DiagramSign::Output, n, &pr.step) {
                    Ok(d) => {
                        steps.push(pr.step);
                        current = d;
                    }
                    Err(e) => return fail(context, span, format!("run auto: {}", e)),
                }
            }
            Ok(None) => break,
            Err(e) => return fail(context, span, format!("run auto: {}", e)),
        }
    }

    if steps.len() >= AUTO_STEP_LIMIT {
        if let Ok(Some(_)) = greedy_parallel_auto_step(scope, &rule_patterns, &current) {
            return fail(context, span,
                format!("run auto: did not terminate within {} steps", AUTO_STEP_LIMIT));
        }
    }

    if steps.is_empty() {
        return (Some(Component::Value(Term::Diag(initial))), result);
    }

    let mut proof = steps[0].clone();
    for step in &steps[1..] {
        match Diagram::paste(n, &proof, step) {
            Ok(d) => proof = d,
            Err(e) => return fail(context, span, format!("run auto: {}", e)),
        }
    }

    (Some(Component::Value(Term::Diag(proof))), result)
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
    if !combined.holes.is_empty() {
        let scope_arc = Arc::new(scope.clone());
        if let Some(Term::Diag(ref d)) = right_opt
            && is_pure_hole_diagram(&assert_stmt.lhs.inner) {
            for hole in &combined.holes[..lhs_hole_count] {
                combined.constraints.push(Constraint::Value {
                    hole: hole.id,
                    diagram: d.clone(),
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Assertion,
                });
            }
        }
        if let Some(Term::Diag(ref d)) = left_opt
            && is_pure_hole_diagram(&assert_stmt.rhs.inner) {
            for hole in &combined.holes[lhs_hole_count..] {
                combined.constraints.push(Constraint::Value {
                    hole: hole.id,
                    diagram: d.clone(),
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Assertion,
                });
            }
        }
    }

    match (left_opt, right_opt) {
        (Some(Term::Diag(d1)), Some(Term::Diag(d2))) => {
            (Some(TermPair::Diagrams { fst: d1, snd: d2 }), combined)
        }
        (Some(Term::Map(mc1)), Some(Term::Map(mc2))) => {
            // Map equality is only meaningful when both sides range over the same
            // declared domain. Otherwise we can silently ignore generators that appear
            // on only one side and accept an invalid assertion.
            if !Arc::ptr_eq(&mc1.domain, &mc2.domain) {
                combined.add_error(make_error(
                    assert_stmt.lhs.span,
                    "The two sides of the equation are incomparable",
                ));
                (None, combined)
            } else {
                (
                    Some(TermPair::Maps { fst: mc1.map, snd: mc2.map, domain: mc1.domain }),
                    combined,
                )
            }
        }
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
    if !combined.holes.is_empty() {
        let scope_arc = Arc::new(scope.clone());
        if let Some(ref d_right) = d_right_opt {
            if let Ok(in_bd) = Diagram::boundary_normal(DiagramSign::Input, k, d_right) {
                for hole in &combined.holes[rhs_hole_count..] {
                    combined.constraints.push(Constraint::BoundaryEq {
                        hole: hole.id,
                        slot: BdSlot { sign: DiagramSign::Output, dim: k },
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
            if let Ok(out_bd) = Diagram::boundary_normal(DiagramSign::Output, k, d_left) {
                for hole in &combined.holes[..rhs_hole_count] {
                    combined.constraints.push(Constraint::BoundaryEq {
                        hole: hole.id,
                        slot: BdSlot { sign: DiagramSign::Input, dim: k },
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
/// Input-boundary constraints are *deferred* until the right neighbour is
/// known so that both input and output constraints use a consistent paste
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
                // Input-boundary emission is deferred until the right neighbour arrives.
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

                    // Infer hole dimension from the left neighbour (left-associative).
                    let n_hole = match hole_block_left.as_ref() {
                        Some(left_diag) => left_diag.top_dim(),
                        None => d_right.top_dim(),
                    };

                    // Input boundary: paste with left at min(left, hole) - 1.
                    if let (Some(left_diag), Some(&first_id)) =
                        (&hole_block_left, block_hole_ids.first()) {
                        if let Some(k_left) = left_diag.top_dim().min(n_hole).checked_sub(1) {
                            if let Ok(out_bd) =
                                Diagram::boundary_normal(DiagramSign::Output, k_left, left_diag) {
                                result.constraints.push(Constraint::BoundaryEq {
                                    hole: first_id,
                                    slot: BdSlot { sign: DiagramSign::Input, dim: k_left },
                                    diagram: out_bd,
                                    scope: scope_arc.clone(),
                                    origin: ConstraintOrigin::Paste { paste_dim: k_left },
                                });
                            }
                        }
                    }

                    // Output boundary: paste with right at min(hole, right) - 1.
                    if let Some(&last_id) = block_hole_ids.last() {
                        if let Some(k_right) = n_hole.min(d_right.top_dim()).checked_sub(1) {
                            if let Ok(in_bd) =
                                Diagram::boundary_normal(DiagramSign::Input, k_right, &d_right) {
                                result.constraints.push(Constraint::BoundaryEq {
                                    hole: last_id,
                                    slot: BdSlot { sign: DiagramSign::Output, dim: k_right },
                                    diagram: in_bd,
                                    scope: scope_arc.clone(),
                                    origin: ConstraintOrigin::Paste { paste_dim: k_right },
                                });
                            }
                        }
                    }

                    // Dimension constraint: infer from left neighbour (left-associative),
                    // falling back to right if there is no left.
                    let n_infer = match hole_block_left.as_ref() {
                        Some(left_diag) => left_diag.top_dim(),
                        None => d_right.top_dim(),
                    };
                    if let Some(paste_k) = n_infer.checked_sub(1) {
                        for &id in &block_hole_ids {
                            result.constraints.push(Constraint::DimEq {
                                hole: id,
                                dim: n_infer,
                                origin: ConstraintOrigin::Paste { paste_dim: paste_k },
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
                    Some(prev) => match prev.top_dim().min(d_right.top_dim()).checked_sub(1) {
                        None => Err(crate::aux::Error::new(
                            "principal paste dimension is below 0")),
                        Some(k) => Diagram::paste(k, &prev, &d_right),
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
    if let Some(start) = last_hole_block_start
        && let Some(ref left_diag) = hole_block_left {
        let n = left_diag.top_dim();
        let scope_arc = Arc::new(scope.clone());
        let trailing_ids: Vec<HoleId> = result.holes[start..].iter().map(|h| h.id).collect();
        if let Some(k) = n.checked_sub(1) {
            if let (Ok(out_bd), Some(&first_id)) =
                (Diagram::boundary_normal(DiagramSign::Output, k, left_diag), trailing_ids.first())
            {
                result.constraints.push(Constraint::BoundaryEq {
                    hole: first_id,
                    slot: BdSlot { sign: DiagramSign::Input, dim: k },
                    diagram: out_bd,
                    scope: scope_arc.clone(),
                    origin: ConstraintOrigin::Paste { paste_dim: k },
                });
            }
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

/// Interpret an input/output boundary pair and wrap the result as `CellData::Boundary`.
///
/// Both sides are always evaluated so that holes in either boundary are detected in a
/// single pass.  Holes in the input boundary are enriched with the output diagram as
/// their `boundary_out` (and vice versa), so the reported context reads naturally as
/// `? -> target` or `source -> ?`.
pub fn interpret_boundaries(
    context: &Context,
    scope: &Complex,
    boundaries: &Spanned<ast::Boundary>,
) -> (Option<CellData>, InterpResult) {
    let (source_opt, source_result) = interpret_diagram(context, scope, &boundaries.inner.input);
    let pre_target_holes = source_result.holes.len();
    // Always evaluate the target even if the source has a hole or error.
    let (target_opt, target_result) =
        interpret_diagram(&source_result.context, scope, &boundaries.inner.output);
    let mut combined = source_result.merge(target_result);

    // Constraint system: a hole in source position must be parallel to the target,
    // and vice versa.  Decomposed eagerly into DimEq + BoundaryEq at principal slots.
    if !combined.holes.is_empty() {
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

#[cfg(test)]
mod dotted_expr_tests {
    //! Behaviour of the deferred [`decompose`]/[`execute`] evaluation of dotted
    //! diagram expressions: boundary suffixes collapse to one direct call, and
    //! maps are applied *after* the boundary (relying on `φ(∂x) = ∂(φx)`).

    use crate::aux::Tag;
    use crate::aux::loader::Loader;
    use crate::core::complex::Complex;
    use crate::core::diagram::{Diagram, Sign};
    use crate::interactive::engine::eval_diagram_expr;
    use crate::interpreter::{GlobalStore, InterpretedFile};
    use std::path::PathBuf;
    use std::sync::Arc;

    /// Load a fixture and return its store, the named type's complex, and path.
    fn load(file: &str, type_name: &str) -> (Arc<GlobalStore>, Arc<Complex>, String) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(file)
            .to_string_lossy()
            .into_owned();
        let f = InterpretedFile::load(&Loader::default(vec![]), &path)
            .ok()
            .expect("fixture should interpret");
        let store = Arc::clone(&f.state);
        let module = store.find_module(&f.path).expect("module");
        let gid = match module.find_generator(type_name) {
            Some((Tag::Global(gid), _)) => *gid,
            _ => panic!("type `{type_name}` not found"),
        };
        let tc = store.find_type(gid).expect("type entry").complex.clone();
        (store, tc, f.path)
    }

    fn eval(store: &Arc<GlobalStore>, tc: &Complex, path: &str, expr: &str) -> Diagram {
        eval_diagram_expr(store, tc, path, expr).unwrap_or_else(|e| panic!("`{expr}`: {e}"))
    }

    #[test]
    fn boundary_suffix_collapses_to_one_direct_call() {
        let (store, tc, path) = load("tests/fixtures/Assoc.ali", "Assoc");
        let lhs2 = eval(&store, &tc, &path, "lhs2");
        assert_eq!(lhs2.top_dim(), 2);

        // A single boundary equals the direct codim-1 call.
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "lhs2.in"),
            &Diagram::boundary(Sign::Input, 1, &lhs2).unwrap(),
        ));
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "lhs2.out"),
            &Diagram::boundary(Sign::Output, 1, &lhs2).unwrap(),
        ));

        // Two ops collapse: only the last polarity and the count matter, so a
        // length-2 suffix lands at dimension n-2 with the last op's polarity.
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "lhs2.in.out"),
            &Diagram::boundary(Sign::Output, 0, &lhs2).unwrap(),
        ));
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "lhs2.out.in"),
            &Diagram::boundary(Sign::Input, 0, &lhs2).unwrap(),
        ));

        // The collapse relations ∂ⁱⁿ∂ⁱⁿ = ∂ᵒᵘᵗ∂ⁱⁿ and ∂ⁱⁿ∂ᵒᵘᵗ = ∂ᵒᵘᵗ∂ᵒᵘᵗ.
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "lhs2.in.in"),
            &eval(&store, &tc, &path, "lhs2.out.in"),
        ));
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "lhs2.in.out"),
            &eval(&store, &tc, &path, "lhs2.out.out"),
        ));
    }

    #[test]
    fn boundary_underflow_is_rejected() {
        let (store, tc, path) = load("tests/fixtures/Assoc.ali", "Assoc");
        // Three boundary ops on a 2-diagram drop below dimension 0.
        assert!(eval_diagram_expr(&store, &tc, &path, "lhs2.in.in.in").is_err());
        // A 0-diagram has no principal boundary.
        let err = eval_diagram_expr(&store, &tc, &path, "pt.in").unwrap_err();
        assert!(err.contains("principal boundary"), "unexpected error: {err}");
    }

    #[test]
    fn maps_are_applied_after_the_boundary() {
        // F : Arrow -> Graph sends s ↦ A.s, t ↦ B.t, arr ↦ (A.arr mid B.arr).
        let (store, tc, path) = load("legacy/examples/Total.ali", "Graph");

        // `F.arr.in` takes ∂ⁱⁿ(arr) = s first, then applies F, giving A.s — and
        // dually `F.arr.out` = B.t.  This is the reordered evaluation.
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "F.arr.in"),
            &eval(&store, &tc, &path, "A.s"),
        ));
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "F.arr.out"),
            &eval(&store, &tc, &path, "B.t"),
        ));
        // Plain application of the map to a 0-cell (no boundary).
        assert!(Diagram::isomorphic(
            &eval(&store, &tc, &path, "F.s"),
            &eval(&store, &tc, &path, "A.s"),
        ));
    }
}
