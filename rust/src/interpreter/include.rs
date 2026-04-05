use std::sync::Arc;

use crate::aux::loader::ModuleStore;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
    map::PMap,
};
use crate::language::ast::{self, IncludeModule, Span};

use super::global_store::GlobalStore;
use super::interpreter::interpret_program;
use super::pmap::{interpret_address, interpret_pmap_def};
use super::types::*;

type ImportedGenerator = (LocalId, Tag, Diagram);

fn ensure_partial_map_name_available(
    context: &Context,
    scope: &Complex,
    name: &str,
    span: Span,
) -> Option<InterpResult> {
    ensure_name_free(context, scope, name, span, NameKind::PartialMap)
}

fn prefixed_generators(
    source_scope: &Complex,
    prefix: &str,
    skip_empty_name: bool,
) -> Vec<ImportedGenerator> {
    source_scope
        .generator_names()
        .into_iter()
        .filter(|name| !skip_empty_name || !name.is_empty())
        .filter_map(|generator_name| {
            let generator_entry = source_scope.find_generator(&generator_name)?;
            let classifier = source_scope.classifier(&generator_name)?.clone();
            let qualified_name = qualify_name(prefix, &generator_name);
            Some((qualified_name, generator_entry.tag.clone(), classifier))
        })
        .collect()
}

fn insert_generators_by_tag(scope: &mut Complex, generators: impl IntoIterator<Item = ImportedGenerator>) {
    for (qualified_name, tag, classifier) in generators {
        if scope.find_generator_by_tag(&tag).is_some() {
            continue;
        }
        scope.add_generator(qualified_name, classifier);
    }
}

fn resolve_attach_type_id(
    domain: &MapDomain,
    context: &Context,
) -> Result<GlobalId, InterpResult> {
    match domain {
        MapDomain::Type(id) => Ok(*id),
        MapDomain::Module(_) => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(
                unknown_span(),
                "Unexpected module domain in attach",
            ));
            Err(result)
        }
    }
}

fn mapped_cell_data(map: &PMap, source_cell_data: &CellData) -> Option<CellData> {
    match source_cell_data {
        CellData::Zero => Some(CellData::Zero),
        CellData::Boundary {
            boundary_in,
            boundary_out,
        } => {
            let image_in = PMap::apply(map, boundary_in).ok()?;
            let image_out = PMap::apply(map, boundary_out).ok()?;
            Some(CellData::Boundary {
                boundary_in: Arc::new(image_in),
                boundary_out: Arc::new(image_out),
            })
        }
    }
}

fn extend_scope_with_attached_generators(
    mode: Mode,
    mut scope: Complex,
    mut state: Arc<GlobalStore>,
    mut map: PMap,
    prefix: &str,
    attachment_scope: &Complex,
) -> (Complex, Arc<GlobalStore>, PMap) {
    for (generator_dim, generator_name, generator_tag) in sorted_generators(attachment_scope) {
        if map.is_defined_at(&generator_tag) {
            continue;
        }

        let source_cell_data = match generator_tag {
            Tag::Global(global_id) => match state.find_cell(global_id) {
                Some(cell_entry) => cell_entry.data.clone(),
                None => continue,
            },
            Tag::Local(_) => continue,
        };

        let image_cell_data = match mapped_cell_data(&map, &source_cell_data) {
            Some(image_cell_data) => image_cell_data,
            None => continue,
        };

        let qualified_name = qualify_name(prefix, &generator_name);
        let image_tag = match mode {
            Mode::Global => {
                let image_id = GlobalId::fresh();
                Arc::make_mut(&mut state).set_cell(image_id, generator_dim, image_cell_data.clone());
                Tag::Global(image_id)
            }
            Mode::Local => Tag::Local(qualified_name.clone()),
        };

        let image_classifier = match Diagram::cell(image_tag, &image_cell_data) {
            Ok(image_classifier) => image_classifier,
            Err(_) => continue,
        };

        if mode == Mode::Local {
            scope.add_local_cell(qualified_name.clone(), generator_dim, image_cell_data.clone());
        }
        scope.add_generator(qualified_name, image_classifier.clone());
        map.insert_raw(generator_tag, generator_dim, source_cell_data, image_classifier);
    }

    (scope, state, map)
}

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

        if let Some(result) = ensure_partial_map_name_available(context, scope, &alias, span) {
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

    let include_context = Context::new_sharing_state(canonical_path.clone(), context);
    let include_result = interpret_program(modules, include_context, &resolved.program);

    let mut result = InterpResult::ok(context.clone());
    result.errors.extend(include_result.errors.clone());

    if include_result.has_errors() {
        return result;
    }

    result.context.state = Arc::clone(&include_result.context.state);

    let included_arc = match result.context.state.find_module_arc(&canonical_path) {
        Some(arc) => arc,
        None => {
            result.add_error(make_error(span, "Included module complex not found"));
            return result;
        }
    };

    let imported_generators = prefixed_generators(&included_arc, &alias, true);
    let inclusion = identity_map(&include_result.context, &included_arc);

    result
        .context
        .state_mut()
        .modify_module(&module_id, |current| {
            insert_generators_by_tag(current, imported_generators);
            current.add_map(alias, MapDomain::Module(canonical_path), inclusion);
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
    let (include_out, include_result) = resolve_include(context, include_stmt, span);
    let context_after = include_result.context.clone();

    let Some((id, name)) = include_out else {
        return (None, include_result);
    };

    if let Some(r) = ensure_partial_map_name_available(&include_result.context, scope, &name, span) {
        return (None, InterpResult::combine(include_result, r));
    }

    let (subtype_opt, subtype_result) =
        resolve_type_complex(&context_after, id, span, "Type not found in global record");
    let Some(subtype) = subtype_opt else {
        return (None, InterpResult::combine(include_result, subtype_result));
    };

    let mut new_scope = scope.clone();
    insert_generators_by_tag(&mut new_scope, prefixed_generators(&subtype, &name, false));
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
    let (attach_out, attach_result) = resolve_attach(context, scope, attach_stmt, span);
    let context_after = attach_result.context.clone();

    let Some((name, map, domain)) = attach_out else {
        return (None, attach_result);
    };

    if let Some(r) = ensure_partial_map_name_available(
        &attach_result.context,
        scope,
        &name,
        attach_stmt.name.span,
    ) {
        return (None, InterpResult::combine(attach_result, r));
    }

    let attachment_id = match resolve_attach_type_id(&domain, &attach_result.context) {
        Ok(attachment_id) => attachment_id,
        Err(result) => return (None, result),
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

    let (mut current_scope, current_state, current_map) = extend_scope_with_attached_generators(
        mode,
        scope.clone(),
        Arc::clone(&context_after.state),
        map.clone(),
        &name,
        &attachment,
    );
    current_scope.add_map(name, domain, current_map);
    let mut r = attach_result;
    r.context.state = current_state;
    (Some(current_scope), r)
}

fn resolve_include(
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

fn resolve_attach(
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
            let map = PMap::empty();
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
