use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
};
use crate::language::ast::{NameWithBoundary, Span};

use super::diagram::interpret_boundaries;
use super::types::{
    Context, InterpResult, NameKind, TypeScope, ensure_name_free, make_error,
    make_error_from_core,
};

/// A named diagram binding produced by a `let` statement: `(name, diagram)`.
pub type DiagramBinding = (LocalId, Diagram);

/// A named map binding produced by a `map` definition: `(name, partial_map, domain)`.
pub type MapBinding = (LocalId, crate::core::partial_map::PartialMap, MapDomain);


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

/// Shared implementation for inserting a binding into both the local type scope and the global
/// store's type complex.
///
/// Fails if the name is already in use or if `has_local_labels` is `true`.
fn insert_type_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    mut result: InterpResult,
    name: String,
    name_span: Span,
    value_span: Span,
    kind: NameKind,
    has_local_labels: bool,
    local_label_msg: &str,
    update_scope: impl FnOnce(&mut Complex),
    update_store: impl FnOnce(&mut Complex),
) -> (Option<Complex>, InterpResult) {
    if let Some(r) = ensure_name_free(&result.context, scope, &name, name_span, kind) {
        return (None, InterpResult::combine(result, r));
    }
    if has_local_labels {
        result.add_error(make_error(value_span, local_label_msg));
        return (None, result);
    }
    let mut updated_scope = scope.clone();
    update_scope(&mut updated_scope);
    result.context.state_mut().modify_type_complex(owner_type_id, update_store);
    (Some(updated_scope), result)
}

/// Insert a diagram binding into both the local type scope and the global store's type complex.
///
/// Fails if the name is already in use, or if the diagram contains local labels
/// (named diagrams must only reference global cells).
pub fn insert_type_diagram_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<DiagramBinding>,
) -> (Option<Complex>, InterpResult) {
    let Some((name, diagram)) = binding else { return (None, result); };
    let has_local = diagram.has_local_labels();
    let (name_sc, diag_sc) = (name.clone(), diagram.clone());
    insert_type_binding(
        owner_type_id, scope, result, name.clone(), name_span, value_span,
        NameKind::Diagram, has_local,
        "Named diagrams must contain only global cells",
        move |sc| sc.add_diagram(name_sc, diag_sc),
        move |tc| tc.add_diagram(name, diagram),
    )
}

/// Insert a map binding into both the local type scope and the global store's type complex.
///
/// Fails if the name is already in use, or if the map is valued in local cells
/// (named maps must only reference global cells).
pub fn insert_type_map_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<MapBinding>,
) -> (Option<Complex>, InterpResult) {
    let Some((name, map, domain)) = binding else { return (None, result); };
    let has_local = map.has_local_labels();
    let (name_sc, map_sc, dom_sc) = (name.clone(), map.clone(), domain.clone());
    insert_type_binding(
        owner_type_id, scope, result, name.clone(), name_span, value_span,
        NameKind::PartialMap, has_local,
        "Named maps must only be valued in global cells",
        move |sc| sc.add_map(name_sc, dom_sc, map_sc),
        move |tc| tc.add_map(name, domain, map),
    )
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

