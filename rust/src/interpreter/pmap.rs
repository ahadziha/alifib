use super::diagram::{interpret_diagram_as_term, render_boundary_partial, render_diagram};
use super::types::*;
use crate::aux::{self, GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use crate::language::ast::{
    self, Address, DefPMap, PMapBasic, PMapClause, PMapDef, PMapExt, Span, Spanned,
};
use std::sync::Arc;

// ---- Address resolution ----

pub fn interpret_address(context: &Context, address: &Address, addr_span: Span) -> Step<GlobalId> {
    let module_id = &context.current_module;

    let module_space = match context.state.find_module(module_id) {
        None => {
            let mut r = InterpResult::ok(context.clone());
            r.add_error(make_error(
                addr_span,
                format!("Module `{}` not found", module_id),
            ));
            return (None, r);
        }
        Some(m) => m,
    };

    let segments: Vec<(Span, String)> = address.iter().map(|n| (n.span, n.inner.clone())).collect();

    let base_result = InterpResult::ok(context.clone());

    if segments.is_empty() {
        let (id_opt, root_result) = resolve_root_owner_type_id(context, module_space, addr_span);
        return (id_opt, InterpResult::combine(base_result, root_result));
    }

    let last_idx = segments.len() - 1;
    let prefix = &segments[..last_idx];
    let (last_span, last_name) = &segments[last_idx];

    let mut current_space = module_space.clone();
    for (seg_span, seg_name) in prefix {
        match current_space.find_map(seg_name) {
            None => {
                let mut r = base_result;
                r.add_error(make_error(
                    *seg_span,
                    format!("Partial map `{}` not found", seg_name),
                ));
                return (None, r);
            }
            Some(me) => match &me.domain {
                MapDomain::Module(mid) => match context.state.find_module(mid) {
                    Some(m) => current_space = m.clone(),
                    None => {
                        let mut r = base_result;
                        r.add_error(make_error(*seg_span, format!("Module `{}` not found", mid)));
                        return (None, r);
                    }
                },
                MapDomain::Type(_) => {
                    let mut r = base_result;
                    r.add_error(make_error(
                        *seg_span,
                        format!("Domain of `{}` is not a module", seg_name),
                    ));
                    return (None, r);
                }
            },
        }
    }

    match current_space.find_diagram(last_name) {
        None => {
            let mut r = base_result;
            r.add_error(make_error(*last_span, format!("Type `{}` not found", last_name)));
            (None, r)
        }
        Some(diagram) => {
            if !diagram.is_cell() {
                let mut r = base_result;
                r.add_error(make_error(*last_span, format!("`{}` is not a cell", last_name)));
                return (None, r);
            }
            let d = dim_index(diagram.dim());
            match diagram.labels.get(d).and_then(|row| row.first()) {
                None => {
                    let mut r = base_result;
                    r.add_error(make_error(*last_span, "Cell has no top label"));
                    (None, r)
                }
                Some(Tag::Global(id)) => (Some(*id), base_result),
                Some(Tag::Local(_)) => {
                    let mut r = base_result;
                    r.add_error(make_error(*last_span, "Cell has local tag (unexpected)"));
                    (None, r)
                }
            }
        }
    }
}

pub fn interpret_anon_map_component(
    context: &Context,
    domain: &Complex,
    target: &Spanned<ast::Complex>,
    def: &Spanned<PMapDef>,
) -> Step<MapComponent> {
    let (ns_opt, target_result) =
        super::interpreter::interpret_complex(context, super::types::Mode::Global, target);
    match ns_opt {
        None => (None, target_result),
        Some(ns) => {
            let (mc_opt, def_result) =
                interpret_pmap_def(&target_result.context, &ns.working_complex, domain, def);
            (mc_opt, InterpResult::combine(target_result, def_result))
        }
    }
}

// ---- PMap interpretation ----

pub fn interpret_pmap(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    pmap: &Spanned<ast::PMap>,
) -> Step<MapComponent> {
    interpret_pmap_inner(context, scope, domain, &pmap.inner, pmap.span)
}

fn interpret_pmap_inner(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    pmap: &ast::PMap,
    span: Span,
) -> Step<MapComponent> {
    match pmap {
        ast::PMap::Basic(basic) => interpret_pmap_basic(context, scope, domain, basic, span),
        ast::PMap::Dot { base, rest } => {
            let (base_opt, base_result) = interpret_pmap_basic(context, scope, domain, base, span);
            match base_opt {
                None => (None, base_result),
                Some(base_comp) => {
                    let (rest_opt, rest_result) =
                        interpret_pmap(&base_result.context, &*base_comp.domain, domain, rest);
                    let combined = InterpResult::combine(base_result, rest_result);
                    match rest_opt {
                        None => (None, combined),
                        Some(rest_comp) => {
                            let composed = PMap::compose(&base_comp.map, &rest_comp.map);
                            (
                                Some(MapComponent {
                                    map: composed,
                                    domain: rest_comp.domain,
                                }),
                                combined,
                            )
                        }
                    }
                }
            }
        }
    }
}

fn interpret_pmap_basic(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    basic: &PMapBasic,
    span: Span,
) -> Step<MapComponent> {
    match basic {
        PMapBasic::Name(name) => {
            let base_result = InterpResult::ok(context.clone());
            match scope.find_map(name) {
                None => fail(context, span, format!("Partial map not found: `{}`", name)),
                Some(entry) => {
                    let (domain_opt, domain_result) =
                        resolve_map_domain_complex(context, &entry.domain, span);
                    let domain_arc = match domain_opt {
                        None => return (None, InterpResult::combine(base_result, domain_result)),
                        Some(domain) => domain,
                    };
                    (
                        Some(MapComponent {
                            map: entry.map.clone(),
                            domain: domain_arc,
                        }),
                        base_result,
                    )
                }
            }
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
) -> Step<MapComponent> {
    match &pmap_def.inner {
        PMapDef::PMap(pmap) => interpret_pmap_inner(context, scope, domain, pmap, pmap_def.span),
        PMapDef::Ext(ext) => interpret_pmap_ext(context, scope, domain, ext, pmap_def.span),
    }
}

fn interpret_pmap_ext(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    ext: &PMapExt,
    span: Span,
) -> Step<MapComponent> {
    // Start with prefix map or empty map
    let (initial_mc, prefix_result) = match &ext.prefix {
        None => {
            let map = PMap::empty().unwrap();
            (
                MapComponent {
                    map,
                    domain: Arc::new(domain.clone()),
                },
                InterpResult::ok(context.clone()),
            )
        }
        Some(prefix) => {
            let (mc_opt, r) = interpret_pmap(context, scope, domain, prefix);
            match mc_opt {
                None => return (None, r),
                Some(mc) => (mc, r),
            }
        }
    };

    // Apply each clause
    let mut current_map = initial_mc.map;
    let effective_domain = &*initial_mc.domain;
    let mut acc_result = prefix_result;

    for clause in &ext.clauses {
        let ctx = acc_result.context.clone();
        let (m_opt, clause_result) =
            interpret_pm_clause(&ctx, scope, effective_domain, current_map, clause, span);
        acc_result = InterpResult::combine(acc_result, clause_result);
        match m_opt {
            None => return (None, acc_result),
            Some(new_m) => current_map = new_m,
        }
        if acc_result.has_errors() {
            return (
                Some(MapComponent {
                    map: current_map,
                    domain: initial_mc.domain,
                }),
                acc_result,
            );
        }
    }

    // Deferred hole boundary computation: use the map as-is after all clauses.
    let ctx = &acc_result.context;
    for hole in &mut acc_result.holes {
        if let Some(tag) = &hole.source_tag {
            if let Some(cell_data) = get_cell_data(ctx, effective_domain, tag) {
                if let CellData::Boundary {
                    boundary_in,
                    boundary_out,
                } = &cell_data
                {
                    let rendered_in = match PMap::apply(&current_map, boundary_in) {
                        Ok(mi) => render_diagram(&mi, scope),
                        Err(_) => render_boundary_partial(boundary_in, &current_map, scope),
                    };
                    let rendered_out = match PMap::apply(&current_map, boundary_out) {
                        Ok(mo) => render_diagram(&mo, scope),
                        Err(_) => render_boundary_partial(boundary_out, &current_map, scope),
                    };
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
            }
        }
    }

    (
        Some(MapComponent {
            map: current_map,
            domain: initial_mc.domain,
        }),
        acc_result,
    )
}

fn interpret_pm_clause(
    context: &Context,
    scope: &Complex,
    domain: &Complex,
    map: PMap,
    clause: &Spanned<PMapClause>,
    _span: Span,
) -> Step<PMap> {
    let (left_opt, left_result) = interpret_diagram_as_term(context, domain, &clause.inner.lhs);
    match left_opt {
        None => return (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) =
                interpret_diagram_as_term(&left_result.context, scope, &clause.inner.rhs);
            let mut combined = InterpResult::combine(left_result, right_result);
            match right_opt {
                None => {
                    if combined.holes.is_empty() {
                        (None, combined)
                    } else {
                        // RHS was a hole — record source tag for deferred boundary computation
                        if let Term::DTerm(source_diag) = &left_term {
                            if source_diag.is_cell() {
                                let d = source_diag.dim().max(0) as usize;
                                if let Some(tag) = source_diag.labels.get(d).and_then(|r| r.first())
                                {
                                    if let Some(last_hole) = combined.holes.last_mut() {
                                        last_hole.source_tag = Some(tag.clone());
                                    }
                                }
                            }
                        }
                        // Return map unchanged so processing continues
                        (Some(map), combined)
                    }
                }
                Some(right_term) => {
                    match interpret_assign(
                        &combined.context,
                        map,
                        domain,
                        scope,
                        &left_term,
                        &right_term,
                        clause.span,
                    ) {
                        Ok(new_m) => (Some(new_m), combined),
                        Err(e) => {
                            let mut r = combined;
                            r.add_error(make_error(clause.span, e.to_string()));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

/// Handle assignment of a term to another term in a map clause.
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
        (Term::DTerm(d_left), Term::DTerm(d_right)) => {
            smart_extend(context, map, domain, target, d_left, d_right, span)
        }
        (Term::MTerm(mc_left), Term::MTerm(mc_right)) => {
            if !Arc::ptr_eq(&mc_left.domain, &mc_right.domain) {
                return Err(aux::Error::new("Not a well-formed assignment"));
            }
            let map_domain = &*mc_left.domain;

            let mut extended = map;
            for (_, gen_name, tag) in sorted_generators(map_domain) {
                let defined_left = mc_left.map.is_defined_at(&tag);
                let defined_right = mc_right.map.is_defined_at(&tag);
                if defined_left && defined_right {
                    let left_image = mc_left.map.image(&tag)?;
                    if left_image.is_cell() {
                        let right_image = mc_right.map.image(&tag)?;
                        extended = smart_extend(
                            context,
                            extended,
                            domain,
                            target,
                            left_image,
                            right_image,
                            span,
                        )?;
                    } else {
                        let all_defined = left_image
                            .labels
                            .iter()
                            .flat_map(|row| row.iter())
                            .all(|t| extended.is_defined_at(t));
                        if !all_defined {
                            return Err(aux::Error::new(
                                "Failed to extend map (not enough information)",
                            ));
                        }
                    }
                } else if defined_left && !defined_right {
                    return Err(aux::Error::new(format!(
                        "{} is in the domain of definition of the first map, but not the second map",
                        gen_name
                    )));
                } else if defined_right && !defined_left {
                    return Err(aux::Error::new(format!(
                        "{} is in the domain of definition of the second map, but not the first map",
                        gen_name
                    )));
                }
            }
            Ok(extended)
        }
        _ => Err(aux::Error::new("Not a well-formed assignment")),
    }
}

/// Smart extension of a map: adds a mapping from a source cell to a target diagram,
/// recursively extending for boundary cells as needed.
pub fn smart_extend(
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
    let d = dim_index(domain_diag.dim());
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

    let dim = dim_index(domain_diag.dim());

    let missing = match &cell_data {
        CellData::Zero => vec![],
        CellData::Boundary {
            boundary_in,
            boundary_out,
        } => {
            let mut missing = vec![];
            for (bd, sign) in &[
                (boundary_in, DiagramSign::Source),
                (boundary_out, DiagramSign::Target),
            ] {
                let bd_d = dim_index(bd.dim());
                if let Some(row) = bd.labels.get(bd_d) {
                    for t in row {
                        if !map.is_defined_at(t) {
                            missing.push((t.clone(), *sign));
                        }
                    }
                }
            }
            missing
        }
    };

    let mut current = map;

    for (focus, sign) in &missing {
        if current.is_defined_at(focus) {
            continue;
        }
        let dim_minus_one = dim - 1;
        let cell_data_focus = get_cell_data(context, domain, focus).ok_or_else(|| {
            aux::Error::new(format!("Cannot find cell data for boundary cell {}", focus))
        })?;

        let target_boundary = match sign {
            DiagramSign::Source => {
                Diagram::boundary(DiagramSign::Source, dim_minus_one, target_diag)?
            }
            DiagramSign::Target => {
                Diagram::boundary(DiagramSign::Target, dim_minus_one, target_diag)?
            }
        };

        let domain_boundary = match (&cell_data, sign) {
            (CellData::Boundary { boundary_in, .. }, DiagramSign::Source) => boundary_in.clone(),
            (CellData::Boundary { boundary_out, .. }, DiagramSign::Target) => boundary_out.clone(),
            _ => continue,
        };

        if domain_boundary.is_cell() {
            let sub_domain = &domain_boundary;
            current = smart_extend(
                context,
                current,
                domain,
                target,
                sub_domain,
                &target_boundary,
                span,
            )?;
        } else {
            match crate::core::diagram::isomorphism_of(
                &domain_boundary.shape,
                &target_boundary.shape,
            ) {
                Err(_) => {
                    return Err(aux::Error::new(
                        "Failed to extend map (boundary shapes don't match)",
                    ));
                }
                Ok(embedding) => {
                    let bd_d = dim_index(domain_boundary.dim());
                    let bd_labels = &domain_boundary.labels;
                    let target_labels = &target_boundary.labels;
                    let embed_map = &embedding.map;

                    let mut image_tag: Option<Tag> = None;
                    let mut consistent = true;

                    if let Some(row) = bd_labels.get(bd_d) {
                        if let Some(map_row) = embed_map.get(bd_d) {
                            for (idx, t) in row.iter().enumerate() {
                                if t == focus {
                                    if let Some(&mapped_idx) = map_row.get(idx) {
                                        if let Some(target_row) = target_labels.get(bd_d) {
                                            if let Some(mapped_t) = target_row.get(mapped_idx) {
                                                match &image_tag {
                                                    None => image_tag = Some(mapped_t.clone()),
                                                    Some(existing) => {
                                                        if existing != mapped_t {
                                                            consistent = false;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if !consistent {
                        return Err(aux::Error::new(
                            "The same generator is mapped to multiple diagrams",
                        ));
                    }

                    let mapped_tag = image_tag
                        .ok_or_else(|| aux::Error::new("Failed to extend map (no image found)"))?;

                    let gen_name = target
                        .find_generator_by_tag(&mapped_tag)
                        .ok_or_else(|| aux::Error::new("Image tag not found in target complex"))?
                        .clone();
                    let d_focus = target
                        .classifier(&gen_name)
                        .ok_or_else(|| aux::Error::new("Classifier not found for image generator"))?
                        .clone();

                    let focus_domain = match domain_boundary
                        .labels
                        .get(bd_d)
                        .and_then(|r| r.iter().position(|t| t == focus))
                    {
                        Some(_) => Diagram::cell(focus.clone(), &cell_data_focus)?,
                        None => continue,
                    };

                    current = smart_extend(
                        context,
                        current,
                        domain,
                        target,
                        &focus_domain,
                        &d_focus,
                        span,
                    )?;
                }
            }
        }
    }

    PMap::extend(current, tag, dim, cell_data, target_diag.clone())
}

// ---- Partial map naming ----

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

    let (mc_opt, m_result) = interpret_pmap_def(&context_after, scope, &domain, &dp.value);
    let mut combined = InterpResult::combine(addr_result, m_result);

    let Some(mc) = mc_opt else {
        return (None, combined);
    };

    if dp.total {
        for gen_name in domain.generator_names() {
            if let Some(entry) = domain.find_generator(&gen_name) {
                if !mc.map.is_defined_at(&entry.tag) {
                    combined.add_error(make_error(
                        dp.name.span,
                        format!(
                            "Total map `{}` is not defined on generator `{}`",
                            dp.name.inner, gen_name
                        ),
                    ));
                }
            }
        }
    }

    let name = dp.name.inner.clone();
    (Some((name, mc.map, MapDomain::Type(id))), combined)
}

// ---- Assert checking ----

pub fn check_assert(
    _context: &Context,
    _scope: &Complex,
    pair: &TermPair,
) -> Result<(), String> {
    match pair {
        TermPair::DTermPair { fst, snd } => {
            if Diagram::isomorphic(fst, snd) {
                Ok(())
            } else {
                Err("The diagrams are not equal".into())
            }
        }
        TermPair::MTermPair { fst, snd, domain } => {
            for (_, gen_name, tag) in sorted_generators(domain) {
                let in_first = fst.is_defined_at(&tag);
                let in_second = snd.is_defined_at(&tag);
                if in_first && !in_second {
                    return Err(format!(
                        "`{}` is in the domain of the first map but not the second",
                        gen_name
                    ));
                }
                if in_second && !in_first {
                    return Err(format!(
                        "`{}` is in the domain of the second map but not the first",
                        gen_name
                    ));
                }
                if in_first {
                    let img1 = fst.image(&tag).map_err(|e| e.to_string())?;
                    let img2 = snd.image(&tag).map_err(|e| e.to_string())?;
                    if !Diagram::isomorphic(img1, img2) {
                        return Err(format!("The maps differ on `{}`", gen_name));
                    }
                }
            }
            Ok(())
        }
    }
}
