#![allow(dead_code)]

use crate::aux::loader::ModuleStore;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
};
use crate::language::ast::{
    self, Block, CInstr, LocalInst, NameWithBoundary, Program, Span, Spanned, TypeInst,
};
use std::sync::Arc;

use super::diagram::{interpret_assert, interpret_boundaries, interpret_let_diag};
use super::include::{
    interpret_attach_instr, interpret_include_instr, interpret_include_module_instr,
};
use super::pmap::{check_assert, interpret_address, interpret_def_pmap};
pub use super::types::{
    Context, InterpResult, Mode, NameKind, TypeScope, ensure_name_free, identity_map, make_error,
    resolve_root_owner_type_id, resolve_type_complex, unknown_span,
};

// ---- Semantic helpers ----

fn resolve_current_module<'a>(context: &'a Context) -> Option<&'a Complex> {
    context.state.find_module(&context.current_module)
}

fn combine_steps<T>(
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

fn combine_scope_steps<T>(
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

fn combine_local_scope_steps<T>(
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

fn apply_module_diagram(context: &mut Context, name: LocalId, diagram: Diagram) {
    let module_id = context.current_module.clone();
    context
        .state_mut()
        .modify_module(&module_id, |c| c.add_diagram(name, diagram));
}

fn apply_module_map(
    context: &mut Context,
    name: LocalId,
    domain: MapDomain,
    map: crate::core::map::PMap,
) {
    let module_id = context.current_module.clone();
    context
        .state_mut()
        .modify_module(&module_id, |c| c.add_map(name, domain, map));
}

fn bind_scope_diagram(
    mut scope: Complex,
    result: InterpResult,
    name_span: Span,
    binding: Option<(LocalId, Diagram)>,
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

fn bind_scope_map(
    mut scope: Complex,
    result: InterpResult,
    name_span: Span,
    binding: Option<(LocalId, crate::core::map::PMap, MapDomain)>,
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

fn bind_module_diagram(result: InterpResult, binding: Option<(LocalId, Diagram)>) -> InterpResult {
    let Some((name, diagram)) = binding else {
        return result;
    };

    let mut result = result;
    apply_module_diagram(&mut result.context, name, diagram);
    result
}

fn bind_module_map(
    result: InterpResult,
    binding: Option<(LocalId, crate::core::map::PMap, MapDomain)>,
) -> InterpResult {
    let Some((name, map, domain)) = binding else {
        return result;
    };

    let mut result = result;
    apply_module_map(&mut result.context, name, domain, map);
    result
}

fn bind_type_diagram(
    owner_type_id: GlobalId,
    scope: &Complex,
    result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<(LocalId, Diagram)>,
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
        .modify_type_complex(owner_type_id, |c| c.add_diagram(name, diagram));

    (Some(updated_scope), result)
}

fn bind_type_map(
    owner_type_id: GlobalId,
    scope: &Complex,
    result: InterpResult,
    name_span: Span,
    value_span: Span,
    binding: Option<(LocalId, crate::core::map::PMap, MapDomain)>,
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
        .modify_type_complex(owner_type_id, |c| c.add_map(name, domain, map));

    (Some(updated_scope), result)
}

fn resolve_type_scope_by_id(
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
// ---- Main interpreter ----

pub fn interpret_program(
    modules: &ModuleStore,
    mut context: Context,
    program: &Program,
) -> InterpResult {
    let module_id = context.current_module.clone();

    // If the module is already loaded, skip
    if context.state.find_module(&module_id).is_some() {
        return InterpResult::ok(context);
    }

    // Initialize module complex with a root (anonymous empty-named) type
    let context = {
        let root_id = GlobalId::fresh();
        let root_tag = Tag::Global(root_id);
        let root_diagram = match Diagram::cell(root_tag, &CellData::Zero) {
            Ok(d) => d,
            Err(e) => {
                let mut r = InterpResult::ok(context);
                r.add_error(make_error(
                    unknown_span(),
                    format!("Failed to create root type cell: {}", e),
                ));
                return r;
            }
        };
        let root_name: LocalId = String::new();
        let mut module_complex = Complex::empty();
        module_complex.add_generator(root_name.clone(), root_diagram.clone());
        module_complex.add_diagram(root_name, root_diagram);
        {
            let s = Arc::make_mut(&mut context.state);
            s.set_type(root_id, CellData::Zero, Complex::empty());
            s.set_module(module_id.clone(), module_complex);
        }
        context
    };

    combine_steps(&context, &program.blocks, |step_context, block| {
        interpret_block(modules, step_context, block)
    })
}

fn interpret_block(
    modules: &ModuleStore,
    context: Context,
    block: &Spanned<Block>,
) -> InterpResult {
    match &block.inner {
        Block::TypeBlock(body) => interpret_type_block(modules, &context, body),
        Block::LocalBlock { complex, body } => interpret_block_complex(context, complex, body),
    }
}

fn interpret_type_block(
    modules: &ModuleStore,
    context: &Context,
    body: &[Spanned<TypeInst>],
) -> InterpResult {
    combine_steps(context, body, |step_context, instr| {
        interpret_type_inst(modules, &step_context, instr)
    })
}

fn interpret_type_inst(
    modules: &ModuleStore,
    context: &Context,
    instr: &Spanned<TypeInst>,
) -> InterpResult {
    match &instr.inner {
        TypeInst::Generator(generator) => interpret_generator_type(context, generator),
        TypeInst::LetDiag(ld) => {
            let module_scope = match resolve_current_module(context) {
                Some(module_scope) => module_scope,
                None => return InterpResult::ok(context.clone()),
            };
            let (diagram_binding, result) = interpret_let_diag(context, module_scope, ld);
            bind_module_diagram(result, diagram_binding)
        }
        TypeInst::DefPMap(dp) => {
            let module_scope = match resolve_current_module(context) {
                Some(module_scope) => module_scope,
                None => return InterpResult::ok(context.clone()),
            };
            let (map_binding, result) = interpret_def_pmap(context, module_scope, dp);
            bind_module_map(result, map_binding)
        }
        TypeInst::IncludeModule(include_mod) => {
            interpret_include_module_instr(modules, context, include_mod, instr.span)
        }
    }
}

fn interpret_generator_type(context: &Context, generator: &ast::Generator) -> InterpResult {
    let nwb = &generator.name.inner;
    let name = nwb.name.inner.clone();
    let name_span = nwb.name.span;

    let module_scope = match resolve_current_module(context) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(name_span, "Module not found"));
            return result;
        }
        Some(module_scope) => module_scope,
    };

    if let Some(result) =
        ensure_name_free(context, module_scope, &name, name_span, NameKind::Generator)
    {
        return result;
    }

    let (boundaries, mut result) = match &nwb.boundary {
        None => (CellData::Zero, InterpResult::ok(context.clone())),
        Some(bounds) => {
            let (boundary_data, boundary_result) =
                interpret_boundaries(context, module_scope, bounds);
            let Some(cell_data) = boundary_data else {
                return boundary_result;
            };
            (cell_data, boundary_result)
        }
    };

    if matches!(boundaries, CellData::Boundary { .. }) {
        result.add_error(make_error(
            name_span,
            "Higher cells in @Type blocks are not supported",
        ));
        return result;
    }

    let ctx = result.context.clone();
    let (type_scope, complex_result) = interpret_complex(&ctx, Mode::Global, &generator.complex);
    result = InterpResult::combine(result, complex_result);

    let Some(type_scope) = type_scope else {
        return result;
    };
    let mut definition_complex = type_scope.working_complex;

    let new_id = GlobalId::fresh();
    let tag = Tag::Global(new_id);
    let classifier = match Diagram::cell(tag, &CellData::Zero) {
        Ok(classifier) => classifier,
        Err(error) => {
            result.add_error(make_error(
                name_span,
                format!("Failed to create generator cell: {}", error),
            ));
            return result;
        }
    };

    let module_id = result.context.current_module.clone();
    let identity = identity_map(&result.context, &definition_complex);
    definition_complex.add_map(name.clone(), MapDomain::Type(new_id), identity);

    {
        let s = result.context.state_mut();
        s.set_type(new_id, CellData::Zero, definition_complex);
        s.modify_module(&module_id, |m| {
            m.add_generator(name.clone(), classifier.clone());
            m.add_diagram(name, classifier);
        });
    }

    result
}

// ---- Complex resolution ----

pub(super) fn interpret_complex(
    context: &Context,
    mode: Mode,
    complex: &Spanned<ast::Complex>,
) -> (Option<TypeScope>, InterpResult) {
    let complex_span = complex.span;

    let module_space = match resolve_current_module(context) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(
                complex_span,
                format!("Module `{}` not found", context.current_module),
            ));
            return (None, result);
        }
        Some(m) => m,
    };

    match &complex.inner {
        ast::Complex::Address(addr) => {
            if addr.is_empty() {
                let (id_opt, root_result) =
                    resolve_root_owner_type_id(context, module_space, complex_span);
                let owner_type_id = match id_opt {
                    None => return (None, root_result),
                    Some(id) => id,
                };
                let (scope_opt, scope_result) = resolve_type_scope_by_id(
                    context,
                    owner_type_id,
                    complex_span,
                    "Type not found:",
                );
                (scope_opt, InterpResult::combine(root_result, scope_result))
            } else {
                let (id_opt, addr_result) = interpret_address(context, addr, complex_span);
                let owner_type_id = match id_opt {
                    None => return (None, addr_result),
                    Some(id) => id,
                };
                let (scope_opt, scope_result) = resolve_type_scope_by_id(
                    &addr_result.context,
                    owner_type_id,
                    complex_span,
                    "Type not found in global record:",
                );
                (scope_opt, InterpResult::combine(addr_result, scope_result))
            }
        }
        ast::Complex::Block { address, body } => {
            let (id_opt, id_result) = match address {
                None => resolve_root_owner_type_id(context, module_space, complex_span),
                Some(addr) => interpret_address(context, addr, complex_span),
            };

            let mut result = id_result;
            let owner_type_id = match id_opt {
                None => return (None, result),
                Some(id) => id,
            };

            let (scope_opt, scope_result) = resolve_type_scope_by_id(
                &result.context,
                owner_type_id,
                complex_span,
                "Type not found in global record:",
            );
            result = InterpResult::combine(result, scope_result);
            let Some(scope) = scope_opt else {
                return (None, result);
            };

            let initial_scope = scope.working_complex;
            let (final_scope, block_result) =
                interpret_c_block(&result.context, mode, initial_scope.clone(), body);
            result = InterpResult::combine(result, block_result);
            let ns = TypeScope {
                owner_type_id,
                working_complex: final_scope,
            };
            (Some(ns), result)
        }
    }
}

fn interpret_c_block(
    context: &Context,
    mode: Mode,
    initial_scope: Complex,
    body: &[Spanned<CInstr>],
) -> (Complex, InterpResult) {
    combine_scope_steps(context, initial_scope, body, |step_context, scope, instr| {
        interpret_c_instr(step_context, mode, scope, instr)
    })
}

fn interpret_c_instr(
    context: Context,
    mode: Mode,
    scope: Complex,
    instr: &Spanned<CInstr>,
) -> (Complex, InterpResult) {
    match &instr.inner {
        CInstr::NameWithBoundary(nwb) => {
            interpret_generator_instr(context, mode, scope, nwb, instr.span)
        }
        CInstr::LetDiag(ld) => {
            let (binding, result) = interpret_let_diag(&context, &scope, ld);
            bind_scope_diagram(scope, result, ld.name.span, binding)
        }
        CInstr::DefPMap(dp) => {
            let (binding, result) = interpret_def_pmap(&context, &scope, dp);
            bind_scope_map(scope, result, dp.name.span, binding)
        }
        CInstr::IncludeStmt(include_stmt) => {
            let (scope_opt, result) =
                interpret_include_instr(&context, mode, &scope, include_stmt, instr.span);
            (scope_opt.unwrap_or(scope), result)
        }
        CInstr::AttachStmt(attach_stmt) => {
            let (scope_opt, result) =
                interpret_attach_instr(&context, mode, &scope, attach_stmt, instr.span);
            (scope_opt.unwrap_or(scope), result)
        }
    }
}

fn interpret_generator_instr(
    context: Context,
    mode: Mode,
    mut scope: Complex,
    nwb: &NameWithBoundary,
    outer_span: Span,
) -> (Complex, InterpResult) {
    let name = nwb.name.inner.clone();
    let name_span = nwb.name.span;

    if let Some(result) = ensure_name_free(&context, &scope, &name, name_span, NameKind::Generator)
    {
        return (scope, result);
    }

    let (boundaries, mut result) = match &nwb.boundary {
        None => (CellData::Zero, InterpResult::ok(context)),
        Some(bounds) => {
            let (bopt, r) = interpret_boundaries(&context, &scope, bounds);
            drop(context);
            match bopt {
                None => return (scope, r),
                Some(b) => (b, r),
            }
        }
    };

    let dim = match &boundaries {
        CellData::Zero => 0,
        CellData::Boundary { boundary_in, .. } => {
            if boundary_in.dim() < 0 {
                1
            } else {
                (boundary_in.dim() as usize) + 1
            }
        }
    };

    let (tag, new_id_opt) = match mode {
        Mode::Global => {
            let id = GlobalId::fresh();
            (Tag::Global(id), Some(id))
        }
        Mode::Local => (Tag::Local(name.clone()), None),
    };

    let bounds_span = nwb.boundary.as_ref().map(|b| b.span).unwrap_or(outer_span);

    let classifier = match Diagram::cell(tag.clone(), &boundaries) {
        Ok(d) => d,
        Err(e) => {
            result.add_error(make_error(
                bounds_span,
                format!("Failed to create generator cell: {}", e),
            ));
            return (scope, result);
        }
    };

    scope.add_generator(name.clone(), classifier.clone());
    scope.add_diagram(name.clone(), classifier.clone());

    if mode == Mode::Local {
        scope.add_local_cell(name.clone(), dim, boundaries.clone());
    }

    if let Some(id) = new_id_opt {
        Arc::make_mut(&mut result.context.state).set_cell(id, dim, boundaries);
    }

    (scope, result)
}

// ---- Local blocks ----

fn interpret_block_complex(
    context: Context,
    complex: &Spanned<ast::Complex>,
    body: &[Spanned<LocalInst>],
) -> InterpResult {
    let (scope_opt, complex_result) = interpret_complex(&context, Mode::Global, complex);
    let mut result = complex_result;

    let Some(scope) = scope_opt else {
        return result;
    };

    if !body.is_empty() {
        let (_, local_result) = interpret_local_block(&result.context, scope, body);
        result = InterpResult::combine(result, local_result);
    }

    result
}

fn interpret_local_block(
    context: &Context,
    initial_scope: TypeScope,
    body: &[Spanned<LocalInst>],
) -> (Complex, InterpResult) {
    let (final_scope, result) =
        combine_local_scope_steps(context, initial_scope, body, |step_context, scope, instr| {
            interpret_local_inst(step_context, scope, instr)
        });
    (final_scope.working_complex, result)
}

fn interpret_local_inst(
    context: &Context,
    namespace: &TypeScope,
    instr: &Spanned<LocalInst>,
) -> (Option<Complex>, InterpResult) {
    let owner_type_id = namespace.owner_type_id;
    let scope = &namespace.working_complex;

    match &instr.inner {
        LocalInst::LetDiag(ld) => {
            let (binding, result) = interpret_let_diag(context, scope, ld);
            bind_type_diagram(
                owner_type_id,
                scope,
                result,
                ld.name.span,
                ld.value.span,
                binding,
            )
        }
        LocalInst::DefPMap(dp) => {
            let (binding, result) = interpret_def_pmap(context, scope, dp);
            bind_type_map(
                owner_type_id,
                scope,
                result,
                dp.name.span,
                dp.value.span,
                binding,
            )
        }
        LocalInst::AssertStmt(assert_stmt) => {
            let (term_pair_opt, assert_result) = interpret_assert(context, scope, assert_stmt);
            let span = instr.span;
            match term_pair_opt {
                None => (None, assert_result),
                Some(term_pair) => match check_assert(&assert_result.context, scope, &term_pair) {
                    Ok(()) => (Some(scope.clone()), assert_result),
                    Err(msg) => {
                        let mut r = assert_result;
                        r.add_error(make_error(span, msg));
                        (None, r)
                    }
                },
            }
        }
    }
}
