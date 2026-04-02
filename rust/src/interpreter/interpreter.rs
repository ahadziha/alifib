#![allow(dead_code)]

use std::sync::Arc;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::aux::loader::ModuleStore;
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
};
use crate::language::ast::{self, Span, Spanned, Program, Block, TypeInst, CInstr,
                            NameWithBoundary, LocalInst};

pub use super::types::{Context, InterpResult, Mode, TypeScope, make_error, unknown_span,
                        identity_map};
use super::diagram::{interpret_boundaries, interpret_let_diag, interpret_assert};
use super::pmap::{interpret_address, interpret_def_pmap, check_assert};
use super::include::{interpret_include_module_instr, interpret_include_instr,
                     interpret_attach_instr};

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

    // Initialize module complex with anonymous owner_type_id type
    let context = {
        let empty_id = GlobalId::fresh();
        let empty_tag = Tag::Global(empty_id);
        let empty_diagram = match Diagram::cell(empty_tag, &CellData::Zero) {
            Ok(d) => d,
            Err(e) => {
                let mut r = InterpResult::ok(context);
                r.add_error(make_error(unknown_span(),
                    format!("Failed to create empty type cell: {}", e)));
                return r;
            }
        };
        let empty_name: LocalId = String::new();
        let mut module_complex = Complex::empty();
        module_complex.add_generator(empty_name.clone(), empty_diagram.clone());
        module_complex.add_diagram(empty_name, empty_diagram);
        {
            let s = Arc::make_mut(&mut context.state);
            s.set_type(empty_id, CellData::Zero, Complex::empty());
            s.set_module(module_id.clone(), module_complex);
        }
        context
    };

    let mut result = InterpResult::ok(context);
    for block in &program.blocks {
        let block_result = interpret_block(modules, result.context.clone(), block);
        result = InterpResult::combine(result, block_result);
    }
    result
}

fn interpret_block(modules: &ModuleStore, context: Context, block: &Spanned<Block>) -> InterpResult {
    match &block.inner {
        Block::TypeBlock(body) => interpret_block_type(modules, context, body),
        Block::LocalBlock { complex, body } => {
            interpret_block_complex(context, complex, body)
        }
    }
}

fn interpret_block_type(
    modules: &ModuleStore,
    context: Context,
    body: &[Spanned<TypeInst>],
) -> InterpResult {
    let mut result = InterpResult::ok(context);
    let type_result = interpret_type_block(modules, &result.context, body);
    result = InterpResult::combine(result, type_result);
    result
}

fn interpret_type_block(
    modules: &ModuleStore,
    context: &Context,
    body: &[Spanned<TypeInst>],
) -> InterpResult {
    let mut acc_result = InterpResult::ok(context.clone());

    for instr in body {
        let ctx = acc_result.context.clone();
        let instr_result = interpret_type_inst(modules, &ctx, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
    }

    acc_result
}

fn interpret_type_inst(
    modules: &ModuleStore,
    context: &Context,
    instr: &Spanned<TypeInst>,
) -> InterpResult {
    match &instr.inner {
        TypeInst::Generator(generator) => interpret_generator_type(context, generator),
        TypeInst::LetDiag(ld) => {
            let module_id = &context.current_module;
            let module_location = match context.state.find_module(module_id) {
                Some(m) => m,
                None => return InterpResult::ok(context.clone()),
            };
            let (out, result) = interpret_let_diag(context, module_location, ld);
            match out {
                None => result,
                Some((name, diagram)) => {
                    let module_id2 = result.context.current_module.clone();
                    let mut r = result;
                    r.context.state_mut().modify_module(&module_id2, |c| c.add_diagram(name, diagram));
                    r
                }
            }
        }
        TypeInst::DefPMap(dp) => {
            let module_id = &context.current_module;
            let module_location = match context.state.find_module(module_id) {
                Some(m) => m,
                None => return InterpResult::ok(context.clone()),
            };
            let (out, result) = interpret_def_pmap(context, module_location, dp);
            match out {
                None => result,
                Some((name, map, domain)) => {
                    let module_id2 = result.context.current_module.clone();
                    let mut r = result;
                    r.context.state_mut().modify_module(&module_id2, |c| c.add_map(name, domain, map));
                    r
                }
            }
        }
        TypeInst::IncludeModule(include_mod) => {
            interpret_include_module_instr(modules, context, include_mod, instr.span)
        }
    }
}

fn interpret_generator_type(
    context: &Context,
    generator: &ast::Generator,
) -> InterpResult {
    let name_with_bd = &generator.name;
    let def = &generator.complex;

    let name = name_with_bd.inner.name.inner.clone();
    let name_span = name_with_bd.inner.name.span;

    let module_id = &context.current_module;
    let module_location = match context.state.find_module(module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(name_span, "Module not found"));
            return result;
        }
        Some(m) => m,
    };

    if module_location.name_in_use(&name) {
        let mut result = InterpResult::ok(context.clone());
        result.add_error(make_error(name_span,
            format!("Generator name already in use: {}", name)));
        return result;
    }

    let (boundaries, mut result) = match &name_with_bd.inner.boundary {
        None => (CellData::Zero, InterpResult::ok(context.clone())),
        Some(bounds) => {
            let (bopt, r) = interpret_boundaries(context, module_location, bounds);
            match bopt {
                None => return r,
                Some(b) => (b, r),
            }
        }
    };

    if !matches!(boundaries, CellData::Zero) {
        result.add_error(make_error(name_span,
            "Higher cells in @Type blocks are not supported"));
        return result;
    }

    let context_after = result.context.clone();
    let (ns_opt, complex_result) = interpret_complex(&context_after, Mode::Global, def);
    result = InterpResult::combine(result, complex_result);

    let mut definition_complex = match ns_opt {
        None => return result,
        Some(ns) => ns.location,
    };

    let new_id = GlobalId::fresh();
    let tag = Tag::Global(new_id);
    let classifier = match Diagram::cell(tag, &CellData::Zero) {
        Ok(d) => d,
        Err(e) => {
            result.add_error(make_error(name_span,
                format!("Failed to create generator cell: {}", e)));
            return result;
        }
    };

    let module_id2 = result.context.current_module.clone();
    let identity = identity_map(&result.context, &definition_complex);
    definition_complex.add_map(
        name.clone(),
        MapDomain::Type(new_id),
        identity,
    );

    {
        let s = result.context.state_mut();
        s.set_type(new_id, CellData::Zero, definition_complex);
        s.modify_module(&module_id2, |m| {
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
    let module_id = &context.current_module;
    let complex_span = complex.span;

    let module_space = match context.state.find_module(module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(complex_span,
                format!("Module `{}` not found", module_id)));
            return (None, result);
        }
        Some(m) => m,
    };

    let empty_name: LocalId = String::new();

    match &complex.inner {
        ast::Complex::Address(addr) => {
            if addr.is_empty() {
                match module_space.find_generator(&empty_name) {
                    None => {
                        let mut r = InterpResult::ok(context.clone());
                        r.add_error(make_error(complex_span, "Root generator not found"));
                        (None, r)
                    }
                    Some(entry) => match &entry.tag {
                        Tag::Global(id) => {
                            let owner_type_id = *id;
                            let type_entry = match context.state.find_type(owner_type_id) {
                                None => {
                                    let mut r = InterpResult::ok(context.clone());
                                    r.add_error(make_error(complex_span,
                                        format!("Type {} not found", owner_type_id)));
                                    return (None, r);
                                }
                                Some(te) => te,
                            };
                            let location = (*type_entry.complex).clone();
                            let ns = TypeScope { owner_type_id, location };
                            (Some(ns), InterpResult::ok(context.clone()))
                        }
                        Tag::Local(_) => {
                            let mut r = InterpResult::ok(context.clone());
                            r.add_error(make_error(complex_span, "Root has local tag (unexpected)"));
                            (None, r)
                        }
                    }
                }
            } else {
                let (root_opt, root_result) = interpret_address(context, addr, complex_span);
                let mut result = root_result;
                let owner_type_id = match root_opt {
                    None => return (None, result),
                    Some(r) => r,
                };
                let type_entry = match result.context.state.find_type(owner_type_id) {
                    None => {
                        result.add_error(make_error(complex_span,
                            format!("Type {} not found in global record", owner_type_id)));
                        return (None, result);
                    }
                    Some(te) => te,
                };
                let location = (*type_entry.complex).clone();
                let ns = TypeScope { owner_type_id, location };
                (Some(ns), result)
            }
        }
        ast::Complex::Block { address, body } => {
            let (root_opt, root_result) = match address {
                None => {
                    match module_space.find_generator(&empty_name) {
                        None => {
                            let mut r = InterpResult::ok(context.clone());
                            r.add_error(make_error(complex_span, "Root generator not found"));
                            (None, r)
                        }
                        Some(entry) => match &entry.tag {
                            Tag::Global(id) => (Some(*id), InterpResult::ok(context.clone())),
                            Tag::Local(_) => {
                                let mut r = InterpResult::ok(context.clone());
                                r.add_error(make_error(complex_span, "Root has local tag (unexpected)"));
                                (None, r)
                            }
                        }
                    }
                }
                Some(addr) => interpret_address(context, addr, complex_span),
            };

            let mut result = root_result;
            let owner_type_id = match root_opt {
                None => return (None, result),
                Some(r) => r,
            };

            let type_entry = match result.context.state.find_type(owner_type_id) {
                None => {
                    result.add_error(make_error(complex_span,
                        format!("Type {} not found in global record", owner_type_id)));
                    return (None, result);
                }
                Some(te) => te.clone(),
            };

            let initial_location = (*type_entry.complex).clone();

            let (location_opt, block_result) = interpret_c_block(
                &result.context, mode, &initial_location, body
            );
            result = InterpResult::combine(result, block_result);
            let location = location_opt.unwrap_or(initial_location);
            let ns = TypeScope { owner_type_id, location };
            (Some(ns), result)
        }
    }
}

fn interpret_c_block(
    context: &Context,
    mode: Mode,
    initial_location: &Complex,
    body: &[Spanned<CInstr>],
) -> (Option<Complex>, InterpResult) {
    let mut current_location: Complex = initial_location.clone();
    let mut current_context: Context = context.clone();
    let mut acc_errors: Vec<crate::language::error::Error> = Vec::new();
    let mut acc_holes: Vec<super::types::HoleInfo> = Vec::new();

    for instr in body {
        let (new_location, instr_result) =
            interpret_c_instr(current_context, mode, current_location, instr);
        current_location = new_location;
        current_context = instr_result.context;
        acc_errors.extend(instr_result.errors);
        acc_holes.extend(instr_result.holes);
    }

    let acc_result = InterpResult { context: current_context, errors: acc_errors, holes: acc_holes };
    (Some(current_location), acc_result)
}

fn interpret_c_instr(
    context: Context,
    mode: Mode,
    mut location: Complex,
    instr: &Spanned<CInstr>,
) -> (Complex, InterpResult) {
    match &instr.inner {
        CInstr::NameWithBoundary(nwb) => {
            interpret_generator_instr(context, mode, location, nwb, instr.span)
        }
        CInstr::LetDiag(ld) => {
            let (out, result) = interpret_let_diag(&context, &location, ld);
            match out {
                None => (location, result),
                Some((name, diagram)) => {
                    if location.name_in_use(&name) {
                        let mut r = result;
                        r.add_error(make_error(ld.name.span,
                            format!("Diagram name already in use: {}", name)));
                        return (location, r);
                    }
                    location.add_diagram(name, diagram);
                    (location, result)
                }
            }
        }
        CInstr::DefPMap(dp) => {
            let (out, result) = interpret_def_pmap(&context, &location, dp);
            match out {
                None => (location, result),
                Some((name, map, domain)) => {
                    if location.name_in_use(&name) {
                        let mut r = result;
                        r.add_error(make_error(dp.name.span,
                            format!("Partial map name already in use: {}", name)));
                        return (location, r);
                    }
                    location.add_map(name, domain, map);
                    (location, result)
                }
            }
        }
        CInstr::IncludeStmt(include_stmt) => {
            let (loc_opt, result) = interpret_include_instr(&context, mode, &location, include_stmt, instr.span);
            (loc_opt.unwrap_or(location), result)
        }
        CInstr::AttachStmt(attach_stmt) => {
            let (loc_opt, result) = interpret_attach_instr(&context, mode, &location, attach_stmt, instr.span);
            (loc_opt.unwrap_or(location), result)
        }
    }
}

fn interpret_generator_instr(
    context: Context,
    mode: Mode,
    mut location: Complex,
    nwb: &NameWithBoundary,
    outer_span: Span,
) -> (Complex, InterpResult) {
    let name = nwb.name.inner.clone();
    let name_span = nwb.name.span;

    if location.name_in_use(&name) {
        let mut result = InterpResult::ok(context);
        result.add_error(make_error(name_span,
            format!("Generator name already in use: {}", name)));
        return (location, result);
    }

    let (boundaries, mut result) = match &nwb.boundary {
        None => (CellData::Zero, InterpResult::ok(context)),
        Some(bounds) => {
            let (bopt, r) = interpret_boundaries(&context, &location, bounds);
            drop(context);
            match bopt {
                None => return (location, r),
                Some(b) => (b, r),
            }
        }
    };

    let dim = match &boundaries {
        CellData::Zero => 0,
        CellData::Boundary { boundary_in, .. } => {
            if boundary_in.dim() < 0 { 1 } else { (boundary_in.dim() as usize) + 1 }
        }
    };

    let (tag, new_id_opt) = match mode {
        Mode::Global => {
            let id = GlobalId::fresh();
            (Tag::Global(id), Some(id))
        }
        Mode::Local => (Tag::Local(name.clone()), None),
    };

    let bounds_span = nwb.boundary.as_ref()
        .map(|b| b.span)
        .unwrap_or(outer_span);

    let classifier = match Diagram::cell(tag.clone(), &boundaries) {
        Ok(d) => d,
        Err(e) => {
            result.add_error(make_error(bounds_span,
                format!("Failed to create generator cell: {}", e)));
            return (location, result);
        }
    };

    location.add_generator(name.clone(), classifier.clone());
    location.add_diagram(name.clone(), classifier.clone());

    if mode == Mode::Local {
        location.add_local_cell(name.clone(), dim, boundaries.clone());
    }

    if let (Mode::Global, Some(id)) = (mode, new_id_opt) {
        Arc::make_mut(&mut result.context.state).set_cell(id, dim, boundaries);
    }

    (location, result)
}

// ---- Local blocks ----

fn interpret_block_complex(
    context: Context,
    complex: &Spanned<ast::Complex>,
    body: &[Spanned<LocalInst>],
) -> InterpResult {
    let (ns_opt, complex_result) = interpret_complex(&context, Mode::Global, complex);
    let mut result = complex_result;

    let namespace = match ns_opt {
        None => return result,
        Some(ns) => ns,
    };

    if !body.is_empty() {
        let (_, local_result) = interpret_local_block(
            &result.context,
            &namespace,
            body,
        );
        result = InterpResult::combine(result, local_result);
    }

    result
}

fn interpret_local_block(
    context: &Context,
    namespace: &TypeScope,
    body: &[Spanned<LocalInst>],
) -> (Option<Complex>, InterpResult) {
    let mut current_ns = namespace.clone();
    let mut acc_result = InterpResult::ok(context.clone());

    for instr in body {
        let ctx = acc_result.context.clone();
        let (loc_opt, instr_result) = interpret_local_inst(&ctx, &current_ns, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
        if let Some(new_loc) = loc_opt {
            current_ns = TypeScope { owner_type_id: current_ns.owner_type_id, location: new_loc };
        }
        if acc_result.has_errors() {
            break;
        }
    }

    (Some(current_ns.location), acc_result)
}

fn interpret_local_inst(
    context: &Context,
    namespace: &TypeScope,
    instr: &Spanned<LocalInst>,
) -> (Option<Complex>, InterpResult) {
    let owner_type_id = namespace.owner_type_id;
    let location = &namespace.location;

    match &instr.inner {
        LocalInst::LetDiag(ld) => {
            let (out, result) = interpret_let_diag(context, location, ld);
            match out {
                None => (None, result),
                Some((name, diagram)) => {
                    if location.name_in_use(&name) {
                        let mut r = result;
                        r.add_error(make_error(ld.name.span,
                            format!("Diagram name already in use: {}", name)));
                        return (None, r);
                    }
                    if diagram.has_local_labels() {
                        let mut r = result;
                        r.add_error(make_error(ld.value.span,
                            "Named diagrams must contain only global cells"));
                        return (None, r);
                    }
                    let mut new_location = location.clone();
                    new_location.add_diagram(name.clone(), diagram.clone());
                    let mut r = result;
                    r.context.state_mut().modify_type_complex(owner_type_id, |c| c.add_diagram(name, diagram));
                    (Some(new_location), r)
                }
            }
        }
        LocalInst::DefPMap(dp) => {
            let (out, result) = interpret_def_pmap(context, location, dp);
            match out {
                None => (None, result),
                Some((name, map, domain)) => {
                    if location.name_in_use(&name) {
                        let mut r = result;
                        r.add_error(make_error(dp.name.span,
                            format!("Partial map name already in use: {}", name)));
                        return (None, r);
                    }
                    if map.has_local_labels() {
                        let mut r = result;
                        r.add_error(make_error(dp.value.span,
                            "Named maps must only be valued in global cells"));
                        return (None, r);
                    }
                    let mut new_location = location.clone();
                    new_location.add_map(name.clone(), domain.clone(), map.clone());
                    let mut r = result;
                    r.context.state_mut().modify_type_complex(owner_type_id, |c| c.add_map(name, domain, map));
                    (Some(new_location), r)
                }
            }
        }
        LocalInst::AssertStmt(assert_stmt) => {
            let (term_pair_opt, assert_result) = interpret_assert(context, location, assert_stmt);
            let span = instr.span;
            match term_pair_opt {
                None => (None, assert_result),
                Some(term_pair) => {
                    let check_result = check_assert(&assert_result.context, location, &term_pair);
                    match check_result {
                        Ok(_) => (Some(location.clone()), assert_result),
                        Err(msg) => {
                            let mut r = assert_result;
                            r.add_error(make_error(span, msg));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}
