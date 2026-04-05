use super::diagram::{interpret_diagram_as_term, render_boundary_partial, render_diagram};
use super::scope::interpret_address;
use super::types::*;
use crate::aux::{self, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use crate::language::ast::{self, DefPMap, PMapBasic, PMapClause, PMapDef, PMapExt, Span, Spanned};
use std::sync::Arc;

// ---- Hole boundary rendering ----

fn render_mapped_boundary(
    scope: &Complex,
    map: &PMap,
    boundary: &Diagram,
) -> String {
    match PMap::apply(map, boundary) {
        Ok(mapped_boundary) => render_diagram(&mapped_boundary, scope),
        Err(_) => render_boundary_partial(boundary, map, scope),
    }
}

fn fill_hole_boundary(
    hole: &mut HoleInfo,
    scope: &Complex,
    domain: &Complex,
    map: &PMap,
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

    let rendered_in = render_mapped_boundary(scope, map, boundary_in);
    let rendered_out = render_mapped_boundary(scope, map, boundary_out);

    match &mut hole.boundary {
        Some(existing) => {
            if existing.boundary_in == "?" {
                existing.boundary_in = rendered_in;
            }
            if existing.boundary_out == "?" {
                existing.boundary_out = rendered_out;
            }
        }
        None => {
            hole.boundary = Some(HoleBoundaryInfo {
                boundary_in: rendered_in,
                boundary_out: rendered_out,
            });
        }
    }
}

fn finalize_hole_boundaries(
    result: &mut InterpResult,
    scope: &Complex,
    domain: &Complex,
    map: &PMap,
) {
    let context = result.context.clone();
    for hole in &mut result.holes {
        fill_hole_boundary(hole, scope, domain, map, &context);
    }
}

pub fn interpret_anon_map_component(
    context: &Context,
    domain: &Complex,
    target: &Spanned<ast::Complex>,
    def: &Spanned<PMapDef>,
) -> Step<EvalMap> {
    let (ns_opt, target_result) =
        super::interpreter::interpret_complex(context, super::types::Mode::Global, target);
    let Some(ns) = ns_opt else { return (None, target_result); };
    let (mc_opt, def_result) =
        interpret_pmap_def(&target_result.context, &ns.working_complex, domain, def);
    (mc_opt, InterpResult::combine(target_result, def_result))
}

// ---- PMap interpretation ----

pub fn interpret_pmap(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    pmap: &Spanned<ast::PMap>,
) -> Step<EvalMap> {
    interpret_pmap_node(context, scope, domain, &pmap.inner, pmap.span)
}

fn interpret_pmap_node(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    pmap: &ast::PMap,
    span: Span,
) -> Step<EvalMap> {
    match pmap {
        ast::PMap::Basic(basic) => interpret_pmap_basic(context, scope, domain, basic, span),
        ast::PMap::Dot { base, rest } => {
            let (base_opt, base_result) = interpret_pmap_basic(context, scope, domain, base, span);
            let Some(base_map) = base_opt else { return (None, base_result); };
            let (rest_opt, rest_result) =
                interpret_pmap(&base_result.context, &*base_map.domain, domain, rest);
            let combined = InterpResult::combine(base_result, rest_result);
            let Some(rest_map) = rest_opt else { return (None, combined); };
            let composed = PMap::compose(&base_map.map, &rest_map.map);
            (Some(EvalMap { map: composed, domain: rest_map.domain }), combined)
        }
    }
}

fn interpret_pmap_basic(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    basic: &PMapBasic,
    span: Span,
) -> Step<EvalMap> {
    match basic {
        PMapBasic::Name(name) => {
            let Some(entry) = scope.find_map(name) else {
                return fail(context, span, format!("Partial map not found: `{}`", name));
            };
            let (domain_opt, result) = resolve_map_domain_complex(context, &entry.domain, span);
            let Some(domain) = domain_opt else {
                return (None, result);
            };
            (Some(EvalMap { map: entry.map.clone(), domain }), InterpResult::ok(context.clone()))
        }
        PMapBasic::AnonMap { def, target } => {
            interpret_anon_map_component(context, domain, target, def)
        }
        PMapBasic::Paren(inner) => interpret_pmap(context, scope, domain, inner),
    }
}

// ---- PMapDef / PMapExt interpretation ----

pub fn interpret_pmap_def(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    pmap_def: &Spanned<PMapDef>,
) -> Step<EvalMap> {
    match &pmap_def.inner {
        PMapDef::PMap(pmap) => interpret_pmap_node(context, scope, domain, pmap, pmap_def.span),
        PMapDef::Ext(ext) => interpret_pmap_ext(context, scope, domain, ext),
    }
}

fn initial_eval_map(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    prefix: &Option<Box<Spanned<ast::PMap>>>,
) -> Step<EvalMap> {
    match prefix {
        None => (
            Some(EvalMap { map: PMap::empty(), domain: Arc::new(domain.clone()) }),
            InterpResult::ok(context.clone()),
        ),
        Some(prefix) => interpret_pmap(context, scope, domain, prefix),
    }
}

fn apply_pmap_clauses(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    initial_map: PMap,
    clauses: &[Spanned<PMapClause>],
) -> Step<PMap> {
    let mut map = initial_map;
    let mut result = InterpResult::ok(context.clone());

    for clause in clauses {
        let (next_map, clause_result) =
            interpret_pmap_clause(&result.context, scope, domain, map, clause);
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

fn interpret_pmap_ext(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    ext: &PMapExt,
) -> Step<EvalMap> {
    let (initial_opt, prefix_result) = initial_eval_map(context, scope, domain, &ext.prefix);
    let Some(initial) = initial_opt else {
        return (None, prefix_result);
    };

    let effective_domain = Arc::clone(&initial.domain);
    let (map_opt, clause_result) = apply_pmap_clauses(
        &prefix_result.context,
        scope,
        &effective_domain,
        initial.map,
        &ext.clauses,
    );
    let Some(current_map) = map_opt else {
        return (None, InterpResult::combine(prefix_result, clause_result));
    };

    let mut result = InterpResult::combine(prefix_result, clause_result);
    finalize_hole_boundaries(&mut result, scope, &effective_domain, &current_map);
    (Some(EvalMap { map: current_map, domain: effective_domain }), result)
}

fn mark_last_hole_source_tag(result: &mut InterpResult, source_term: &Term) {
    let Term::Diag(source_diagram) = source_term else {
        return;
    };
    if !source_diagram.is_cell() {
        return;
    }

    let top_dim = source_diagram.top_dim();
    let Some(tag) = source_diagram.labels.get(top_dim).and_then(|row| row.first()) else {
        return;
    };
    if let Some(last_hole) = result.holes.last_mut() {
        last_hole.source_tag = Some(tag.clone());
    }
}

fn interpret_pmap_clause(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    map: PMap,
    clause: &Spanned<PMapClause>,
) -> Step<PMap> {
    let (left_opt, left_result) = interpret_diagram_as_term(context, domain, &clause.inner.lhs);
    let Some(left_term) = left_opt else { return (None, left_result); };

    let (right_opt, right_result) =
        interpret_diagram_as_term(&left_result.context, scope, &clause.inner.rhs);
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

    match interpret_assign(&combined.context, map, domain, scope, &left_term, &right_term, clause.span) {
        Ok(new_map) => (Some(new_map), combined),
        Err(e) => {
            combined.add_error(make_error(clause.span, e.to_string()));
            (None, combined)
        }
    }
}

/// Handle assignment of a term to another term in a map clause.
fn extend_matching_map_images(
    context: &Context,
    map: PMap,
    domain: &Complex,
    target: &Complex,
    left_map: &EvalMap,
    right_map: &EvalMap,
    span: Span,
) -> Result<PMap, aux::Error> {
    let map_domain = &*left_map.domain;
    let mut extended = map;

    for (_, generator_name, tag) in sorted_generators(map_domain) {
        match (left_map.map.is_defined_at(&tag), right_map.map.is_defined_at(&tag)) {
            (true, true) => {
                let left_image = left_map.map.image(&tag)?;
                if left_image.is_cell() {
                    let right_image = right_map.map.image(&tag)?;
                    extended = extend_map_for_cell(context, extended, domain, target, left_image, right_image, span)?;
                } else {
                    let all_defined = left_image
                        .labels
                        .iter()
                        .flat_map(|row| row.iter())
                        .all(|tag| extended.is_defined_at(tag));
                    if !all_defined {
                        return Err(aux::Error::new("Failed to extend map (not enough information)"));
                    }
                }
            }
            (true, false) => {
                return Err(aux::Error::new(format!(
                    "{} is in the domain of definition of the first map, but not the second map",
                    generator_name
                )));
            }
            (false, true) => {
                return Err(aux::Error::new(format!(
                    "{} is in the domain of definition of the second map, but not the first map",
                    generator_name
                )));
            }
            (false, false) => {}
        }
    }

    Ok(extended)
}

fn interpret_assign(
    context: &Context,
    map: PMap,
    domain: &Complex,
    target: &Complex,
    left: &Term,
    right: &Term,
    span: Span,
) -> Result<PMap, aux::Error> {
    match (left, right) {
        (Term::Diag(d_left), Term::Diag(d_right)) => {
            extend_map_for_cell(context, map, domain, target, d_left, d_right, span)
        }
        (Term::Map(mc_left), Term::Map(mc_right)) => {
            if !Arc::ptr_eq(&mc_left.domain, &mc_right.domain) {
                return Err(aux::Error::new("Not a well-formed assignment"));
            }
            extend_matching_map_images(context, map, domain, target, mc_left, mc_right, span)
        }
        _ => Err(aux::Error::new("Not a well-formed assignment")),
    }
}

fn boundary_dependencies(
    cell_data: &CellData,
    map: &PMap,
) -> Vec<(Tag, DiagramSign)> {
    match cell_data {
        CellData::Zero => vec![],
        CellData::Boundary {
            boundary_in,
            boundary_out,
        } => {
            let mut missing = vec![];
            for (boundary, sign) in &[
                (boundary_in, DiagramSign::Source),
                (boundary_out, DiagramSign::Target),
            ] {
                let boundary_dim = boundary.top_dim();
                if let Some(row) = boundary.labels.get(boundary_dim) {
                    for tag in row {
                        if !map.is_defined_at(tag) {
                            missing.push((tag.clone(), *sign));
                        }
                    }
                }
            }
            missing
        }
    }
}

fn source_boundary_for_sign(
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
    let embedding = crate::core::diagram::isomorphism_of(&source_boundary.shape, &target_boundary.shape)
        .map_err(|_| aux::Error::new("Failed to extend map (boundary shapes don't match)"))?;

    let boundary_dim = source_boundary.top_dim();
    let (Some(source_row), Some(map_row), Some(target_row)) = (
        source_boundary.labels.get(boundary_dim),
        embedding.map.get(boundary_dim),
        target_boundary.labels.get(boundary_dim),
    ) else {
        return Err(aux::Error::new("Failed to extend map (no image found)"));
    };

    let mut image_tag: Option<Tag> = None;
    for (index, tag) in source_row.iter().enumerate() {
        if tag != focus { continue; }
        let Some(&mapped_index) = map_row.get(index) else { continue; };
        let Some(mapped_tag) = target_row.get(mapped_index) else { continue; };
        match &image_tag {
            None => image_tag = Some(mapped_tag.clone()),
            Some(existing) if existing != mapped_tag => {
                return Err(aux::Error::new("The same generator is mapped to multiple diagrams"));
            }
            _ => {}
        }
    }

    let mapped_tag = image_tag.ok_or_else(|| aux::Error::new("Failed to extend map (no image found)"))?;
    let generator_name = target
        .find_generator_by_tag(&mapped_tag)
        .ok_or_else(|| aux::Error::new("Image tag not found in target complex"))?
        .clone();
    target.classifier(&generator_name)
        .ok_or_else(|| aux::Error::new("Classifier not found for image generator"))
        .cloned()
}

fn extend_missing_boundary_dependencies(
    context: &Context,
    map: PMap,
    domain: &Complex,
    target: &Complex,
    source_cell_data: &CellData,
    source_dim: usize,
    target_diagram: &Diagram,
    span: Span,
) -> Result<PMap, aux::Error> {
    let mut current_map = map;

    for (focus_tag, sign) in boundary_dependencies(source_cell_data, &current_map) {
        if current_map.is_defined_at(&focus_tag) {
            continue;
        }

        let focus_cell_data = get_cell_data(context, domain, &focus_tag).ok_or_else(|| {
            aux::Error::new(format!("Cannot find cell data for boundary cell {}", focus_tag))
        })?;
        let target_boundary = Diagram::boundary(sign, source_dim - 1, target_diagram)?;
        let Some(source_boundary) = source_boundary_for_sign(source_cell_data, sign) else {
            continue;
        };

        current_map = if source_boundary.is_cell() {
            extend_map_for_cell(
                context,
                current_map,
                domain,
                target,
                &source_boundary,
                &target_boundary,
                span,
            )?
        } else {
            let focus_image = image_classifier_via_boundary(
                &focus_tag,
                &source_boundary,
                &target_boundary,
                target,
            )?;
            let focus_diagram = Diagram::cell(focus_tag.clone(), &focus_cell_data)?;
            extend_map_for_cell(
                context,
                current_map,
                domain,
                target,
                &focus_diagram,
                &focus_image,
                span,
            )?
        };
    }

    Ok(current_map)
}

/// Smart extension of a map: adds a mapping from a source cell to a target diagram,
/// recursively extending for boundary cells as needed.
pub fn extend_map_for_cell(
    context: &Context,
    map: PMap,
    domain: &Complex,
    target: &Complex,
    domain_diag: &Diagram,
    target_diag: &Diagram,
    span: Span,
) -> Result<PMap, aux::Error> {
    if !domain_diag.is_cell() {
        return Err(aux::Error::new(
            "Left-hand side of map instruction must be a cell",
        ));
    }
    let d = domain_diag.top_dim();
    let tag = domain_diag
        .labels
        .get(d)
        .and_then(|r| r.first())
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

    let current = extend_missing_boundary_dependencies(
        context, map, domain, target, &cell_data, d, target_diag, span,
    )?;

    PMap::extend(current, tag, d, cell_data, target_diag.clone())
}

// ---- Partial map naming ----

fn ensure_total_map_defined(
    result: &mut InterpResult,
    domain: &Complex,
    map: &PMap,
    map_name: &str,
    name_span: Span,
    is_total: bool,
) {
    if !is_total {
        return;
    }

    for generator_name in domain.generator_names() {
        if let Some(generator_entry) = domain.find_generator(&generator_name) {
            if !map.is_defined_at(&generator_entry.tag) {
                result.add_error(make_error(
                    name_span,
                    format!(
                        "Total map `{}` is not defined on generator `{}`",
                        map_name, generator_name
                    ),
                ));
            }
        }
    }
}

pub fn interpret_def_pmap(
    context: &Context,
    scope: &Complex,
    dp: &DefPMap,
) -> (Option<(LocalId, PMap, MapDomain)>, InterpResult) {
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

    ensure_total_map_defined(&mut combined, &domain, &eval_map.map, &dp.name.inner, dp.name.span, dp.total);

    let name = dp.name.inner.clone();
    (Some((name, eval_map.map, MapDomain::Type(id))), combined)
}

