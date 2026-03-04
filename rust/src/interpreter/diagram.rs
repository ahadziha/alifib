use std::sync::Arc;
use crate::core::{
    complex::Complex,
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use crate::language::ast::{self, Span, Spanned, DExpr, DComponent};
use super::types::*;

// ---- Diagram interpretation ----

pub fn interpret_diagram(
    context: &Context,
    location: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Diagram>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::Principal(exprs) => interpret_principal(context, location, exprs, diagram.span),
        ast::Diagram::Paste { lhs, dim, rhs } => {
            let k = match dim.inner.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error(dim.span,
                        format!("Invalid paste dimension: {}", dim.inner)));
                    return (None, r);
                }
            };
            interpret_diagram_paste(context, location, diagram.span, lhs, k, rhs)
        }
    }
}

fn interpret_principal(
    context: &Context,
    location: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Diagram>, InterpResult) {
    if exprs.is_empty() {
        let mut r = InterpResult::ok(context.clone());
        r.add_error(make_error(span, "Empty diagram expression"));
        return (None, r);
    }

    // Interpret first expression
    let (first_opt, first_result) = interpret_d_expr(context, location, &exprs[0]);
    match first_opt {
        None => return (None, first_result),
        Some(Term::MTerm(_)) if exprs.len() == 1 => {
            let mut r = first_result;
            r.add_error(make_error(exprs[0].span, "Not a diagram"));
            return (None, r);
        }
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
                let (term_opt, expr_result) = interpret_d_expr(&result.context, location, expr);
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
                                result.add_error(make_error(span,
                                    format!("Failed to paste diagrams: {}", e)));
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
    location: &Complex,
    span: Span,
    left: &Spanned<ast::Diagram>,
    k: usize,
    right: &[Spanned<DExpr>],
) -> (Option<Diagram>, InterpResult) {
    // Process right side first (as in old code)
    let (right_opt, right_result) = interpret_principal(context, location, right, span);
    match right_opt {
        None => (None, right_result),
        Some(d_right) => {
            let (left_opt, left_result) = interpret_diagram(&right_result.context, location, left);
            let combined = InterpResult::combine(right_result, left_result);
            match left_opt {
                None => (None, combined),
                Some(d_left) => {
                    match Diagram::paste(k, &d_left, &d_right) {
                        Ok(d) => (Some(d), combined),
                        Err(e) => {
                            let mut r = combined;
                            r.add_error(make_error(span,
                                format!("Failed to paste diagrams: {}", e)));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

pub fn interpret_d_expr(
    context: &Context,
    location: &Complex,
    d_expr: &Spanned<DExpr>,
) -> (Option<Term>, InterpResult) {
    match &d_expr.inner {
        DExpr::Component(comp) => {
            let (comp_opt, result) = interpret_d_comp(context, location, comp, d_expr.span);
            match comp_opt {
                None => (None, result),
                Some(Component::Hole) | Some(Component::Bd(_)) => {
                    let mut r = result;
                    r.add_error(make_error(d_expr.span, "Not a diagram or map"));
                    (None, r)
                }
                Some(Component::Term(t)) => (Some(t), result),
            }
        }
        DExpr::Dot { base, field } => {
            let (left_opt, left_result) = interpret_d_expr(context, location, base);
            match left_opt {
                None => (None, left_result),
                Some(Term::DTerm(diagram)) => {
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context, location, &field.inner, field.span
                    );
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
                        Some(Component::Term(_)) | Some(Component::Hole) => {
                            let mut r = combined;
                            r.add_error(make_error(field.span, "Not a well-formed diagram expression"));
                            (None, r)
                        }
                    }
                }
                Some(Term::MTerm(mc)) => {
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context, &*mc.source, &field.inner, field.span
                    );
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Hole) | Some(Component::Bd(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(field.span, "Not a diagram or map"));
                            (None, r)
                        }
                        Some(Component::Term(Term::DTerm(d))) => {
                            match PMap::apply(&mc.map, &d) {
                                Ok(d_img) => (Some(Term::DTerm(d_img)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error(field.span, e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        Some(Component::Term(Term::MTerm(right_mc))) => {
                            let composed = PMap::compose(&mc.map, &right_mc.map);
                            (Some(Term::MTerm(MapComponent { map: composed, source: right_mc.source })), combined)
                        }
                    }
                }
            }
        }
    }
}

pub fn interpret_d_comp(
    context: &Context,
    location: &Complex,
    d_comp: &DComponent,
    span: Span,
) -> (Option<Component>, InterpResult) {
    match d_comp {
        DComponent::Name(name) => {
            let base_result = InterpResult::ok(context.clone());
            if let Some(diagram) = location.find_diagram(name) {
                return (Some(Component::Term(Term::DTerm(diagram.clone()))), base_result);
            }
            if let Some(entry) = location.find_map(name) {
                let source = match &entry.domain {
                    crate::core::complex::MapDomain::Type(id) => match context.state.find_type(*id) {
                        Some(te) => Arc::clone(&te.complex),
                        None => {
                            let mut r = base_result;
                            r.add_error(make_error(span, format!("Type {} not found", id)));
                            return (None, r);
                        }
                    },
                    crate::core::complex::MapDomain::Module(mid) => match context.state.find_module_arc(mid) {
                        Some(m) => m,
                        None => {
                            let mut r = base_result;
                            r.add_error(make_error(span, format!("Module `{}` not found", mid)));
                            return (None, r);
                        }
                    },
                };
                return (Some(Component::Term(Term::MTerm(MapComponent {
                    map: entry.map.clone(),
                    source,
                }))), base_result);
            }
            let mut r = base_result;
            r.add_error(make_error(span, format!("Name `{}` not found", name)));
            (None, r)
        }
        DComponent::In => {
            (Some(Component::Bd(DiagramSign::Input)), InterpResult::ok(context.clone()))
        }
        DComponent::Out => {
            (Some(Component::Bd(DiagramSign::Output)), InterpResult::ok(context.clone()))
        }
        DComponent::Paren(inner_diag) => {
            let (d_opt, result) = interpret_diagram(context, location, inner_diag);
            match d_opt {
                None => (None, result),
                Some(d) => (Some(Component::Term(Term::DTerm(d))), result),
            }
        }
        DComponent::Hole => {
            (Some(Component::Hole), InterpResult::ok(context.clone()))
        }
        DComponent::AnonMap { def, target } => {
            let (ns_opt, target_result) = super::interpreter::interpret_complex(
                context, super::types::Mode::Global, target,
            );
            match ns_opt {
                None => (None, target_result),
                Some(ns) => {
                    let (mc_opt, def_result) = super::pmap::interpret_pmap_def(
                        &target_result.context, &ns.location, location, def,
                    );
                    let combined = InterpResult::combine(target_result, def_result);
                    match mc_opt {
                        None => (None, combined),
                        Some(mc) => (Some(Component::Term(Term::MTerm(mc))), combined),
                    }
                }
            }
        }
    }
}

// ---- Assert ----

pub fn interpret_assert(
    context: &Context,
    location: &Complex,
    assert_stmt: &crate::language::ast::AssertStmt,
) -> (Option<TermPair>, InterpResult) {
    let (left_opt, left_result) = interpret_diagram_as_term(context, location, &assert_stmt.lhs);
    match left_opt {
        None => (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) = interpret_diagram_as_term(&left_result.context, location, &assert_stmt.rhs);
            let combined = InterpResult::combine(left_result, right_result);
            match right_opt {
                None => (None, combined),
                Some(right_term) => {
                    match (left_term, right_term) {
                        (Term::DTerm(d1), Term::DTerm(d2)) => {
                            (Some(TermPair::DTermPair { fst: d1, snd: d2 }), combined)
                        }
                        (Term::MTerm(mc1), Term::MTerm(mc2)) => {
                            (Some(TermPair::MTermPair {
                                fst: mc1.map,
                                snd: mc2.map,
                                source: mc1.source,
                            }), combined)
                        }
                        _ => {
                            let span = assert_stmt.lhs.span;
                            let mut r = combined;
                            r.add_error(make_error(span, "The two sides of the equation are incomparable"));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

pub fn interpret_diagram_as_term(
    context: &Context,
    location: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Term>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::Principal(exprs) => {
            interpret_principal_as_term(context, location, exprs, diagram.span)
        }
        ast::Diagram::Paste { lhs, dim, rhs } => {
            let k = match dim.inner.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error(dim.span,
                        format!("Invalid paste dimension: {}", dim.inner)));
                    return (None, r);
                }
            };
            // Right side first
            let (right_opt, right_result) = interpret_principal_as_term(context, location, rhs, diagram.span);
            match right_opt {
                None => (None, right_result),
                Some(Term::MTerm(_)) => {
                    let mut r = right_result;
                    r.add_error(make_error(diagram.span, "Not a diagram"));
                    (None, r)
                }
                Some(Term::DTerm(d_right)) => {
                    let (left_opt, left_result) = interpret_diagram_as_term(&right_result.context, location, lhs);
                    let combined = InterpResult::combine(right_result, left_result);
                    match left_opt {
                        None => (None, combined),
                        Some(Term::MTerm(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(diagram.span, "Not a diagram"));
                            (None, r)
                        }
                        Some(Term::DTerm(d_left)) => {
                            match Diagram::paste(k, &d_left, &d_right) {
                                Ok(d) => (Some(Term::DTerm(d)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error(diagram.span,
                                        format!("Failed to paste diagrams: {}", e)));
                                    (None, r)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn interpret_principal_as_term(
    context: &Context,
    location: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Term>, InterpResult) {
    if exprs.is_empty() {
        let mut r = InterpResult::ok(context.clone());
        r.add_error(make_error(span, "Empty diagram expression"));
        return (None, r);
    }

    let (first_opt, first_result) = interpret_d_expr(context, location, &exprs[0]);
    match first_opt {
        None => return (None, first_result),
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
                let (term_opt, expr_result) = interpret_d_expr(&result.context, location, expr);
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
                                result.add_error(make_error(span,
                                    format!("Failed to paste diagrams: {}", e)));
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
    location: &Complex,
    boundaries: &Spanned<ast::Boundary>,
) -> (Option<CellData>, InterpResult) {
    let (in_opt, src_result) = interpret_diagram(context, location, &boundaries.inner.source);
    match in_opt {
        None => (None, src_result),
        Some(boundary_in) => {
            let (out_opt, tgt_result) = interpret_diagram(&src_result.context, location, &boundaries.inner.target);
            let combined = InterpResult::combine(src_result, tgt_result);
            match out_opt {
                None => (None, combined),
                Some(boundary_out) => {
                    (Some(CellData::Boundary { boundary_in: Arc::new(boundary_in), boundary_out: Arc::new(boundary_out) }), combined)
                }
            }
        }
    }
}

// ---- Diagram naming ----

pub fn interpret_let_diag(
    context: &Context,
    location: &Complex,
    ld: &crate::language::ast::LetDiag,
) -> (Option<(String, Diagram)>, InterpResult) {
    let (diag_opt, diag_result) = interpret_diagram(context, location, &ld.value);
    match diag_opt {
        None => (None, diag_result),
        Some(diagram) => {
            let name = ld.name.inner.clone();
            (Some((name, diagram)), diag_result)
        }
    }
}
