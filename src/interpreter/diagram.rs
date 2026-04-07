use super::resolve::resolve_map_domain_complex;
use super::types::{
    Component, Context, EvalMap, HoleBd, HoleBoundaryInfo, HoleInfo, InterpResult, Step,
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

    // Cross-enrich diagram holes from the concrete opposite side.
    // After merge, combined.holes = [LHS holes (..lhs_hole_count)] ++ [RHS holes].
    let enrich = |holes: &mut [crate::interpreter::types::HoleInfo], d: &Diagram, scope: &Complex| {
        let k = d.top_dim().saturating_sub(1);
        if let (Ok(in_bd), Ok(out_bd)) = (
            Diagram::boundary(DiagramSign::Source, k, d),
            Diagram::boundary(DiagramSign::Target, k, d),
        ) {
            for hole in holes {
                match &mut hole.boundary {
                    None => hole.boundary = Some(HoleBoundaryInfo {
                        boundary_in: HoleBd::Full(in_bd.clone(), Arc::new(scope.clone())),
                        boundary_out: HoleBd::Full(out_bd.clone(), Arc::new(scope.clone())),
                    }),
                    Some(bd) => {
                        if matches!(bd.boundary_in, HoleBd::Unknown) {
                            bd.boundary_in = HoleBd::Full(in_bd.clone(), Arc::new(scope.clone()));
                        }
                        if matches!(bd.boundary_out, HoleBd::Unknown) {
                            bd.boundary_out = HoleBd::Full(out_bd.clone(), Arc::new(scope.clone()));
                        }
                    }
                }
            }
        }
    };
    if let Some(Term::Diag(ref d)) = right_opt {
        enrich(&mut combined.holes[..lhs_hole_count], d, scope);
    }
    if let Some(Term::Diag(ref d)) = left_opt {
        enrich(&mut combined.holes[lhs_hole_count..], d, scope);
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

    // Cross-enrich holes: a concrete RHS enriches LHS holes (boundary_out = RHS.source_k);
    // a concrete LHS enriches RHS holes (boundary_in = LHS.target_k).
    // After merge, combined.holes = [RHS holes (0..rhs_hole_count)] ++ [LHS holes].
    if let Some(ref d_right) = d_right_opt {
        if let Ok(in_bd) = Diagram::boundary(DiagramSign::Source, k, d_right) {
            for hole in &mut combined.holes[rhs_hole_count..] {
                match &mut hole.boundary {
                    None => hole.boundary = Some(HoleBoundaryInfo {
                        boundary_in: HoleBd::Unknown,
                        boundary_out: HoleBd::Full(in_bd.clone(), Arc::new(scope.clone())),
                    }),
                    Some(bd) if matches!(bd.boundary_out, HoleBd::Unknown) => {
                        bd.boundary_out = HoleBd::Full(in_bd.clone(), Arc::new(scope.clone()));
                    }
                    _ => {}
                }
            }
        }
    }
    if let Some(ref d_left) = d_left_opt {
        if let Ok(out_bd) = Diagram::boundary(DiagramSign::Target, k, d_left) {
            for hole in &mut combined.holes[..rhs_hole_count] {
                match &mut hole.boundary {
                    None => hole.boundary = Some(HoleBoundaryInfo {
                        boundary_in: HoleBd::Full(out_bd.clone(), Arc::new(scope.clone())),
                        boundary_out: HoleBd::Unknown,
                    }),
                    Some(bd) if matches!(bd.boundary_in, HoleBd::Unknown) => {
                        bd.boundary_in = HoleBd::Full(out_bd.clone(), Arc::new(scope.clone()));
                    }
                    _ => {}
                }
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
    let mut last_hole_block_start: Option<usize> = None;
    let mut has_holes = false;
    let mut result = InterpResult::ok(context.clone());

    for expr in exprs {
        let prev_hole_count = result.holes.len();
        let (term_opt, expr_result) = interpret_dexpr(&result.context, scope, expr);
        result = result.merge(expr_result);
        let new_holes = result.holes.len() > prev_hole_count;

        match term_opt {
            None if new_holes => {
                // Enrich each new hole's left boundary from the most recent concrete diagram.
                let left_ref = right_acc.as_ref().or(left_acc.as_ref());
                if let Some(left_diag) = left_ref {
                    let k = left_diag.top_dim().saturating_sub(1);
                    if let Ok(out_bd) = Diagram::boundary(DiagramSign::Target, k, left_diag) {
                        for hole in &mut result.holes[prev_hole_count..] {
                            match &mut hole.boundary {
                                None => hole.boundary = Some(HoleBoundaryInfo {
                                    boundary_in: HoleBd::Full(out_bd.clone(), Arc::new(scope.clone())),
                                    boundary_out: HoleBd::Unknown,
                                }),
                                Some(bd) if matches!(bd.boundary_in, HoleBd::Unknown) => {
                                    bd.boundary_in = HoleBd::Full(out_bd.clone(), Arc::new(scope.clone()));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // Start a new hole block only when not already in one;
                // adjacent holes share the same block so they all get right
                // context from the next concrete diagram.
                if last_hole_block_start.is_none() {
                    last_hole_block_start = Some(prev_hole_count);
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
                // Enrich the right boundary of every hole in the current block.
                if let Some(start) = last_hole_block_start {
                    let k = d_right.top_dim().saturating_sub(1);
                    if let Ok(in_bd) = Diagram::boundary(DiagramSign::Source, k, &d_right) {
                        for hole in &mut result.holes[start..prev_hole_count] {
                            match &mut hole.boundary {
                                None => hole.boundary = Some(HoleBoundaryInfo {
                                    boundary_in: HoleBd::Unknown,
                                    boundary_out: HoleBd::Full(in_bd.clone(), Arc::new(scope.clone())),
                                }),
                                Some(bd) if matches!(bd.boundary_out, HoleBd::Unknown) => {
                                    bd.boundary_out = HoleBd::Full(in_bd.clone(), Arc::new(scope.clone()));
                                }
                                _ => {}
                            }
                        }
                    }
                    last_hole_block_start = None;
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

    // Enrich source-side holes with the target diagram and vice versa.
    // After merge, combined.holes = [source holes (0..pre_target_holes)] ++ [target holes].
    if let Some(ref tgt) = target_opt {
        let bd = HoleBd::Full(tgt.clone(), Arc::new(scope.clone()));
        for hole in &mut combined.holes[..pre_target_holes] {
            match &mut hole.boundary {
                None => hole.boundary = Some(HoleBoundaryInfo {
                    boundary_in: HoleBd::Unknown,
                    boundary_out: bd.clone(),
                }),
                Some(existing) if matches!(existing.boundary_out, HoleBd::Unknown) => {
                    existing.boundary_out = bd.clone();
                }
                _ => {}
            }
        }
    }
    if let Some(ref src) = source_opt {
        let bd = HoleBd::Full(src.clone(), Arc::new(scope.clone()));
        for hole in &mut combined.holes[pre_target_holes..] {
            match &mut hole.boundary {
                None => hole.boundary = Some(HoleBoundaryInfo {
                    boundary_in: bd.clone(),
                    boundary_out: HoleBd::Unknown,
                }),
                Some(existing) if matches!(existing.boundary_in, HoleBd::Unknown) => {
                    existing.boundary_in = bd.clone();
                }
                _ => {}
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
