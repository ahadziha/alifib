use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
};
use crate::language::ast::{Address, NameWithBoundary, Span, Spanned};
use std::sync::Arc;

use super::diagram::interpret_boundaries;
use super::types::{
    Context, InterpResult, NameKind, Step, TypeScope, ensure_name_free, make_error,
    make_error_from_core, resolve_root_owner_type_id, resolve_type_complex,
};

/// A named diagram binding produced by a `let` statement: `(name, diagram)`.
pub type DiagramBinding = (LocalId, Diagram);

/// A named map binding produced by a `map` definition: `(name, partial_map, domain)`.
pub type MapBinding = (LocalId, crate::core::partial_map::PartialMap, MapDomain);

/// Look up the current module's complex in the global store.
pub fn current_module_scope(context: &Context) -> Option<&Complex> {
    context.state.find_module(&context.current_module)
}

/// Ensure the current module exists in the store, creating it with a fresh root generator if absent.
pub fn initialize_module_context(mut context: Context) -> InterpResult {
    let module_id = context.current_module.clone();
    if context.state.find_module(&module_id).is_some() {
        return InterpResult::ok(context);
    }

    let root_id = GlobalId::fresh();
    let root_diagram = match Diagram::cell(Tag::Global(root_id), &CellData::Zero) {
        Ok(root_diagram) => root_diagram,
        Err(error) => {
            let mut result = InterpResult::ok(context);
            result.add_error(make_error_from_core(Span::synthetic(), error));
            return result;
        }
    };

    let root_name: LocalId = String::new();
    let mut module_complex = Complex::empty();
    module_complex.add_generator(root_name.clone(), Tag::Global(root_id), root_diagram.clone());
    module_complex.add_diagram(root_name, root_diagram);

    {
        let state = Arc::make_mut(&mut context.state);
        state.set_type(root_id, CellData::Zero, Complex::empty());
        state.set_module(module_id, module_complex);
    }

    InterpResult::ok(context)
}

/// Interpret a sequence of items, threading context through each step.
pub fn interpret_items<T>(
    context: &Context,
    items: &[T],
    mut step: impl FnMut(Context, &T) -> InterpResult,
) -> InterpResult {
    let mut result = InterpResult::ok(context.clone());

    for item in items {
        let step_result = step(result.context.clone(), item);
        result = InterpResult::combine(result, step_result);
    }

    result
}

/// Interpret a sequence of items, threading both context and a mutable complex scope.
pub fn interpret_items_in_complex_scope<T>(
    context: &Context,
    mut scope: Complex,
    items: &[T],
    mut step: impl FnMut(Context, Complex, &T) -> (Complex, InterpResult),
) -> (Complex, InterpResult) {
    let mut result = InterpResult::ok(context.clone());

    for item in items {
        let (next_scope, step_result) = step(result.context.clone(), scope, item);
        scope = next_scope;
        result = InterpResult::combine(result, step_result);
    }

    (scope, result)
}

/// Interpret a sequence of items inside a type scope, threading scope through each step.
///
/// Unlike [`interpret_items`] and [`interpret_items_in_complex_scope`], this function
/// **breaks early on the first error**. Later instructions depend on the scope being in
/// a consistent state, which an earlier error may have invalidated; continuing past it
/// risks misleading cascading errors.
pub fn interpret_items_in_type_scope<T>(
    context: &Context,
    mut scope: TypeScope,
    items: &[T],
    mut step: impl FnMut(&Context, &TypeScope, &T) -> (Option<Complex>, InterpResult),
) -> (TypeScope, InterpResult) {
    let mut result = InterpResult::ok(context.clone());

    for item in items {
        let (next_complex, step_result) = step(&result.context, &scope, item);
        result = InterpResult::combine(result, step_result);
        if let Some(working_complex) = next_complex {
            scope = TypeScope {
                owner_type_id: scope.owner_type_id,
                working_complex,
            };
        }
        if result.has_errors() {
            break;
        }
    }

    (scope, result)
}

/// Insert a named binding into a complex scope after verifying the name is free.
fn insert_complex_binding(
    mut scope: Complex,
    result: InterpResult,
    name: String,
    name_span: Span,
    kind: NameKind,
    update: impl FnOnce(&mut Complex),
) -> (Complex, InterpResult) {
    if let Some(name_result) = ensure_name_free(&result.context, &scope, &name, name_span, kind) {
        return (scope, InterpResult::combine(result, name_result));
    }
    update(&mut scope);
    (scope, result)
}

/// Insert a diagram binding into a complex scope.
pub fn insert_complex_diagram_binding(
    scope: Complex,
    result: InterpResult,
    name_span: Span,
    binding: Option<DiagramBinding>,
) -> (Complex, InterpResult) {
    let Some((name, diagram)) = binding else { return (scope, result); };
    insert_complex_binding(scope, result, name.clone(), name_span, NameKind::Diagram, move |sc| {
        sc.add_diagram(name, diagram)
    })
}

/// Insert a map binding into a complex scope.
pub fn insert_complex_map_binding(
    scope: Complex,
    result: InterpResult,
    name_span: Span,
    binding: Option<MapBinding>,
) -> (Complex, InterpResult) {
    let Some((name, map, domain)) = binding else { return (scope, result); };
    insert_complex_binding(scope, result, name.clone(), name_span, NameKind::PartialMap, move |sc| {
        sc.add_map(name, domain, map)
    })
}

/// Insert a diagram binding into the current module's complex.
pub fn insert_module_diagram_binding(
    result: InterpResult,
    binding: Option<DiagramBinding>,
) -> InterpResult {
    let Some((name, diagram)) = binding else { return result; };
    let mut result = result;
    result.context.modify_current_module(|m| m.add_diagram(name, diagram));
    result
}

/// Insert a map binding into the current module's complex.
pub fn insert_module_map_binding(
    result: InterpResult,
    binding: Option<MapBinding>,
) -> InterpResult {
    let Some((name, map, domain)) = binding else { return result; };
    let mut result = result;
    result.context.modify_current_module(|m| m.add_map(name, domain, map));
    result
}

/// Insert a diagram binding into both the local type scope and the global store's type complex.
///
/// Fails if the name is already in use, or if the diagram contains local labels
/// (named diagrams must only reference global cells).
pub fn insert_type_diagram_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    mut result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<DiagramBinding>,
) -> (Option<Complex>, InterpResult) {
    let Some((name, diagram)) = binding else { return (None, result); };
    if let Some(r) = ensure_name_free(&result.context, scope, &name, name_span, NameKind::Diagram) {
        return (None, InterpResult::combine(result, r));
    }
    if diagram.has_local_labels() {
        result.add_error(make_error(value_span, "Named diagrams must contain only global cells"));
        return (None, result);
    }
    let mut updated_scope = scope.clone();
    updated_scope.add_diagram(name.clone(), diagram.clone());
    result.context.state_mut().modify_type_complex(owner_type_id, |tc| tc.add_diagram(name, diagram));
    (Some(updated_scope), result)
}

/// Insert a map binding into both the local type scope and the global store's type complex.
///
/// Fails if the name is already in use, or if the map is valued in local cells
/// (named maps must only reference global cells).
pub fn insert_type_map_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    mut result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<MapBinding>,
) -> (Option<Complex>, InterpResult) {
    let Some((name, map, domain)) = binding else { return (None, result); };
    if let Some(r) = ensure_name_free(&result.context, scope, &name, name_span, NameKind::PartialMap) {
        return (None, InterpResult::combine(result, r));
    }
    if map.has_local_labels() {
        result.add_error(make_error(value_span, "Named maps must only be valued in global cells"));
        return (None, result);
    }
    let mut updated_scope = scope.clone();
    updated_scope.add_map(name.clone(), domain.clone(), map.clone());
    result.context.state_mut().modify_type_complex(owner_type_id, |tc| tc.add_map(name, domain, map));
    (Some(updated_scope), result)
}

/// Interpret the optional boundary annotation of a generator declaration.
///
/// Returns `CellData::Zero` if no boundary is specified (a 0-dimensional generator).
pub fn interpret_generator_boundaries(
    context: &Context,
    scope: &Complex,
    generator_name: &NameWithBoundary,
) -> (Option<CellData>, InterpResult) {
    match &generator_name.boundary {
        None => (Some(CellData::Zero), InterpResult::ok(context.clone())),
        Some(boundaries) => interpret_boundaries(context, scope, boundaries),
    }
}

/// Returns the dimension of a cell with the given boundary data.
pub fn cell_dim(cell_data: &CellData) -> usize {
    match cell_data {
        CellData::Zero => 0,
        CellData::Boundary { boundary_in, .. } => boundary_in.top_dim() + 1,
    }
}

/// Create a cell diagram for a generator, wrapping core errors as language errors at `span`.
pub fn create_generator_diagram(
    span: Span,
    tag: Tag,
    boundaries: &CellData,
) -> Result<Diagram, crate::language::error::Error> {
    Diagram::cell(tag, boundaries)
        .map_err(|error| make_error_from_core(span, error))
}

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
    (scope, InterpResult::combine(owner_result, scope_result))
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
        return (id_opt, InterpResult::combine(module_result, root_result));
    }

    let last_idx = segments.len() - 1;
    let prefix = &segments[..last_idx];
    let (last_span, last_name) = &segments[last_idx];

    let (target_scope, prefix_result) = resolve_address_prefix_scope(context, module_scope, prefix);
    let Some(target_scope) = target_scope else {
        return (None, InterpResult::combine(module_result, prefix_result));
    };

    let (id_opt, id_result) =
        type_id_of_named_diagram(&target_scope, last_name, *last_span, context);
    (
        id_opt,
        InterpResult::combine(InterpResult::combine(module_result, prefix_result), id_result),
    )
}
