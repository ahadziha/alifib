use super::types::*;
use crate::aux::Tag;
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use crate::language::ast::{self, DComponent, DExpr, PMapBasic, Span, Spanned};
use std::sync::Arc;

// ---- Helpers ----

fn parse_paste_dim(context: &Context, dim: &Spanned<String>) -> Step<usize> {
    dim.inner
        .parse::<usize>()
        .map(|k| (Some(k), InterpResult::ok(context.clone())))
        .unwrap_or_else(|_| fail(context, dim.span, format!("Invalid paste dimension: {}", dim.inner)))
}

fn top_labels_rendered(diagram: &Diagram, f: impl Fn(&Tag) -> String) -> String {
    let d = diagram.top_dim();
    match diagram.labels.get(d) {
        Some(labels) if !labels.is_empty() => {
            labels.iter().map(f).collect::<Vec<_>>().join(" ")
        }
        _ => "?".to_string(),
    }
}

// ---- Diagram interpretation ----

// Holes use a two-phase computation: they are created here with no boundary info,
// then enriched with source/target boundary strings later by `fill_hole_boundary`
// in pmap.rs once a map clause gives enough context to render them.
fn add_hole_result(context: &Context, span: Span) -> (Option<Term>, InterpResult) {
    let mut result = InterpResult::ok(context.clone());
    result.add_hole(HoleInfo {
        span,
        boundary: None,
        source_tag: None,
    });
    (None, result)
}

fn boundary_term_from_diagram(
    diagram: &Diagram,
    sign: DiagramSign,
    span: Span,
    result: InterpResult,
) -> (Option<Term>, InterpResult) {
    let boundary_dim = diagram.top_dim().saturating_sub(1);
    match Diagram::boundary(sign, boundary_dim, diagram) {
        Ok(boundary) => (Some(Term::Diag(boundary)), result),
        Err(error) => {
            let mut result = result;
            result.add_error(make_error(span, error.to_string()));
            (None, result)
        }
    }
}

fn apply_map_component(
    eval_map: &EvalMap,
    component: Component,
    span: Span,
    result: InterpResult,
) -> (Option<Term>, InterpResult) {
    match component {
        Component::Hole => {
            let mut result = result;
            result.add_hole(HoleInfo {
                span,
                boundary: None,
                source_tag: None,
            });
            (None, result)
        }
        Component::Bd(_) => {
            let mut result = result;
            result.add_error(make_error(span, "Not a diagram or map"));
            (None, result)
        }
        Component::Value(Term::Diag(diagram)) => match PMap::apply(&eval_map.map, &diagram) {
            Ok(image_diagram) => (Some(Term::Diag(image_diagram)), result),
            Err(error) => {
                let mut result = result;
                result.add_error(make_error(span, error.to_string()));
                (None, result)
            }
        },
        Component::Value(Term::Map(inner_map)) => {
            let composed = PMap::compose(&eval_map.map, &inner_map.map);
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

pub fn interpret_diagram(
    context: &Context,
    scope: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Diagram>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::Principal(exprs) => {
            interpret_principal(context, scope, exprs, diagram.span)
        }
        ast::Diagram::Paste { lhs, dim, rhs } => {
            let (k_opt, k_result) = parse_paste_dim(context, dim);
            let Some(k) = k_opt else { return (None, k_result); };
            interpret_diagram_paste(context, scope, diagram.span, lhs, k, rhs)
        }
    }
}

fn interpret_principal(
    context: &Context,
    scope: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Diagram>, InterpResult) {
    if exprs.is_empty() {
        return fail(context, span, "Empty diagram expression");
    }

    // Interpret first expression
    let (first_opt, first_result) = interpret_d_expr(context, scope, &exprs[0]);
    match first_opt {
        None => return (None, first_result),
        Some(Term::Map(_)) => {
            let mut result = first_result;
            result.add_error(make_error(exprs[0].span, "Not a diagram"));
            return (None, result);
        }
        Some(Term::Diag(d_first)) => {
            if exprs.len() == 1 {
                return (Some(d_first), first_result);
            }

            // Fold left-to-right
            let mut acc = d_first;
            let mut result = first_result;

            for expr in &exprs[1..] {
                let (term_opt, expr_result) = interpret_d_expr(&result.context, scope, expr);
                result = InterpResult::combine(result, expr_result);
                match term_opt {
                    None => return (None, result),
                    Some(Term::Map(_)) => {
                        result.add_error(make_error(expr.span, "Not a diagram"));
                        return (None, result);
                    }
                    Some(Term::Diag(d_right)) => {
                        let k = acc.top_dim()
                            .min(d_right.top_dim())
                            .saturating_sub(1);
                        match Diagram::paste(k, &acc, &d_right) {
                            Ok(d) => acc = d,
                            Err(e) => {
                                result.add_error(make_error(
                                    span,
                                    format!("Failed to paste diagrams: {}", e),
                                ));
                                return (None, result);
                            }
                        }
                    }
                }
            }

            (Some(acc), result)
        }
    }
}

fn interpret_diagram_paste(
    context: &Context,
    scope: &Complex,
    span: Span,
    left: &Spanned<ast::Diagram>,
    k: usize,
    right: &[Spanned<DExpr>],
) -> (Option<Diagram>, InterpResult) {
    let (right_opt, right_result) = interpret_principal(context, scope, right, span);
    match right_opt {
        None => (None, right_result),
        Some(d_right) => {
            let (left_opt, left_result) = interpret_diagram(&right_result.context, scope, left);
            let combined = InterpResult::combine(right_result, left_result);
            match left_opt {
                None => (None, combined),
                Some(d_left) => match Diagram::paste(k, &d_left, &d_right) {
                    Ok(d) => (Some(d), combined),
                    Err(e) => {
                        let mut r = combined;
                        r.add_error(make_error(span, format!("Failed to paste diagrams: {}", e)));
                        (None, r)
                    }
                },
            }
        }
    }
}

pub fn interpret_d_expr(
    context: &Context,
    scope: &Complex,
    d_expr: &Spanned<DExpr>,
) -> (Option<Term>, InterpResult) {
    match &d_expr.inner {
        DExpr::Component(comp) => {
            let (comp_opt, result) = interpret_d_comp(context, scope, comp, d_expr.span);
            match comp_opt {
                None => (None, result),
                Some(Component::Hole) => add_hole_result(&result.context, d_expr.span),
                Some(Component::Bd(_)) => {
                    let mut r = result;
                    r.add_error(make_error(d_expr.span, "Not a diagram or map"));
                    (None, r)
                }
                Some(Component::Value(t)) => (Some(t), result),
            }
        }
        DExpr::Dot { base, field } => {
            let (left_opt, left_result) = interpret_d_expr(context, scope, base);
            match left_opt {
                None => (None, left_result),
                Some(Term::Diag(diagram)) => {
                    let (comp_opt, comp_result) =
                        interpret_d_comp(&left_result.context, scope, &field.inner, field.span);
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Bd(sign)) => {
                            boundary_term_from_diagram(&diagram, sign, field.span, combined)
                        }
                        Some(Component::Hole) => add_hole_result(&combined.context, field.span),
                        Some(Component::Value(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(
                                field.span,
                                "Not a well-formed diagram expression",
                            ));
                            (None, r)
                        }
                    }
                }
                Some(Term::Map(eval_map)) => {
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context,
                        &*eval_map.domain,
                        &field.inner,
                        field.span,
                    );
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(component) => apply_map_component(&eval_map, component, field.span, combined),
                    }
                }
            }
        }
    }
}

pub fn interpret_d_comp(
    context: &Context,
    scope: &Complex,
    d_comp: &DComponent,
    span: Span,
) -> (Option<Component>, InterpResult) {
    match d_comp {
        DComponent::PMap(basic) => match basic {
            PMapBasic::Name(name) => {
                let base_result = InterpResult::ok(context.clone());
                if let Some(diagram) = scope.find_diagram(name) {
                    return (
                        Some(Component::Value(Term::Diag(diagram.clone()))),
                        base_result,
                    );
                }
                if let Some(entry) = scope.find_map(name) {
                    let (domain_opt, domain_result) =
                        resolve_map_domain_complex(context, &entry.domain, span);
                    let domain_complex = match domain_opt {
                        None => return (None, InterpResult::combine(base_result, domain_result)),
                        Some(domain) => domain,
                    };
                    return (
                        Some(Component::Value(Term::Map(EvalMap {
                            map: entry.map.clone(),
                            domain: domain_complex,
                        }))),
                        base_result,
                    );
                }
                let mut r = base_result;
                r.add_error(make_error(span, format!("Name `{}` not found", name)));
                (None, r)
            }
            PMapBasic::AnonMap { def, target } => {
                let (mc_opt, result) =
                    super::pmap::interpret_anon_map_component(context, scope, target, def);
                match mc_opt {
                    None => (None, result),
                    Some(mc) => (Some(Component::Value(Term::Map(mc))), result),
                }
            }
            PMapBasic::Paren(inner_pmap) => {
                let (mc_opt, result) =
                    super::pmap::interpret_pmap(context, scope, scope, inner_pmap);
                match mc_opt {
                    None => (None, result),
                    Some(mc) => (Some(Component::Value(Term::Map(mc))), result),
                }
            }
        },
        DComponent::In => (
            Some(Component::Bd(DiagramSign::Source)),
            InterpResult::ok(context.clone()),
        ),
        DComponent::Out => (
            Some(Component::Bd(DiagramSign::Target)),
            InterpResult::ok(context.clone()),
        ),
        DComponent::Paren(inner_diag) => {
            let (d_opt, result) = interpret_diagram(context, scope, inner_diag);
            match d_opt {
                None => (None, result),
                Some(d) => (Some(Component::Value(Term::Diag(d))), result),
            }
        }
        DComponent::Hole => (Some(Component::Hole), InterpResult::ok(context.clone())),
    }
}

// ---- Assert ----

pub fn interpret_assert(
    context: &Context,
    scope: &Complex,
    assert_stmt: &crate::language::ast::AssertStmt,
) -> (Option<TermPair>, InterpResult) {
    let (left_opt, left_result) = interpret_diagram_as_term(context, scope, &assert_stmt.lhs);
    match left_opt {
        None => (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) =
                interpret_diagram_as_term(&left_result.context, scope, &assert_stmt.rhs);
            let combined = InterpResult::combine(left_result, right_result);
            match right_opt {
                None => (None, combined),
                Some(right_term) => match (left_term, right_term) {
                    (Term::Diag(d1), Term::Diag(d2)) => {
                        (Some(TermPair::Diagrams { fst: d1, snd: d2 }), combined)
                    }
                    (Term::Map(mc1), Term::Map(mc2)) => (
                        Some(TermPair::Maps {
                            fst: mc1.map,
                            snd: mc2.map,
                            domain: mc1.domain,
                        }),
                        combined,
                    ),
                    _ => {
                        let span = assert_stmt.lhs.span;
                        let mut r = combined;
                        r.add_error(make_error(
                            span,
                            "The two sides of the equation are incomparable",
                        ));
                        (None, r)
                    }
                },
            }
        }
    }
}

pub fn interpret_diagram_as_term(
    context: &Context,
    scope: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Term>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::Principal(exprs) => {
            interpret_principal_as_term(context, scope, exprs, diagram.span)
        }
        ast::Diagram::Paste { lhs, dim, rhs } => {
            let (k_opt, k_result) = parse_paste_dim(context, dim);
            let Some(k) = k_opt else { return (None, k_result); };
            // Right side first
            let (right_opt, right_result) =
                interpret_principal_as_term(context, scope, rhs, diagram.span);
            match right_opt {
                None => (None, right_result),
                Some(Term::Map(_)) => {
                    let mut r = right_result;
                    r.add_error(make_error(diagram.span, "Not a diagram"));
                    (None, r)
                }
                Some(Term::Diag(d_right)) => {
                    let (left_opt, left_result) =
                        interpret_diagram_as_term(&right_result.context, scope, lhs);
                    let combined = InterpResult::combine(right_result, left_result);
                    match left_opt {
                        None => (None, combined),
                        Some(Term::Map(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(diagram.span, "Not a diagram"));
                            (None, r)
                        }
                        Some(Term::Diag(d_left)) => match Diagram::paste(k, &d_left, &d_right) {
                            Ok(d) => (Some(Term::Diag(d)), combined),
                            Err(e) => {
                                let mut r = combined;
                                r.add_error(make_error(
                                    diagram.span,
                                    format!("Failed to paste diagrams: {}", e),
                                ));
                                (None, r)
                            }
                        },
                    }
                }
            }
        }
    }
}

fn interpret_principal_as_term(
    context: &Context,
    scope: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Term>, InterpResult) {
    if exprs.is_empty() {
        return fail(context, span, "Empty diagram expression");
    }

    let (first_opt, first_result) = interpret_d_expr(context, scope, &exprs[0]);
    match first_opt {
        None => {
            let mut result = first_result;
            // If a hole was added and there are more exprs, use right-context
            if !result.holes.is_empty() && exprs.len() > 1 {
                let (next_opt, next_result) =
                    interpret_d_expr(&result.context, scope, &exprs[1]);
                result = InterpResult::combine(result, next_result);
                if let Some(Term::Diag(d_right)) = next_opt {
                    let k = d_right.top_dim().saturating_sub(1);
                    if let Ok(in_bd) = Diagram::boundary(DiagramSign::Source, k, &d_right) {
                        if let Some(last_hole) = result.holes.last_mut() {
                            let bd_out = render_diagram(&in_bd, scope);
                            match &mut last_hole.boundary {
                                Some(existing) => {
                                    existing.boundary_out = bd_out;
                                }
                                None => {
                                    last_hole.boundary = Some(HoleBoundaryInfo {
                                        boundary_in: "?".into(),
                                        boundary_out: bd_out,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            return (None, result);
        }
        Some(term) => {
            if exprs.len() == 1 {
                return (Some(term), first_result);
            }

            // Multiple exprs: must all be diagrams
            let d_first = match term {
                Term::Diag(d) => d,
                Term::Map(_) => {
                    let mut r = first_result;
                    r.add_error(make_error(exprs[0].span, "Not a diagram"));
                    return (None, r);
                }
            };

            let mut acc = d_first;
            let mut result = first_result;

            for expr in &exprs[1..] {
                let prev_hole_count = result.holes.len();
                let (term_opt, expr_result) = interpret_d_expr(&result.context, scope, expr);
                result = InterpResult::combine(result, expr_result);
                match term_opt {
                    None => {
                        // If a hole was just added, enrich with left-context boundary
                        if result.holes.len() > prev_hole_count {
                            let k = acc.top_dim().saturating_sub(1);
                            if let Ok(out_bd) = Diagram::boundary(DiagramSign::Target, k, &acc) {
                                if let Some(last_hole) = result.holes.last_mut() {
                                    last_hole.boundary = Some(HoleBoundaryInfo {
                                        boundary_in: render_diagram(&out_bd, scope),
                                        boundary_out: "?".into(),
                                    });
                                }
                            }
                        }
                        return (None, result);
                    }
                    Some(Term::Map(_)) => {
                        result.add_error(make_error(expr.span, "Not a diagram"));
                        return (None, result);
                    }
                    Some(Term::Diag(d_right)) => {
                        let k = acc.top_dim()
                            .min(d_right.top_dim())
                            .saturating_sub(1);
                        match Diagram::paste(k, &acc, &d_right) {
                            Ok(d) => acc = d,
                            Err(e) => {
                                result.add_error(make_error(
                                    span,
                                    format!("Failed to paste diagrams: {}", e),
                                ));
                                return (None, result);
                            }
                        }
                    }
                }
            }

            (Some(Term::Diag(acc)), result)
        }
    }
}

// ---- Boundaries ----

pub fn interpret_boundaries(
    context: &Context,
    scope: &Complex,
    boundaries: &Spanned<ast::Boundary>,
) -> (Option<CellData>, InterpResult) {
    let (source_opt, source_result) = interpret_diagram(context, scope, &boundaries.inner.source);
    match source_opt {
        None => (None, source_result),
        Some(boundary_in) => {
            let (target_opt, target_result) =
                interpret_diagram(&source_result.context, scope, &boundaries.inner.target);
            let combined = InterpResult::combine(source_result, target_result);
            match target_opt {
                None => (None, combined),
                Some(boundary_out) => (
                    Some(CellData::Boundary {
                        boundary_in: Arc::new(boundary_in),
                        boundary_out: Arc::new(boundary_out),
                    }),
                    combined,
                ),
            }
        }
    }
}

// ---- Render helper ----

pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    top_labels_rendered(diagram, |tag| {
        scope
            .find_generator_by_tag(tag)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", tag))
    })
}

/// Render a source boundary diagram through a partial map. Mapped tags are rendered
/// via their image's top label; unmapped tags are rendered as `?`.
pub fn render_boundary_partial(boundary: &Diagram, map: &PMap, scope: &Complex) -> String {
    top_labels_rendered(boundary, |tag| match map.image(tag) {
        Ok(img) => render_diagram(img, scope),
        Err(_) => "?".to_string(),
    })
}

// ---- Diagram naming ----

pub fn interpret_let_diag(
    context: &Context,
    scope: &Complex,
    ld: &crate::language::ast::LetDiag,
) -> (Option<(String, Diagram)>, InterpResult) {
    let (diag_opt, diag_result) = interpret_diagram(context, scope, &ld.value);
    match diag_opt {
        None => (None, diag_result),
        Some(diagram) => {
            let name = ld.name.inner.clone();
            (Some((name, diagram)), diag_result)
        }
    }
}
