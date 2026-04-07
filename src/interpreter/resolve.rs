use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::complex::{Complex, MapDomain};
use crate::language::ast::{Address, Span, Spanned};
use std::sync::Arc;

use super::types::{Context, InterpResult, Step, TypeScope, make_error};

// ---- Type/complex resolution ----

/// Find the global ID of the root (unnamed) generator in a module scope.
pub fn resolve_root_owner_type_id(
    context: &Context,
    module_space: &Complex,
    span: Span,
) -> Step<GlobalId> {
    let empty_name: LocalId = String::new();
    let mut result = InterpResult::ok(context.clone());

    let Some((root_tag, _)) = module_space.find_generator(&empty_name) else {
        result.add_error(make_error(span, "Root generator not found"));
        return (None, result);
    };

    match root_tag {
        Tag::Global(id) => (Some(*id), result),
        Tag::Local(_) => {
            result.add_error(make_error(span, "Root has local tag (unexpected)"));
            (None, result)
        }
    }
}

/// Look up the definition complex for a type by its global ID.
///
/// `missing_prefix` is prepended to the ID in the error message when the type is not found.
pub fn resolve_type_complex(
    context: &Context,
    type_id: GlobalId,
    span: Span,
    missing_prefix: &str,
) -> Step<Arc<Complex>> {
    let mut result = InterpResult::ok(context.clone());
    let Some(type_entry) = context.state.find_type(type_id) else {
        result.add_error(make_error(span, format!("{} {}", missing_prefix, type_id)));
        return (None, result);
    };
    (Some(Arc::clone(&type_entry.complex)), result)
}

/// Resolve a map domain (type or module) to its defining complex.
pub fn resolve_map_domain_complex(
    context: &Context,
    domain: &MapDomain,
    span: Span,
) -> Step<Arc<Complex>> {
    let mut result = InterpResult::ok(context.clone());
    match domain {
        MapDomain::Type(id) => {
            let Some(type_entry) = context.state.find_type(*id) else {
                result.add_error(make_error(span, format!("Type {} not found", id)));
                return (None, result);
            };
            (Some(Arc::clone(&type_entry.complex)), result)
        }
        MapDomain::Module(mid) => {
            let Some(module_arc) = context.state.find_module_arc(mid) else {
                result.add_error(make_error(span, format!("Module `{}` not found", mid)));
                return (None, result);
            };
            (Some(module_arc), result)
        }
    }
}

// ---- Type scope resolution ----

/// Open a type scope by looking up the type's complex in the global store.
pub fn open_type_scope(
    context: &Context,
    owner_type_id: GlobalId,
    span: Span,
    not_found_msg: &str,
) -> (Option<TypeScope>, InterpResult) {
    let (owner_complex, complex_result) =
        resolve_type_complex(context, owner_type_id, span, not_found_msg);
    match owner_complex {
        None => (None, complex_result),
        Some(type_complex) => (
            Some(TypeScope {
                owner_type_id,
                working_complex: (*type_complex).clone(),
            }),
            complex_result,
        ),
    }
}

/// Resolve the owner type ID from an optional dotted-path address.
///
/// If `address` is `None` or empty, falls back to the root generator of the module scope.
pub fn resolve_owner_type_id(
    context: &Context,
    module_scope: &Complex,
    address: Option<&Vec<Spanned<String>>>,
    span: Span,
) -> (Option<GlobalId>, InterpResult) {
    match address {
        Some(address) if !address.is_empty() => interpret_address(context, address, span),
        _ => resolve_root_owner_type_id(context, module_scope, span),
    }
}

/// Resolve an address to a type scope: resolve the owner ID, then open its complex.
pub fn resolve_type_scope(
    context: &Context,
    module_scope: &Complex,
    address: Option<&Vec<Spanned<String>>>,
    span: Span,
    not_found_msg: &str,
) -> (Option<TypeScope>, InterpResult) {
    let (owner_type_id, owner_result) =
        resolve_owner_type_id(context, module_scope, address, span);
    let Some(owner_type_id) = owner_type_id else {
        return (None, owner_result);
    };

    let (scope, scope_result) =
        open_type_scope(&owner_result.context, owner_type_id, span, not_found_msg);
    (scope, owner_result.merge(scope_result))
}

// ---- Address resolution ----

/// Get an `Arc` reference to the current module's complex, failing if it is not in the store.
fn current_module_arc(context: &Context, span: Span) -> Step<Arc<Complex>> {
    let module_id = &context.current_module;
    let mut result = InterpResult::ok(context.clone());

    let Some(module_arc) = context.state.find_module_arc(module_id) else {
        result.add_error(make_error(span, format!("Module `{}` not found", module_id)));
        return (None, result);
    };

    (Some(module_arc), result)
}

/// Walk a dotted address prefix, resolving each segment through module maps.
///
/// Each segment must name a map whose domain is a module; the scope advances into that module.
fn resolve_address_prefix_scope(
    context: &Context,
    initial_scope: Arc<Complex>,
    prefix: &[(Span, String)],
) -> Step<Arc<Complex>> {
    let mut current_scope = initial_scope;
    let mut result = InterpResult::ok(context.clone());

    for (segment_span, segment_name) in prefix {
        let Some((_, domain)) = current_scope.find_map(segment_name) else {
            result.add_error(make_error(
                *segment_span,
                format!("Partial map `{}` not found", segment_name),
            ));
            return (None, result);
        };

        match domain {
            MapDomain::Module(module_id) => match context.state.find_module_arc(module_id) {
                Some(module_arc) => current_scope = module_arc,
                None => {
                    result.add_error(make_error(
                        *segment_span,
                        format!("Module `{}` not found", module_id),
                    ));
                    return (None, result);
                }
            },
            MapDomain::Type(_) => {
                result.add_error(make_error(
                    *segment_span,
                    format!("Domain of `{}` is not a module", segment_name),
                ));
                return (None, result);
            }
        }
    }

    (Some(current_scope), result)
}

/// Look up the global type ID for a named diagram in a complex scope.
///
/// The diagram must be a cell with a global tag.
fn type_id_of_named_diagram(
    scope: &Complex,
    name: &str,
    name_span: Span,
    context: &Context,
) -> Step<GlobalId> {
    let mut result = InterpResult::ok(context.clone());

    let Some(diagram) = scope.find_diagram(name) else {
        result.add_error(make_error(name_span, format!("Type `{}` not found", name)));
        return (None, result);
    };

    if !diagram.is_cell() {
        result.add_error(make_error(name_span, format!("`{}` is not a cell", name)));
        return (None, result);
    }

    match diagram.top_label() {
        None => {
            result.add_error(make_error(name_span, "Cell has no top label"));
            (None, result)
        }
        Some(Tag::Global(id)) => (Some(*id), result),
        Some(Tag::Local(_)) => {
            result.add_error(make_error(name_span, "Cell has local tag (unexpected)"));
            (None, result)
        }
    }
}

/// Resolve a dotted-path address in the current module scope to a global type ID.
pub fn interpret_address(context: &Context, address: &Address, addr_span: Span) -> Step<GlobalId> {
    let (module_scope, module_result) = current_module_arc(context, addr_span);
    let Some(module_scope) = module_scope else {
        return (None, module_result);
    };
    let segments: Vec<(Span, String)> = address.iter().map(|n| (n.span, n.inner.clone())).collect();

    if segments.is_empty() {
        let (id_opt, root_result) = resolve_root_owner_type_id(context, &module_scope, addr_span);
        return (id_opt, module_result.merge(root_result));
    }

    let last_idx = segments.len() - 1;
    let prefix = &segments[..last_idx];
    let (last_span, last_name) = &segments[last_idx];

    let (target_scope, prefix_result) = resolve_address_prefix_scope(context, module_scope, prefix);
    let Some(target_scope) = target_scope else {
        return (None, module_result.merge(prefix_result));
    };

    let (id_opt, id_result) =
        type_id_of_named_diagram(&target_scope, last_name, *last_span, context);
    (id_opt, module_result.merge(prefix_result).merge(id_result))
}
