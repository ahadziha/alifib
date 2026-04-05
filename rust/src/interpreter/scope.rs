use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
};
use crate::language::ast::{NameWithBoundary, Span, Spanned};
use std::sync::Arc;

use super::diagram::interpret_boundaries;
use super::pmap::interpret_address;
use super::types::{
    Context, InterpResult, NameKind, TypeScope, ensure_name_free, make_error,
    resolve_root_owner_type_id, resolve_type_complex, unknown_span,
};

pub type DiagramBinding = (LocalId, Diagram);
pub type MapBinding = (LocalId, crate::core::map::PMap, MapDomain);

pub fn current_module_scope<'a>(context: &'a Context) -> Option<&'a Complex> {
    context.state.find_module(&context.current_module)
}

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
            result.add_error(make_error(
                unknown_span(),
                format!("Failed to create root type cell: {}", error),
            ));
            return result;
        }
    };

    let root_name: LocalId = String::new();
    let mut module_complex = Complex::empty();
    module_complex.add_generator(root_name.clone(), root_diagram.clone());
    module_complex.add_diagram(root_name, root_diagram);

    {
        let state = Arc::make_mut(&mut context.state);
        state.set_type(root_id, CellData::Zero, Complex::empty());
        state.set_module(module_id, module_complex);
    }

    InterpResult::ok(context)
}

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

pub fn insert_complex_diagram_binding(
    mut scope: Complex,
    result: InterpResult,
    name_span: Span,
    binding: Option<DiagramBinding>,
) -> (Complex, InterpResult) {
    let Some((name, diagram)) = binding else {
        return (scope, result);
    };

    if let Some(name_result) = ensure_name_free(
        &result.context,
        &scope,
        &name,
        name_span,
        NameKind::Diagram,
    ) {
        return (scope, InterpResult::combine(result, name_result));
    }

    scope.add_diagram(name, diagram);
    (scope, result)
}

pub fn insert_complex_map_binding(
    mut scope: Complex,
    result: InterpResult,
    name_span: Span,
    binding: Option<MapBinding>,
) -> (Complex, InterpResult) {
    let Some((name, map, domain)) = binding else {
        return (scope, result);
    };

    if let Some(name_result) = ensure_name_free(
        &result.context,
        &scope,
        &name,
        name_span,
        NameKind::PartialMap,
    ) {
        return (scope, InterpResult::combine(result, name_result));
    }

    scope.add_map(name, domain, map);
    (scope, result)
}

pub fn insert_module_diagram_binding(
    result: InterpResult,
    binding: Option<DiagramBinding>,
) -> InterpResult {
    let Some((name, diagram)) = binding else { return result; };
    let mut result = result;
    result.context.modify_current_module(|m| m.add_diagram(name, diagram));
    result
}

pub fn insert_module_map_binding(
    result: InterpResult,
    binding: Option<MapBinding>,
) -> InterpResult {
    let Some((name, map, domain)) = binding else { return result; };
    let mut result = result;
    result.context.modify_current_module(|m| m.add_map(name, domain, map));
    result
}

pub fn insert_type_diagram_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<DiagramBinding>,
) -> (Option<Complex>, InterpResult) {
    let Some((name, diagram)) = binding else {
        return (None, result);
    };

    if let Some(name_result) = ensure_name_free(
        &result.context,
        scope,
        &name,
        name_span,
        NameKind::Diagram,
    ) {
        return (None, InterpResult::combine(result, name_result));
    }

    if diagram.has_local_labels() {
        let mut result = result;
        result.add_error(make_error(
            value_span,
            "Named diagrams must contain only global cells",
        ));
        return (None, result);
    }

    let mut updated_scope = scope.clone();
    updated_scope.add_diagram(name.clone(), diagram.clone());

    let mut result = result;
    result
        .context
        .state_mut()
        .modify_type_complex(owner_type_id, |type_scope| {
            type_scope.add_diagram(name, diagram)
        });

    (Some(updated_scope), result)
}

pub fn insert_type_map_binding(
    owner_type_id: GlobalId,
    scope: &Complex,
    result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<MapBinding>,
) -> (Option<Complex>, InterpResult) {
    let Some((name, map, domain)) = binding else {
        return (None, result);
    };

    if let Some(name_result) = ensure_name_free(
        &result.context,
        scope,
        &name,
        name_span,
        NameKind::PartialMap,
    ) {
        return (None, InterpResult::combine(result, name_result));
    }

    if map.has_local_labels() {
        let mut result = result;
        result.add_error(make_error(
            value_span,
            "Named maps must only be valued in global cells",
        ));
        return (None, result);
    }

    let mut updated_scope = scope.clone();
    updated_scope.add_map(name.clone(), domain.clone(), map.clone());

    let mut result = result;
    result
        .context
        .state_mut()
        .modify_type_complex(owner_type_id, |type_scope| {
            type_scope.add_map(name, domain, map)
        });

    (Some(updated_scope), result)
}

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

pub fn create_generator_diagram(
    span: Span,
    tag: Tag,
    boundaries: &CellData,
) -> Result<Diagram, crate::language::error::Error> {
    Diagram::cell(tag, boundaries)
        .map_err(|error| make_error(span, format!("Failed to create generator cell: {}", error)))
}

pub fn resolve_type_scope_by_id(
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
                working_complex: type_complex,
            }),
            complex_result,
        ),
    }
}

pub fn resolve_complex_owner_type_id(
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

pub fn resolve_complex_type_scope(
    context: &Context,
    module_scope: &Complex,
    address: Option<&Vec<Spanned<String>>>,
    span: Span,
    not_found_msg: &str,
) -> (Option<TypeScope>, InterpResult) {
    let (owner_type_id, owner_result) =
        resolve_complex_owner_type_id(context, module_scope, address, span);
    let Some(owner_type_id) = owner_type_id else {
        return (None, owner_result);
    };

    let (scope, scope_result) =
        resolve_type_scope_by_id(&owner_result.context, owner_type_id, span, not_found_msg);
    (scope, InterpResult::combine(owner_result, scope_result))
}
