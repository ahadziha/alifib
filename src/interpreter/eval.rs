use crate::aux::{GlobalId, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::CellData,
};
use crate::language::ast::{
    self, Block, ComplexInstr, LocalInst, NameWithBoundary, Program, Span, Spanned, TypeInst,
};
use std::sync::Arc;

use super::diagram::{check_assert, interpret_assert, interpret_let_diag};
use super::include::{
    interpret_attach_instr, interpret_include_instr, interpret_include_module_instr,
};
use super::partial_map::interpret_def_pmap;
use super::scope::{
    cell_dim, create_generator_diagram, current_module_scope, initialize_module_context,
    insert_complex_diagram_binding, insert_complex_map_binding, insert_module_diagram_binding,
    insert_module_map_binding, insert_type_diagram_binding, insert_type_map_binding,
    interpret_generator_boundaries, interpret_items, interpret_items_in_complex_scope,
    interpret_items_in_type_scope, resolve_type_scope,
};
pub use super::types::{Context, InterpResult};
use super::types::{Mode, NameKind, TypeScope, ensure_name_free, error_result, identity_map, make_error};

// ---- Main interpreter ----

/// Interpret a parsed program in the given context, returning the accumulated result.
///
/// Initialises the current module if needed, then interprets each top-level block
/// in order, threading context through all steps.
pub fn interpret_program(
    context: Context,
    program: &Program,
) -> InterpResult {
    let initialization = initialize_module_context(context);
    if initialization.has_errors() {
        return initialization;
    }

    interpret_items(&initialization.context, &program.blocks, |step_context, block| {
        interpret_block(step_context, block)
    })
}

/// Dispatch a top-level block to the appropriate handler (type block or local block).
fn interpret_block(
    context: Context,
    block: &Spanned<Block>,
) -> InterpResult {
    match &block.inner {
        Block::TypeBlock(body) => interpret_type_block(&context, body),
        Block::LocalBlock { complex, body } => interpret_local_block(context, complex, body),
    }
}

/// Interpret the instructions in a `@Type` block sequentially.
fn interpret_type_block(
    context: &Context,
    body: &[Spanned<TypeInst>],
) -> InterpResult {
    interpret_items(context, body, |step_context, instr| {
        interpret_type_inst(&step_context, instr)
    })
}

/// Interpret a single instruction inside a `@Type` block.
fn interpret_type_inst(
    context: &Context,
    instr: &Spanned<TypeInst>,
) -> InterpResult {
    match &instr.inner {
        TypeInst::Generator(generator) => interpret_type_generator(context, generator),
        TypeInst::LetDiag(ld) => {
            let Some(scope) = current_module_scope(context) else {
                return InterpResult::ok(context.clone());
            };
            let (diagram_binding, result) = interpret_let_diag(context, scope, ld);
            insert_module_diagram_binding(result, diagram_binding)
        }
        TypeInst::DefPartialMap(dp) => {
            let Some(scope) = current_module_scope(context) else {
                return InterpResult::ok(context.clone());
            };
            let (map_binding, result) = interpret_def_pmap(context, scope, dp);
            insert_module_map_binding(result, map_binding)
        }
        TypeInst::IncludeModule(include_mod) => {
            interpret_include_module_instr(context, include_mod, instr.span)
        }
    }
}

/// Interpret a generator declaration at the `@Type` level.
///
/// Only 0-dimensional (object) generators are allowed here; higher-dimensional
/// generators must appear inside a type body block.
fn interpret_type_generator(context: &Context, generator: &ast::Generator) -> InterpResult {
    let generator_name = &generator.name.inner;
    let name = generator_name.name.inner.clone();
    let name_span = generator_name.name.span;

    let Some(module_scope) = current_module_scope(context) else {
        return error_result(context, name_span, "Module not found");
    };

    if let Some(result) = ensure_name_free(context, module_scope, &name, name_span, NameKind::Generator) {
        return result;
    }

    let (boundaries, mut result) = match interpret_generator_boundaries(context, module_scope, generator_name) {
        (Some(boundaries), result) => (boundaries, result),
        (None, result) => return result,
    };

    // @Type blocks may only introduce 0-dimensional type generators (objects).
    // Higher-dimensional generators are declared inside the type complex itself
    // (i.e., inside the `{ ... }` body), not at the top-level @Type block.
    if matches!(boundaries, CellData::Boundary { .. }) {
        result.add_error(make_error(
            name_span,
            "Higher cells in @Type blocks are not supported",
        ));
        return result;
    }

    let ctx = result.context.clone();
    let (type_scope_opt, complex_result) = interpret_complex(&ctx, Mode::Global, &generator.complex);
    result = InterpResult::combine(result, complex_result);

    let Some(type_scope) = type_scope_opt else {
        return result;
    };
    let mut definition_complex = type_scope.working_complex;

    let new_id = GlobalId::fresh();
    let classifier = match create_generator_diagram(name_span, Tag::Global(new_id), &CellData::Zero)
    {
        Ok(classifier) => classifier,
        Err(error) => {
            result.add_error(error);
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
            m.add_generator(name.clone(), Tag::Global(new_id), classifier.clone());
            m.add_diagram(name, classifier);
        });
    }

    result
}

// ---- Complex resolution ----

/// Resolve and optionally extend a complex (type body), returning the resulting type scope.
///
/// A complex may be an address (lookup an existing type) or an address plus a block
/// (extend the type's complex with new generators and definitions).
pub(super) fn interpret_complex(
    context: &Context,
    mode: Mode,
    complex: &Spanned<ast::Complex>,
) -> (Option<TypeScope>, InterpResult) {
    let complex_span = complex.span;

    let Some(module_scope) = current_module_scope(context) else {
        return (None, error_result(context, complex_span, format!("Module `{}` not found", context.current_module)));
    };

    match &complex.inner {
        ast::Complex::Address(addr) => {
            let missing_msg = if addr.is_empty() {
                "Type not found:"
            } else {
                "Type not found in global record:"
            };
            resolve_type_scope(
                context,
                module_scope,
                Some(addr),
                complex_span,
                missing_msg,
            )
        }
        ast::Complex::Block { address, body } => {
            let (scope_opt, mut result) = resolve_type_scope(
                context,
                module_scope,
                address.as_ref(),
                complex_span,
                "Type not found in global record:",
            );
            let Some(scope) = scope_opt else {
                return (None, result);
            };

            let owner_type_id = scope.owner_type_id;
            let initial_scope = scope.working_complex;
            let (final_scope, block_result) =
                interpret_complex_body(&result.context, mode, initial_scope.clone(), body);
            result = InterpResult::combine(result, block_result);
            let ns = TypeScope {
                owner_type_id,
                working_complex: final_scope,
            };
            (Some(ns), result)
        }
    }
}

/// Interpret the body of a complex block, threading scope through each instruction.
fn interpret_complex_body(
    context: &Context,
    mode: Mode,
    initial_scope: Complex,
    body: &[Spanned<ComplexInstr>],
) -> (Complex, InterpResult) {
    interpret_items_in_complex_scope(context, initial_scope, body, |step_context, scope, instr| {
        interpret_complex_instr(step_context, mode, scope, instr)
    })
}

/// Interpret one instruction inside a complex body.
fn interpret_complex_instr(
    context: Context,
    mode: Mode,
    scope: Complex,
    instr: &Spanned<ComplexInstr>,
) -> (Complex, InterpResult) {
    match &instr.inner {
        ComplexInstr::NameWithBoundary(generator_name) => {
            interpret_complex_generator(context, mode, scope, generator_name, instr.span)
        }
        ComplexInstr::LetDiag(ld) => {
            let (binding, result) = interpret_let_diag(&context, &scope, ld);
            insert_complex_diagram_binding(scope, result, ld.name.span, binding)
        }
        ComplexInstr::DefPartialMap(dp) => {
            let (binding, result) = interpret_def_pmap(&context, &scope, dp);
            insert_complex_map_binding(scope, result, dp.name.span, binding)
        }
        ComplexInstr::IncludeStmt(include_stmt) => {
            let (scope_opt, result) =
                interpret_include_instr(&context, &scope, include_stmt, instr.span);
            (scope_opt.unwrap_or(scope), result)
        }
        ComplexInstr::AttachStmt(attach_stmt) => {
            let (scope_opt, result) =
                interpret_attach_instr(&context, mode, &scope, attach_stmt, instr.span);
            (scope_opt.unwrap_or(scope), result)
        }
    }
}

/// Declare a new generator inside a complex, minting a fresh global ID in `Global` mode
/// or a local tag in `Local` mode.
fn interpret_complex_generator(
    context: Context,
    mode: Mode,
    mut scope: Complex,
    generator_name: &NameWithBoundary,
    outer_span: Span,
) -> (Complex, InterpResult) {
    let name = generator_name.name.inner.clone();
    let name_span = generator_name.name.span;

    if let Some(result) = ensure_name_free(&context, &scope, &name, name_span, NameKind::Generator)
    {
        return (scope, result);
    }

    let (boundaries, mut result) = match interpret_generator_boundaries(&context, &scope, generator_name) {
        (Some(boundaries), result) => (boundaries, result),
        (None, result) => return (scope, result),
    };

    let dim = cell_dim(&boundaries);

    let (tag, new_id_opt) = match mode {
        Mode::Global => {
            let id = GlobalId::fresh();
            (Tag::Global(id), Some(id))
        }
        Mode::Local => (Tag::Local(name.clone()), None),
    };

    let bounds_span = generator_name.boundary.as_ref().map(|b| b.span).unwrap_or(outer_span);

    let classifier = match create_generator_diagram(bounds_span, tag.clone(), &boundaries) {
        Ok(classifier) => classifier,
        Err(error) => {
            result.add_error(error);
            return (scope, result);
        }
    };

    scope.add_generator(name.clone(), tag.clone(), classifier.clone());
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

/// Interpret a `@Local` block: resolve its complex, then process the body statements.
fn interpret_local_block(
    context: Context,
    complex: &Spanned<ast::Complex>,
    body: &[Spanned<LocalInst>],
) -> InterpResult {
    let (scope_opt, result) = interpret_complex(&context, Mode::Global, complex);
    let Some(scope) = scope_opt else { return result; };
    let (_, local_result) = interpret_local_body(&result.context, scope, body);
    InterpResult::combine(result, local_result)
}

/// Interpret the body of a local block, threading the type scope through each instruction.
fn interpret_local_body(
    context: &Context,
    initial_scope: TypeScope,
    body: &[Spanned<LocalInst>],
) -> (Complex, InterpResult) {
    let (final_scope, result) =
        interpret_items_in_type_scope(context, initial_scope, body, |step_context, scope, instr| {
            interpret_local_inst(step_context, scope, instr)
        });
    (final_scope.working_complex, result)
}

/// Interpret a single instruction inside a `@Local` block.
fn interpret_local_inst(
    context: &Context,
    type_scope: &TypeScope,
    instr: &Spanned<LocalInst>,
) -> (Option<Complex>, InterpResult) {
    let owner_type_id = type_scope.owner_type_id;
    let scope = &type_scope.working_complex;

    match &instr.inner {
        LocalInst::LetDiag(ld) => {
            let (binding, result) = interpret_let_diag(context, scope, ld);
            insert_type_diagram_binding(
                owner_type_id,
                scope,
                result,
                ld.name.span,
                ld.value.span,
                binding,
            )
        }
        LocalInst::DefPartialMap(dp) => {
            let (binding, result) = interpret_def_pmap(context, scope, dp);
            insert_type_map_binding(
                owner_type_id,
                scope,
                result,
                dp.name.span,
                dp.value.span,
                binding,
            )
        }
        LocalInst::AssertStmt(assert_stmt) => {
            let (term_pair_opt, mut result) = interpret_assert(context, scope, assert_stmt);
            let Some(term_pair) = term_pair_opt else {
                return (None, result);
            };
            match check_assert(&term_pair) {
                Ok(()) => (Some(scope.clone()), result),
                Err(msg) => {
                    result.add_error(make_error(instr.span, msg));
                    (None, result)
                }
            }
        }
    }
}
