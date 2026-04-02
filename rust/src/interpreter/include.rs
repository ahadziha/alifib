use std::sync::Arc;

use crate::aux::loader::ModuleStore;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
    map::PMap,
};
use crate::language::ast::{self, IncludeModule, Span};

use super::interpreter::interpret_program;
use super::pmap::{interpret_address, interpret_pmap_def};
use super::types::*;

pub fn interpret_include_module_instr(
    modules: &ModuleStore,
    context: &Context,
    include_mod: &IncludeModule,
    span: Span,
) -> InterpResult {
    let module_name: LocalId = include_mod.name.inner.clone();
    let alias: LocalId = include_mod
        .alias
        .as_ref()
        .map(|a| a.inner.clone())
        .unwrap_or_else(|| module_name.clone());

    let module_id = context.current_module.clone();
    {
        let scope = match context.state.find_module(&module_id) {
            None => {
                let mut result = InterpResult::ok(context.clone());
                result.add_error(make_error(span, "Module not found"));
                return result;
            }
            Some(m) => m,
        };

        if let Some(result) = ensure_name_free(context, scope, &alias, span, NameKind::PartialMap) {
            return result;
        }
    }

    let canonical_path = match modules.resolve(&module_id, &module_name) {
        Some(p) => p.to_owned(),
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(
                span,
                format!("Module file {}.ali not found in search paths", module_name),
            ));
            return result;
        }
    };

    let resolved = match modules.get(&canonical_path) {
        Some(r) => r,
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(
                span,
                format!("Resolved module {} not found in store", canonical_path),
            ));
            return result;
        }
    };

    let included_module_id = canonical_path.clone();
    let include_context = Context::new_sharing_state(included_module_id.clone(), context);
    let include_result = interpret_program(modules, include_context, &resolved.program);

    let mut result = InterpResult::ok(context.clone());
    result.errors.extend(include_result.errors.clone());

    if include_result.has_errors() {
        return result;
    }

    result.context.state = Arc::clone(&include_result.context.state);

    let included_arc = match result.context.state.find_module_arc(&included_module_id) {
        Some(arc) => arc,
        None => {
            result.add_error(make_error(span, "Included module complex not found"));
            return result;
        }
    };

    let gen_data: Vec<_> = included_arc
        .generator_names()
        .into_iter()
        .filter(|n| !n.is_empty())
        .filter_map(|gen_name| {
            let gen_entry = included_arc.find_generator(&gen_name)?;
            let classifier = included_arc.classifier(&gen_name)?.clone();
            let tag = gen_entry.tag.clone();
            let combined_name = qualify_name(&alias, &gen_name);
            Some((combined_name, tag, classifier))
        })
        .collect();

    let inclusion = identity_map(&include_result.context, &included_arc);

    result
        .context
        .state_mut()
        .modify_module(&module_id, |current| {
            for (combined_name, tag, classifier) in gen_data {
                if current.find_generator_by_tag(&tag).is_some() {
                    continue;
                }
                current.add_generator(combined_name, classifier);
            }
            current.add_map(alias, MapDomain::Module(included_module_id), inclusion);
        });

    result
}

pub fn interpret_include_instr(
    context: &Context,
    _mode: Mode,
    scope: &Complex,
    include_stmt: &ast::IncludeStmt,
    span: Span,
) -> (Option<Complex>, InterpResult) {
    let (include_out, include_result) = interpret_include(context, include_stmt, span);
    let context_after = include_result.context.clone();

    let Some((id, name)) = include_out else {
        return (None, include_result);
    };

    if let Some(r) = ensure_name_free(
        &include_result.context,
        scope,
        &name,
        span,
        NameKind::PartialMap,
    ) {
        return (None, InterpResult::combine(include_result, r));
    }

    let (subtype_opt, subtype_result) =
        resolve_type_complex(&context_after, id, span, "Type not found in global record");
    let Some(subtype) = subtype_opt else {
        return (None, InterpResult::combine(include_result, subtype_result));
    };

    let mut new_scope = scope.clone();
    for gen_name in subtype.generator_names() {
        if let Some(gen_entry) = subtype.find_generator(&gen_name) {
            if new_scope.find_generator_by_tag(&gen_entry.tag).is_some() {
                continue;
            }
            let classifier = match subtype.classifier(&gen_name) {
                Some(d) => d.clone(),
                None => continue,
            };
            let combined = qualify_name(&name, &gen_name);
            new_scope.add_generator(combined, classifier);
        }
    }

    let inclusion = identity_map(&context_after, &subtype);
    new_scope.add_map(name, MapDomain::Type(id), inclusion);

    (Some(new_scope), include_result)
}

pub fn interpret_attach_instr(
    context: &Context,
    mode: Mode,
    scope: &Complex,
    attach_stmt: &ast::AttachStmt,
    span: Span,
) -> (Option<Complex>, InterpResult) {
    let (attach_out, attach_result) = interpret_attach(context, scope, attach_stmt, span);
    let context_after = attach_result.context.clone();

    let Some((name, map, domain)) = attach_out else {
        return (None, attach_result);
    };

    if let Some(r) = ensure_name_free(
        &attach_result.context,
        scope,
        &name,
        attach_stmt.name.span,
        NameKind::PartialMap,
    ) {
        return (None, InterpResult::combine(attach_result, r));
    }

    let attachment_id = match &domain {
        MapDomain::Type(id) => *id,
        MapDomain::Module(_) => {
            let mut r = attach_result;
            r.add_error(make_error(
                unknown_span(),
                "Unexpected module domain in attach",
            ));
            return (None, r);
        }
    };

    let (attachment_opt, attachment_result) = resolve_type_complex(
        &context_after,
        attachment_id,
        attach_stmt.name.span,
        "Type not found in global record",
    );
    let Some(attachment) = attachment_opt else {
        return (None, InterpResult::combine(attach_result, attachment_result));
    };

    let generators = sorted_generators(&attachment);

    let mut current_scope = scope.clone();
    let mut current_state = Arc::clone(&context_after.state);
    let mut current_map = map.clone();

    for (gen_dim, gen_name, gen_tag) in &generators {
        if current_map.is_defined_at(gen_tag) {
            continue;
        }

        let gen_cell_data = match gen_tag {
            Tag::Global(gid) => match current_state.find_cell(*gid) {
                Some(ce) => ce.data.clone(),
                None => continue,
            },
            Tag::Local(_) => continue,
        };

        let image_cell_data = match &gen_cell_data {
            CellData::Zero => CellData::Zero,
            CellData::Boundary {
                boundary_in,
                boundary_out,
            } => {
                let image_in = match PMap::apply(&current_map, boundary_in) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let image_out = match PMap::apply(&current_map, boundary_out) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                CellData::Boundary {
                    boundary_in: Arc::new(image_in),
                    boundary_out: Arc::new(image_out),
                }
            }
        };

        let combined = qualify_name(&name, gen_name);

        let image_tag = match mode {
            Mode::Global => {
                let image_id = GlobalId::fresh();
                Arc::make_mut(&mut current_state).set_cell(
                    image_id,
                    *gen_dim,
                    image_cell_data.clone(),
                );
                Tag::Global(image_id)
            }
            Mode::Local => Tag::Local(combined.clone()),
        };

        let image_classifier = match Diagram::cell(image_tag.clone(), &image_cell_data) {
            Ok(d) => d,
            Err(_) => continue,
        };

        match mode {
            Mode::Global => {
                current_scope.add_generator(combined.clone(), image_classifier.clone())
            }
            Mode::Local => {
                current_scope.add_local_cell(
                    combined.clone(),
                    *gen_dim,
                    image_cell_data.clone(),
                );
                current_scope.add_generator(combined.clone(), image_classifier.clone());
            }
        };

        current_map.insert_raw(gen_tag.clone(), *gen_dim, gen_cell_data, image_classifier);
    }

    current_scope.add_map(name, domain, current_map);
    let mut r = attach_result;
    r.context.state = current_state;
    (Some(current_scope), r)
}

fn interpret_include(
    context: &Context,
    include_stmt: &ast::IncludeStmt,
    span: Span,
) -> (Option<(GlobalId, LocalId)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(
        context,
        &include_stmt.address.inner,
        include_stmt.address.span,
    );
    match id_opt {
        None => (None, addr_result),
        Some(id) => {
            let name = match &include_stmt.alias {
                Some(alias_node) => alias_node.inner.clone(),
                None => {
                    let module_id = &context.current_module;
                    let tag = Tag::Global(id);
                    match context
                        .state
                        .find_module(module_id)
                        .and_then(|m| m.find_generator_by_tag(&tag))
                    {
                        Some(gen_name) => {
                            if gen_name.contains('.') {
                                let mut r = addr_result;
                                r.add_error(make_error(
                                    span,
                                    "Inclusion of non-local types requires an alias",
                                ));
                                return (None, r);
                            }
                            gen_name.clone()
                        }
                        None => {
                            let mut r = addr_result;
                            r.add_error(make_error(span, "Could not infer include alias"));
                            return (None, r);
                        }
                    }
                }
            };
            (Some((id, name)), addr_result)
        }
    }
}

fn interpret_attach(
    context: &Context,
    scope: &Complex,
    attach_stmt: &ast::AttachStmt,
    span: Span,
) -> (Option<(LocalId, PMap, MapDomain)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(
        context,
        &attach_stmt.address.inner,
        attach_stmt.address.span,
    );
    let context_after = addr_result.context.clone();

    let Some(id) = id_opt else {
        return (None, addr_result);
    };

    let name = attach_stmt.name.inner.clone();

    match &attach_stmt.along {
        None => {
            let map = PMap::empty().unwrap();
            (Some((name, map, MapDomain::Type(id))), addr_result)
        }
        Some(pmap_node) => {
            let (domain_opt, domain_result) =
                resolve_type_complex(&context_after, id, span, "Type not found");
            let Some(domain) = domain_opt else {
                return (None, InterpResult::combine(addr_result, domain_result));
            };
            let (mc_opt, pmap_result) =
                interpret_pmap_def(&context_after, scope, &domain, pmap_node);
            let combined = InterpResult::combine(addr_result, pmap_result);
            let Some(mc) = mc_opt else {
                return (None, combined);
            };
            (Some((name, mc.map, MapDomain::Type(id))), combined)
        }
    }
}
