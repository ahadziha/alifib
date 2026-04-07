use std::sync::Arc;

use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
    partial_map::PartialMap,
};
use crate::language::ast::{self, IncludeModule, Span};

use super::partial_map::interpret_pmap_def;
use super::resolve::{interpret_address, resolve_type_complex};
use super::types::{
    Context, InterpResult, Mode, NameKind, ensure_name_free, error_result, identity_map,
    make_error, qualify_name, sorted_generators,
};

/// A generator to import: `(qualified_name, tag, classifier_diagram)`.
type ImportedGenerator = (LocalId, Tag, Diagram);

/// Collect generators from a complex, prefixing each name with `prefix`.
///
/// If `skip_empty_name` is `true`, the unnamed root generator is excluded.
fn prefixed_generators(
    source_scope: &Complex,
    prefix: &str,
    skip_empty_name: bool,
) -> Vec<ImportedGenerator> {
    source_scope
        .generators_iter()
        .filter(|(name, _, _)| !skip_empty_name || !name.is_empty())
        .filter_map(|(name, tag, _)| {
            let classifier = source_scope.classifier(name)?.clone();
            let qualified_name = qualify_name(prefix, name);
            Some((qualified_name, tag.clone(), classifier))
        })
        .collect()
}

/// Insert generators into a complex, skipping any whose tag is already present.
fn insert_generators_by_tag(scope: &mut Complex, generators: impl IntoIterator<Item = ImportedGenerator>) {
    for (qualified_name, tag, classifier) in generators {
        if scope.find_generator_by_tag(&tag).is_some() {
            continue;
        }
        scope.add_generator(qualified_name, tag, classifier);
    }
}

/// Apply a partial map to cell boundary data, returning `None` if the map is incomplete.
fn mapped_cell_data(map: &PartialMap, source_cell_data: &CellData) -> Option<CellData> {
    match source_cell_data {
        CellData::Zero => Some(CellData::Zero),
        CellData::Boundary {
            boundary_in,
            boundary_out,
        } => {
            let image_in = PartialMap::apply(map, boundary_in).ok()?;
            let image_out = PartialMap::apply(map, boundary_out).ok()?;
            Some(CellData::Boundary {
                boundary_in: Arc::new(image_in),
                boundary_out: Arc::new(image_out),
            })
        }
    }
}

/// Add unmapped generators from `attachment_scope` into `scope`.
///
/// In `Global` mode, each new generator gets a fresh global ID registered in the store.
/// In `Local` mode, generators are added as local cells.
/// Returns the updated scope and extended map.
fn extend_scope_with_attached_generators(
    mode: Mode,
    mut scope: Complex,
    context: &mut Context,
    mut map: PartialMap,
    prefix: &str,
    attachment_scope: &Complex,
) -> (Complex, PartialMap) {
    for (generator_dim, generator_name, generator_tag) in sorted_generators(attachment_scope) {
        if map.is_defined_at(&generator_tag) {
            continue;
        }

        let Tag::Global(global_id) = generator_tag else { continue; };
        let Some(cell_entry) = context.state.find_cell(global_id) else { continue; };
        let source_cell_data = cell_entry.data.clone();

        let Some(image_cell_data) = mapped_cell_data(&map, &source_cell_data) else { continue; };

        let qualified_name = qualify_name(prefix, &generator_name);
        let image_tag = match mode {
            Mode::Global => {
                let image_id = crate::aux::GlobalId::fresh();
                context.state_mut().set_cell(image_id, generator_dim, image_cell_data.clone());
                Tag::Global(image_id)
            }
            Mode::Local => {
                scope.add_local_cell(qualified_name.clone(), generator_dim, image_cell_data.clone());
                Tag::Local(qualified_name.clone())
            }
        };

        let Ok(image_classifier) = Diagram::cell(image_tag.clone(), &image_cell_data) else { continue; };

        scope.add_generator(qualified_name, image_tag, image_classifier.clone());
        map.insert_raw(Tag::Global(global_id), generator_dim, source_cell_data, image_classifier);
    }

    (scope, map)
}

/// Interpret an `include module` instruction at the top level.
///
/// Imports generators from a pre-interpreted module (looked up by resolved path)
/// and registers an identity inclusion map under the given alias.
pub fn interpret_include_module_instr(
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

    let Some(scope) = context.state.find_module(&module_id) else {
        return error_result(context, span, "Module not found");
    };
    if let Some(result) = ensure_name_free(context, scope, &alias, span, NameKind::PartialMap) {
        return result;
    }

    let Some(canonical_path) = context.resolutions.resolve(&module_id, &module_name).map(|p| p.to_owned()) else {
        return error_result(context, span, format!("Module file {}.ali not found in search paths", module_name));
    };

    // The module was pre-interpreted in topological order before this program
    // started, so its Complex is already in the global store.
    let Some(included_arc) = context.state.find_module_arc(&canonical_path) else {
        return error_result(context, span, format!("Module {} was not pre-interpreted (internal error)", canonical_path));
    };

    let imported_generators = prefixed_generators(&included_arc, &alias, true);
    let inclusion = identity_map(context, &included_arc);

    let mut result = InterpResult::ok(context.clone());
    result.context.modify_current_module(|current| {
        insert_generators_by_tag(current, imported_generators);
        current.add_map(alias, MapDomain::Module(canonical_path), inclusion);
    });

    result
}

/// Interpret an `include` instruction inside a complex body.
///
/// Looks up the included type, imports its generators with the given alias as a prefix,
/// and registers an identity inclusion map.
pub fn interpret_include_instr(
    context: &Context,
    scope: &Complex,
    include_stmt: &ast::IncludeStmt,
    span: Span,
) -> (Option<Complex>, InterpResult) {
    let (include_out, include_result) = resolve_include(context, include_stmt, span);
    let context_after = include_result.context.clone();

    let Some((id, name)) = include_out else {
        return (None, include_result);
    };

    if let Some(r) = ensure_name_free(&include_result.context, scope, &name, span, NameKind::PartialMap) {
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

/// Interpret an `attach` instruction inside a complex body.
///
/// Attaches a type along an optional partial map, extending the scope with freshly
/// created image generators for any unmapped generators in the attachment type.
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

    if let Some(r) = ensure_name_free(&attach_result.context, scope, &name, attach_stmt.name.span, NameKind::PartialMap) {
        return (None, InterpResult::combine(attach_result, r));
    }

    let attachment_id = match domain {
        MapDomain::Type(id) => id,
        MapDomain::Module(_) => return (None, error_result(&attach_result.context, Span::synthetic(), "Unexpected module domain in attach")),
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

    let mut r = attach_result;
    let (mut current_scope, current_map) = extend_scope_with_attached_generators(
        mode,
        scope.clone(),
        &mut r.context,
        map,
        &name,
        &attachment,
    );
    current_scope.add_map(name, domain, current_map);
    (Some(current_scope), r)
}

/// Resolve the address and alias for an include statement.
///
/// Returns the `(GlobalId, alias_name)` pair, or `None` on error.
fn resolve_include(
    context: &Context,
    include_stmt: &ast::IncludeStmt,
    span: Span,
) -> (Option<(GlobalId, LocalId)>, InterpResult) {
    let (id_opt, mut addr_result) = interpret_address(
        context,
        &include_stmt.address.inner,
        include_stmt.address.span,
    );
    let Some(id) = id_opt else { return (None, addr_result); };

    let name = if let Some(alias_node) = &include_stmt.alias {
        alias_node.inner.clone()
    } else {
        let tag = Tag::Global(id);
        let gen_name = context.state
            .find_module(&context.current_module)
            .and_then(|m| m.find_generator_by_tag(&tag));
        match gen_name {
            Some(n) if n.contains('.') => {
                addr_result.add_error(make_error(span, "Inclusion of non-local types requires an alias"));
                return (None, addr_result);
            }
            Some(n) => n.clone(),
            None => {
                addr_result.add_error(make_error(span, "Could not infer include alias"));
                return (None, addr_result);
            }
        }
    };

    (Some((id, name)), addr_result)
}

/// Resolve the address, name, and optional map for an attach statement.
///
/// Returns `(name, partial_map, domain)`, or `None` on error.
fn resolve_attach(
    context: &Context,
    scope: &Complex,
    attach_stmt: &ast::AttachStmt,
    span: Span,
) -> (Option<(LocalId, PartialMap, MapDomain)>, InterpResult) {
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
            let map = PartialMap::empty();
            (Some((name, map, MapDomain::Type(id))), addr_result)
        }
        Some(pmap_node) => {
            let (domain_opt, domain_result) =
                resolve_type_complex(&context_after, id, span, "Type not found");
            let Some(domain) = domain_opt else {
                return (None, InterpResult::combine(addr_result, domain_result));
            };
            let (eval_map_opt, pmap_result) =
                interpret_pmap_def(&context_after, scope, &domain, pmap_node);
            let combined = InterpResult::combine(addr_result, pmap_result);
            let Some(eval_map) = eval_map_opt else {
                return (None, combined);
            };
            (Some((name, eval_map.map, MapDomain::Type(id))), combined)
        }
    }
}
