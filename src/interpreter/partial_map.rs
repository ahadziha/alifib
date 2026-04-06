use super::diagram::interpret_diagram_as_term;
use super::scope::interpret_address;
use super::types::*;
use crate::aux::{self, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    partial_map::PartialMap,
};
use crate::language::ast::{self, DefPartialMap, PartialMapBasic, PartialMapClause, PartialMapDef, PartialMapExt, Span, Spanned};
use std::sync::Arc;

// ---- Hole boundary enrichment ----

fn make_hole_bd(scope: &Complex, map: &PartialMap, boundary: &Diagram) -> HoleBd {
    match PartialMap::apply(map, boundary) {
        Ok(mapped_boundary) => HoleBd::Full(mapped_boundary, Arc::new(scope.clone())),
        Err(_) => HoleBd::Partial {
            boundary: boundary.clone(),
            map: map.clone(),
            scope: Arc::new(scope.clone()),
        },
    }
}

fn enrich_hole(
    hole: &mut HoleInfo,
    scope: &Complex,
    domain: &Complex,
    map: &PartialMap,
    context: &Context,
) {
    let Some(source_tag) = &hole.source_tag else {
        return;
    };
    let Some(cell_data) = get_cell_data(context, domain, source_tag) else {
        return;
    };
    let CellData::Boundary {
        boundary_in,
        boundary_out,
    } = &cell_data
    else {
        return;
    };

    let hbd_in = make_hole_bd(scope, map, boundary_in);
    let hbd_out = make_hole_bd(scope, map, boundary_out);

    match &mut hole.boundary {
        Some(existing) => {
            if matches!(existing.boundary_in, HoleBd::Unknown) {
                existing.boundary_in = hbd_in;
            }
            if matches!(existing.boundary_out, HoleBd::Unknown) {
                existing.boundary_out = hbd_out;
            }
        }
        None => {
            hole.boundary = Some(HoleBoundaryInfo {
                boundary_in: hbd_in,
                boundary_out: hbd_out,
            });
        }
    }
}

fn enrich_holes(
    result: &mut InterpResult,
    scope: &Complex,
    domain: &Complex,
    map: &PartialMap,
) {
    let context = result.context.clone();
    for hole in &mut result.holes {
        enrich_hole(hole, scope, domain, map, &context);
    }
}

pub fn interpret_anon_map_component(
    context: &Context,
    domain: &Complex,
    target: &Spanned<ast::Complex>,
    def: &Spanned<PartialMapDef>,
) -> Step<EvalMap> {
    let (ns_opt, target_result) =
        super::eval::interpret_complex(context, super::types::Mode::Global, target);
    let Some(ns) = ns_opt else { return (None, target_result); };
    let (mc_opt, def_result) =
        interpret_pmap_def(&target_result.context, &ns.working_complex, domain, def);
    (mc_opt, InterpResult::combine(target_result, def_result))
}

// ---- PartialMap interpretation ----

/// Bundled context for evaluating partial map expressions.
/// Packages the three parameters that travel together through every internal
/// evaluation function: the interpreter state, the lexical scope for name
/// resolution, and the complex being mapped from.
#[derive(Clone, Copy)]
struct PartialMapCtx<'a> {
    context: &'a Context,
    /// Lexical environment: where diagram and map names are resolved.
    scope: &'a Complex,
    /// The complex being mapped from (the domain of definition).
    domain: &'a Complex,
}

pub fn interpret_partial_map(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    partial_map: &Spanned<ast::PartialMap>,
) -> Step<EvalMap> {
    eval_partial_map(&PartialMapCtx { context, scope, domain }, &partial_map.inner, partial_map.span)
}

fn eval_partial_map(ctx: &PartialMapCtx<'_>, partial_map: &ast::PartialMap, span: Span) -> Step<EvalMap> {
    match partial_map {
        ast::PartialMap::Basic(basic) => eval_partial_map_basic(ctx, basic, span),
        ast::PartialMap::Dot { base, rest } => {
            let (base_opt, base_result) = eval_partial_map_basic(ctx, base, span);
            let Some(base_map) = base_opt else { return (None, base_result); };
            // Dot traversal: the new lookup scope is the base map's domain.
            let (rest_opt, rest_result) =
                interpret_partial_map(&base_result.context, &base_map.domain, ctx.domain, rest);
            let combined = InterpResult::combine(base_result, rest_result);
            let Some(rest_map) = rest_opt else { return (None, combined); };
            let composed = PartialMap::compose(&base_map.map, &rest_map.map);
            (Some(EvalMap { map: composed, domain: rest_map.domain }), combined)
        }
    }
}

fn eval_partial_map_basic(ctx: &PartialMapCtx<'_>, basic: &PartialMapBasic, span: Span) -> Step<EvalMap> {
    match basic {
        PartialMapBasic::Name(name) => {
            let Some((map, domain)) = ctx.scope.find_map(name) else {
                return fail(ctx.context, span, format!("Partial map not found: `{}`", name));
            };
            let (domain_opt, result) = resolve_map_domain_complex(ctx.context, domain, span);
            let Some(domain) = domain_opt else {
                return (None, result);
            };
            (Some(EvalMap { map: map.clone(), domain }), InterpResult::ok(ctx.context.clone()))
        }
        PartialMapBasic::AnonMap { def, target } => {
            interpret_anon_map_component(ctx.context, ctx.domain, target, def)
        }
        PartialMapBasic::Paren(inner) => interpret_partial_map(ctx.context, ctx.scope, ctx.domain, inner),
    }
}

// ---- PartialMapDef / PartialMapExt interpretation ----

pub fn interpret_pmap_def(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    partial_map_def: &Spanned<PartialMapDef>,
) -> Step<EvalMap> {
    let ctx = PartialMapCtx { context, scope, domain };
    match &partial_map_def.inner {
        PartialMapDef::PartialMap(partial_map) => eval_partial_map(&ctx, partial_map, partial_map_def.span),
        PartialMapDef::Ext(ext) => interpret_partial_map_ext(&ctx, ext),
    }
}

fn initial_eval_map(ctx: &PartialMapCtx<'_>, prefix: &Option<Box<Spanned<ast::PartialMap>>>) -> Step<EvalMap> {
    match prefix {
        None => (
            Some(EvalMap { map: PartialMap::empty(), domain: Arc::new(ctx.domain.clone()) }),
            InterpResult::ok(ctx.context.clone()),
        ),
        Some(prefix) => interpret_partial_map(ctx.context, ctx.scope, ctx.domain, prefix),
    }
}

fn eval_pmap_clauses(
    ctx: &PartialMapCtx<'_>,
    initial_map: PartialMap,
    clauses: &[Spanned<PartialMapClause>],
) -> Step<PartialMap> {
    let mut map = initial_map;
    let mut result = InterpResult::ok(ctx.context.clone());

    for clause in clauses {
        let step_ctx = PartialMapCtx { context: &result.context, ..*ctx };
        let (next_map, clause_result) = interpret_partial_map_clause(&step_ctx, map, clause);
        result = InterpResult::combine(result, clause_result);
        let Some(updated_map) = next_map else {
            return (None, result);
        };
        map = updated_map;
        if result.has_errors() {
            return (Some(map), result);
        }
    }

    (Some(map), result)
}

fn interpret_partial_map_ext(ctx: &PartialMapCtx<'_>, ext: &PartialMapExt) -> Step<EvalMap> {
    let (initial_opt, prefix_result) = initial_eval_map(ctx, &ext.prefix);
    let Some(initial) = initial_opt else {
        return (None, prefix_result);
    };

    let effective_domain = Arc::clone(&initial.domain);
    let clauses_ctx = PartialMapCtx { context: &prefix_result.context, domain: &effective_domain, ..*ctx };
    let (map_opt, clause_result) = eval_pmap_clauses(&clauses_ctx, initial.map, &ext.clauses);
    let Some(current_map) = map_opt else {
        return (None, InterpResult::combine(prefix_result, clause_result));
    };

    let mut result = InterpResult::combine(prefix_result, clause_result);
    enrich_holes(&mut result, ctx.scope, &effective_domain, &current_map);
    (Some(EvalMap { map: current_map, domain: effective_domain }), result)
}

fn mark_last_hole_source_tag(result: &mut InterpResult, source_term: &Term) {
    let Term::Diag(source_diagram) = source_term else {
        return;
    };
    if !source_diagram.is_cell() {
        return;
    }

    let Some(tag) = source_diagram.top_label() else {
        return;
    };
    if let Some(last_hole) = result.holes.last_mut() {
        last_hole.source_tag = Some(tag.clone());
    }
}

fn interpret_partial_map_clause(ctx: &PartialMapCtx<'_>, map: PartialMap, clause: &Spanned<PartialMapClause>) -> Step<PartialMap> {
    let (left_opt, left_result) = interpret_diagram_as_term(ctx.context, ctx.domain, &clause.inner.lhs);
    let Some(left_term) = left_opt else { return (None, left_result); };

    let (right_opt, right_result) =
        interpret_diagram_as_term(&left_result.context, ctx.scope, &clause.inner.rhs);
    let mut combined = InterpResult::combine(left_result, right_result);

    let Some(right_term) = right_opt else {
        // Right side failed. If a hole was recorded, tag it with the left-side source and
        // return the partially-built map; otherwise the whole clause fails.
        if combined.holes.is_empty() {
            return (None, combined);
        }
        mark_last_hole_source_tag(&mut combined, &left_term);
        return (Some(map), combined);
    };

    match interpret_assign(&combined.context, map, ctx.domain, ctx.scope, &left_term, &right_term) {
        Ok(new_map) => (Some(new_map), combined),
        Err(e) => {
            combined.add_error(make_error_from_core(clause.span, e));
            (None, combined)
        }
    }
}

/// Handle assignment of a term to another term in a map clause.
fn extend_matching_map_images(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    left_map: &EvalMap,
    right_map: &EvalMap,
) -> Result<PartialMap, aux::Error> {
    let map_domain = &*left_map.domain;
    let mut extended = map;

    for (_, generator_name, tag) in sorted_generators(map_domain) {
        match (left_map.map.is_defined_at(&tag), right_map.map.is_defined_at(&tag)) {
            (false, false) => {}
            (true, false) => return Err(aux::Error::new(format!(
                "`{}` is in the domain of the first map but not the second",
                generator_name
            ))),
            (false, true) => return Err(aux::Error::new(format!(
                "`{}` is in the domain of the second map but not the first",
                generator_name
            ))),
            (true, true) => {
                let left_image = left_map.map.image(&tag)?;
                if left_image.is_cell() {
                    let right_image = right_map.map.image(&tag)?;
                    extended = extend_map_for_cell(context, extended, domain, target, left_image, right_image)?;
                } else {
                    let all_defined = left_image.all_labels().all(|tag| extended.is_defined_at(tag));
                    if !all_defined {
                        return Err(aux::Error::new("Failed to extend map (not enough information)"));
                    }
                }
            }
        }
    }

    Ok(extended)
}

fn interpret_assign(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    left: &Term,
    right: &Term,
) -> Result<PartialMap, aux::Error> {
    match (left, right) {
        (Term::Diag(d_left), Term::Diag(d_right)) => {
            extend_map_for_cell(context, map, domain, target, d_left, d_right)
        }
        (Term::Map(mc_left), Term::Map(mc_right)) => {
            if !Arc::ptr_eq(&mc_left.domain, &mc_right.domain) {
                return Err(aux::Error::new("Not a well-formed assignment"));
            }
            extend_matching_map_images(context, map, domain, target, mc_left, mc_right)
        }
        _ => Err(aux::Error::new("Not a well-formed assignment")),
    }
}

fn boundary_dependencies(cell_data: &CellData, map: &PartialMap) -> Vec<(Tag, DiagramSign)> {
    let CellData::Boundary { boundary_in, boundary_out } = cell_data else {
        return vec![];
    };
    [(boundary_in.as_ref(), DiagramSign::Source), (boundary_out.as_ref(), DiagramSign::Target)]
        .into_iter()
        .flat_map(|(boundary, sign)| {
            let d = boundary.top_dim();
            boundary.labels_at(d).into_iter().flat_map(move |row| {
                row.iter()
                    .filter(|tag| !map.is_defined_at(tag))
                    .map(move |tag| (tag.clone(), sign))
            })
        })
        .collect()
}

fn boundary_of_sign(
    cell_data: &CellData,
    sign: DiagramSign,
) -> Option<Arc<Diagram>> {
    match (cell_data, sign) {
        (CellData::Boundary { boundary_in, .. }, DiagramSign::Source) => Some(boundary_in.clone()),
        (CellData::Boundary { boundary_out, .. }, DiagramSign::Target) => Some(boundary_out.clone()),
        _ => None,
    }
}

fn image_classifier_via_boundary(
    focus: &Tag,
    source_boundary: &Diagram,
    target_boundary: &Diagram,
    target: &Complex,
) -> Result<Diagram, aux::Error> {
    let mapped_tag = Diagram::map_tag_via_shape_iso(source_boundary, target_boundary, focus)
        .map_err(|e| aux::Error::new(format!("Failed to extend map ({})", e)))?;
    let generator_name = target
        .find_generator_by_tag(&mapped_tag)
        .ok_or_else(|| aux::Error::new("Image tag not found in target complex"))?
        .clone();
    target.classifier(&generator_name)
        .ok_or_else(|| aux::Error::new("Classifier not found for image generator"))
        .cloned()
}

/// Smart extension of a map: adds a mapping from a source cell to a target diagram,
/// recursively extending for boundary cells as needed.
pub fn extend_map_for_cell(
    context: &Context,
    map: PartialMap,
    domain: &Complex,
    target: &Complex,
    domain_diag: &Diagram,
    target_diag: &Diagram,
) -> Result<PartialMap, aux::Error> {
    if !domain_diag.is_cell() {
        return Err(aux::Error::new(
            "Left-hand side of map instruction must be a cell",
        ));
    }
    let d = domain_diag.top_dim();
    let tag = domain_diag
        .top_label()
        .ok_or_else(|| aux::Error::new("Domain cell has no top label"))?
        .clone();

    if map.is_defined_at(&tag) {
        let current = map.image(&tag)?;
        if Diagram::isomorphic(current, target_diag) {
            return Ok(map);
        } else {
            return Err(aux::Error::new(
                "The same generator is mapped to multiple diagrams",
            ));
        }
    }

    let cell_data = get_cell_data(context, domain, &tag)
        .ok_or_else(|| aux::Error::new("Cannot find cell data for generator"))?;

    // Extend the map for any boundary dependencies not yet in the domain.
    let mut current_map = map;
    for (focus_tag, sign) in boundary_dependencies(&cell_data, &current_map) {
        if current_map.is_defined_at(&focus_tag) {
            continue;
        }
        let focus_cell_data = get_cell_data(context, domain, &focus_tag).ok_or_else(|| {
            aux::Error::new(format!("Cannot find cell data for boundary cell {}", focus_tag))
        })?;
        let target_boundary = Diagram::boundary(sign, d - 1, target_diag)?;
        let Some(source_boundary) = boundary_of_sign(&cell_data, sign) else { continue; };
        current_map = if source_boundary.is_cell() {
            extend_map_for_cell(context, current_map, domain, target, &source_boundary, &target_boundary)?
        } else {
            let focus_image = image_classifier_via_boundary(&focus_tag, &source_boundary, &target_boundary, target)?;
            let focus_diagram = Diagram::cell(focus_tag.clone(), &focus_cell_data)?;
            extend_map_for_cell(context, current_map, domain, target, &focus_diagram, &focus_image)?
        };
    }

    PartialMap::extend(current_map, tag, d, cell_data, target_diag.clone())
}

// ---- Partial map naming ----

fn check_map_totality(
    result: &mut InterpResult,
    domain: &Complex,
    map: &PartialMap,
    map_name: &str,
    name_span: Span,
    is_total: bool,
) {
    if !is_total {
        return;
    }

    for (generator_name, tag, _) in domain.generators_iter() {
        if !map.is_defined_at(tag) {
            result.add_error(make_error(
                name_span,
                format!("Total map `{}` is not defined on generator `{}`", map_name, generator_name),
            ));
        }
    }
}

pub fn interpret_def_pmap(
    context: &Context,
    scope: &Complex,
    dp: &DefPartialMap,
) -> (Option<(LocalId, PartialMap, MapDomain)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &dp.address.inner, dp.address.span);
    let Some(id) = id_opt else {
        return (None, addr_result);
    };

    let context_after = addr_result.context.clone();
    let (domain_opt, domain_result) =
        resolve_type_complex(&context_after, id, dp.address.span, "Type not found");
    let Some(domain) = domain_opt else {
        return (None, InterpResult::combine(addr_result, domain_result));
    };

    let (eval_map_opt, def_result) = interpret_pmap_def(&context_after, scope, &domain, &dp.value);
    let mut combined = InterpResult::combine(addr_result, def_result);

    let Some(eval_map) = eval_map_opt else {
        return (None, combined);
    };

    check_map_totality(&mut combined, &domain, &eval_map.map, &dp.name.inner, dp.name.span, dp.total);

    let name = dp.name.inner.clone();
    (Some((name, eval_map.map, MapDomain::Type(id))), combined)
}

