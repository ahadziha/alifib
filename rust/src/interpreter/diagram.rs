use super::types::*;
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use crate::language::ast::{self, DComponent, DExpr, PMapBasic, Span, Spanned};
use std::sync::Arc;

// ---- Diagram interpretation ----

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
            let k = match dim.inner.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error(
                        dim.span,
                        format!("Invalid paste dimension: {}", dim.inner),
                    ));
                    return (None, r);
                }
            };
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
        let mut r = InterpResult::ok(context.clone());
        r.add_error(make_error(span, "Empty diagram expression"));
        return (None, r);
    }

    // Interpret first expression
    let (first_opt, first_result) = interpret_d_expr(context, scope, &exprs[0]);
    match first_opt {
        None => return (None, first_result),
        Some(Term::MTerm(_)) => {
            let mut r = first_result;
            r.add_error(make_error(exprs[0].span, "Not a diagram"));
            return (None, r);
        }
        Some(Term::DTerm(d_first)) => {
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
                    Some(Term::MTerm(_)) => {
                        result.add_error(make_error(expr.span, "Not a diagram"));
                        return (None, result);
                    }
                    Some(Term::DTerm(d_right)) => {
                        let k = (acc.dim().max(0) as usize)
                            .min(d_right.dim().max(0) as usize)
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
                Some(Component::Hole) => {
                    let mut r = result;
                    r.add_hole(HoleInfo {
                        span: d_expr.span,
                        boundary: None,
                        source_tag: None,
                    });
                    (None, r)
                }
                Some(Component::Bd(_)) => {
                    let mut r = result;
                    r.add_error(make_error(d_expr.span, "Not a diagram or map"));
                    (None, r)
                }
                Some(Component::Term(t)) => (Some(t), result),
            }
        }
        DExpr::Dot { base, field } => {
            let (left_opt, left_result) = interpret_d_expr(context, scope, base);
            match left_opt {
                None => (None, left_result),
                Some(Term::DTerm(diagram)) => {
                    let (comp_opt, comp_result) =
                        interpret_d_comp(&left_result.context, scope, &field.inner, field.span);
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Bd(sign)) => {
                            let k = (diagram.dim().max(0) as usize).saturating_sub(1);
                            match Diagram::boundary(sign, k, &diagram) {
                                Ok(bd) => (Some(Term::DTerm(bd)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error(field.span, e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        Some(Component::Hole) => {
                            let mut r = combined;
                            r.add_hole(HoleInfo {
                                span: field.span,
                                boundary: None,
                                source_tag: None,
                            });
                            (None, r)
                        }
                        Some(Component::Term(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(
                                field.span,
                                "Not a well-formed diagram expression",
                            ));
                            (None, r)
                        }
                    }
                }
                Some(Term::MTerm(mc)) => {
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context,
                        &*mc.domain,
                        &field.inner,
                        field.span,
                    );
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Hole) => {
                            let mut r = combined;
                            r.add_hole(HoleInfo {
                                span: field.span,
                                boundary: None,
                                source_tag: None,
                            });
                            (None, r)
                        }
                        Some(Component::Bd(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(field.span, "Not a diagram or map"));
                            (None, r)
                        }
                        Some(Component::Term(Term::DTerm(d))) => match PMap::apply(&mc.map, &d) {
                            Ok(d_img) => (Some(Term::DTerm(d_img)), combined),
                            Err(e) => {
                                let mut r = combined;
                                r.add_error(make_error(field.span, e.to_string()));
                                (None, r)
                            }
                        },
                        Some(Component::Term(Term::MTerm(right_mc))) => {
                            let composed = PMap::compose(&mc.map, &right_mc.map);
                            (
                                Some(Term::MTerm(MapComponent {
                                    map: composed,
                                    domain: right_mc.domain,
                                })),
                                combined,
                            )
                        }
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
                        Some(Component::Term(Term::DTerm(diagram.clone()))),
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
                        Some(Component::Term(Term::MTerm(MapComponent {
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
                    Some(mc) => (Some(Component::Term(Term::MTerm(mc))), result),
                }
            }
            PMapBasic::Paren(inner_pmap) => {
                let (mc_opt, result) =
                    super::pmap::interpret_pmap(context, scope, scope, inner_pmap);
                match mc_opt {
                    None => (None, result),
                    Some(mc) => (Some(Component::Term(Term::MTerm(mc))), result),
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
                Some(d) => (Some(Component::Term(Term::DTerm(d))), result),
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
                    (Term::DTerm(d1), Term::DTerm(d2)) => {
                        (Some(TermPair::DTermPair { fst: d1, snd: d2 }), combined)
                    }
                    (Term::MTerm(mc1), Term::MTerm(mc2)) => (
                        Some(TermPair::MTermPair {
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
            let k = match dim.inner.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error(
                        dim.span,
                        format!("Invalid paste dimension: {}", dim.inner),
                    ));
                    return (None, r);
                }
            };
            // Right side first
            let (right_opt, right_result) =
                interpret_principal_as_term(context, scope, rhs, diagram.span);
            match right_opt {
                None => (None, right_result),
                Some(Term::MTerm(_)) => {
                    let mut r = right_result;
                    r.add_error(make_error(diagram.span, "Not a diagram"));
                    (None, r)
                }
                Some(Term::DTerm(d_right)) => {
                    let (left_opt, left_result) =
                        interpret_diagram_as_term(&right_result.context, scope, lhs);
                    let combined = InterpResult::combine(right_result, left_result);
                    match left_opt {
                        None => (None, combined),
                        Some(Term::MTerm(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(diagram.span, "Not a diagram"));
                            (None, r)
                        }
                        Some(Term::DTerm(d_left)) => match Diagram::paste(k, &d_left, &d_right) {
                            Ok(d) => (Some(Term::DTerm(d)), combined),
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
        let mut r = InterpResult::ok(context.clone());
        r.add_error(make_error(span, "Empty diagram expression"));
        return (None, r);
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
                if let Some(Term::DTerm(d_right)) = next_opt {
                    let k = (d_right.dim().max(0) as usize).saturating_sub(1);
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
                Term::DTerm(d) => d,
                Term::MTerm(_) => {
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
                            let k = (acc.dim().max(0) as usize).saturating_sub(1);
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
                    Some(Term::MTerm(_)) => {
                        result.add_error(make_error(expr.span, "Not a diagram"));
                        return (None, result);
                    }
                    Some(Term::DTerm(d_right)) => {
                        let k = (acc.dim().max(0) as usize)
                            .min(d_right.dim().max(0) as usize)
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

            (Some(Term::DTerm(acc)), result)
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
    let d = diagram.dim().max(0) as usize;
    match diagram.labels.get(d) {
        Some(top_labels) if !top_labels.is_empty() => top_labels
            .iter()
            .map(|tag| {
                scope
                    .find_generator_by_tag(tag)
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag))
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => "?".to_string(),
    }
}

/// Render a source boundary diagram through a partial map. Mapped tags are rendered
/// via their image's top label; unmapped tags are rendered as `?`.
pub fn render_boundary_partial(boundary: &Diagram, map: &PMap, scope: &Complex) -> String {
    let d = boundary.dim().max(0) as usize;
    match boundary.labels.get(d) {
        Some(top_labels) if !top_labels.is_empty() => top_labels
            .iter()
            .map(|tag| match map.image(tag) {
                Ok(img) => render_diagram(img, scope),
                Err(_) => "?".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => "?".to_string(),
    }
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
