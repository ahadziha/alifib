use std::sync::Arc;
use crate::aux::{GlobalId, LocalId, Tag};
use crate::aux::path;
use crate::aux::loader::{FileLoader, LoadError};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram},
    map::PMap,
};
use crate::language::{
    self,
    ast::{self, Span, IncludeModule},
};
use super::types::*;
use super::interpreter::interpret_program;
use super::pmap::{interpret_address, interpret_pmap};

pub fn interpret_include_module_instr(
    loader: &FileLoader,
    context: &Context,
    include_mod: &IncludeModule,
    span: Span,
) -> (Option<Complex>, InterpResult) {
    let module_name: LocalId = include_mod.name.inner.clone();
    let alias: LocalId = include_mod.alias.as_ref()
        .map(|a| a.inner.clone())
        .unwrap_or_else(|| module_name.clone());

    let module_id = context.current_module.clone();
    let location = match context.state.find_module(&module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(span, "Module not found"));
            return (None, result);
        }
        Some(m) => m.clone(),
    };

    if location.name_in_use(&alias) {
        let mut result = InterpResult::ok(context.clone());
        result.add_error(make_error(span, format!("Partial map name already in use: {}", alias)));
        return (None, result);
    }

    // Search for the module file
    let filename = format!("{}.ali", module_name);
    let find_file = |loader: &FileLoader| -> Result<(String, String), String> {
        for dir in &loader.search_paths {
            let candidate = format!("{}/{}", dir, filename);
            let canonical = path::canonicalize(&candidate);
            match (loader.read_file)(&canonical) {
                Ok(contents) => return Ok((canonical, contents)),
                Err(LoadError::NotFound) => continue,
                Err(LoadError::IoError(reason)) => {
                    return Err(format!("Failed to load {}: {}", canonical, reason));
                }
            }
        }
        Err(format!("Module file {} not found in search paths", filename))
    };

    let (canonical_path, contents) = match find_file(loader) {
        Ok(pair) => pair,
        Err(msg) => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(span, msg));
            return (None, result);
        }
    };

    // Build loader that includes the module's directory
    let module_dir = std::path::Path::new(&canonical_path)
        .parent()
        .and_then(|p| p.to_str())
        .map(path::canonicalize)
        .unwrap_or_else(|| canonical_path.clone());
    let mut new_search_paths = vec![module_dir];
    new_search_paths.extend(loader.search_paths.iter().cloned());
    let loader_for_module = FileLoader {
        search_paths: path::normalize_search_paths(new_search_paths),
        read_file: loader.read_file.clone(),
    };

    // Parse the module file
    let program = match language::parse(&contents) {
        Ok(p) => p,
        Err(parse_errors) => {
            let mut result = InterpResult::ok(context.clone());
            result.errors.extend(parse_errors);
            return (None, result);
        }
    };

    // Interpret the included module
    let included_module_id = canonical_path.clone();
    let include_context = Context::new_sharing_state(included_module_id.clone(), context);
    let include_result = interpret_program(&loader_for_module, include_context, &program);

    let mut result = InterpResult::ok(context.clone());
    result.errors.extend(include_result.errors.clone());

    if include_result.has_errors() {
        return (None, result);
    }

    // Carry forward the state from included module (has all new types/cells)
    result.context.state = Arc::clone(&include_result.context.state);

    let updated_state = &*include_result.context.state;

    let included_location = match updated_state.find_module(&included_module_id) {
        Some(loc) => loc.clone(),
        None => {
            result.add_error(make_error(span, "Included module complex not found"));
            return (None, result);
        }
    };

    let mut current_location = match updated_state.find_module(&module_id) {
        Some(loc) => loc.clone(),
        None => location.clone(),
    };

    // Copy generators from included module
    for gen_name in included_location.generator_names() {
        if gen_name.is_empty() {
            continue;
        }
        let gen_entry = match included_location.find_generator(&gen_name) {
            Some(e) => e.clone(),
            None => continue,
        };
        if current_location.find_generator_by_tag(&gen_entry.tag).is_some() {
            continue;
        }
        let classifier = match included_location.classifier(&gen_name) {
            Some(d) => d.clone(),
            None => continue,
        };
        let combined_name = if alias.is_empty() {
            gen_name.clone()
        } else if gen_name.is_empty() {
            alias.clone()
        } else {
            format!("{}.{}", alias, gen_name)
        };
        current_location.add_generator(combined_name, classifier);
    }

    let inclusion = identity_map(&include_result.context, &included_location);
    current_location.add_map(
        alias,
        MapDomain::Module(included_module_id),
        inclusion,
    );

    result.context.state_mut().set_module(module_id, current_location.clone());

    (Some(current_location), result)
}

pub fn interpret_include_instr(
    context: &Context,
    _mode: Mode,
    location: &Complex,
    include_stmt: &ast::IncludeStmt,
    span: Span,
) -> (Option<Complex>, InterpResult) {
    let (include_out, include_result) = interpret_include(context, include_stmt, span);
    let context_after = include_result.context.clone();

    let (id, name) = match include_out {
        None => return (None, include_result),
        Some(pair) => pair,
    };

    if location.name_in_use(&name) {
        let mut r = include_result;
        r.add_error(make_error(span, format!("Partial map name already in use: {}", name)));
        return (None, r);
    }

    let subtype = match context_after.state.find_type(id) {
        None => {
            let mut r = include_result;
            r.add_error(make_error(span,
                format!("Type {} not found in global record", id)));
            return (None, r);
        }
        Some(te) => (*te.complex).clone(),
    };

    let mut new_location = location.clone();
    for gen_name in subtype.generator_names() {
        if let Some(gen_entry) = subtype.find_generator(&gen_name) {
            if new_location.find_generator_by_tag(&gen_entry.tag).is_some() {
                continue;
            }
            let classifier = match subtype.classifier(&gen_name) {
                Some(d) => d.clone(),
                None => continue,
            };
            let alias_prefix = name.as_str();
            let combined = if alias_prefix.is_empty() { gen_name.clone() }
                else if gen_name.is_empty() { alias_prefix.to_owned() }
                else { format!("{}.{}", alias_prefix, gen_name) };
            new_location.add_generator(combined, classifier);
        }
    }

    let inclusion = identity_map(&context_after, &subtype);
    new_location.add_map(name, MapDomain::Type(id), inclusion);

    (Some(new_location), include_result)
}

pub fn interpret_attach_instr(
    context: &Context,
    mode: Mode,
    location: &Complex,
    attach_stmt: &ast::AttachStmt,
    span: Span,
) -> (Option<Complex>, InterpResult) {
    let (attach_out, attach_result) = interpret_attach(context, location, attach_stmt, span);
    let context_after = attach_result.context.clone();

    let (name, map, domain) = match attach_out {
        None => return (None, attach_result),
        Some(triple) => triple,
    };

    if location.name_in_use(&name) {
        let mut r = attach_result;
        r.add_error(make_error(attach_stmt.name.span,
            format!("Partial map name already in use: {}", name)));
        return (None, r);
    }

    let attachment_id = match &domain {
        MapDomain::Type(id) => *id,
        MapDomain::Module(_) => {
            let mut r = attach_result;
            r.add_error(make_error(unknown_span(), "Unexpected module domain in attach"));
            return (None, r);
        }
    };

    let attachment = match context_after.state.find_type(attachment_id) {
        None => {
            let mut r = attach_result;
            r.add_error(make_error(attach_stmt.name.span,
                format!("Type {} not found in global record", attachment_id)));
            return (None, r);
        }
        Some(te) => (*te.complex).clone(),
    };

    let mut generators: Vec<(usize, LocalId, Tag)> = attachment.generator_names()
        .into_iter()
        .filter_map(|n| attachment.find_generator(&n).map(|e| (e.dim, n, e.tag.clone())))
        .collect();
    generators.sort_by_key(|(dim, _, _)| *dim);

    let mut current_location = location.clone();
    let mut current_state = Arc::clone(&context_after.state);
    let mut current_map = map.clone();

    for (gen_dim, gen_name, gen_tag) in &generators {
        if current_map.is_defined_at(gen_tag) {
            continue;
        }

        let gen_cell_data = match gen_tag {
            Tag::Global(gid) => {
                match current_state.find_cell(*gid) {
                    Some(ce) => ce.data.clone(),
                    None => continue,
                }
            }
            Tag::Local(_) => continue,
        };

        let image_cell_data = match &gen_cell_data {
            CellData::Zero => CellData::Zero,
            CellData::Boundary { boundary_in, boundary_out } => {
                let image_in = match PMap::apply(&current_map, boundary_in) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let image_out = match PMap::apply(&current_map, boundary_out) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                CellData::Boundary { boundary_in: Arc::new(image_in), boundary_out: Arc::new(image_out) }
            }
        };

        let base_name = name.as_str();
        let gen_name_str = gen_name.as_str();
        let combined = if base_name.is_empty() { gen_name_str.to_owned() }
            else if gen_name_str.is_empty() { base_name.to_owned() }
            else { format!("{}.{}", base_name, gen_name_str) };

        let image_tag = match mode {
            Mode::Global => {
                let image_id = GlobalId::fresh();
                Arc::make_mut(&mut current_state).set_cell(image_id, *gen_dim, image_cell_data.clone());
                Tag::Global(image_id)
            }
            Mode::Local => Tag::Local(combined.clone()),
        };

        let image_classifier = match Diagram::cell(image_tag.clone(), &image_cell_data) {
            Ok(d) => d,
            Err(_) => continue,
        };

        match mode {
            Mode::Global => current_location.add_generator(combined.clone(), image_classifier.clone()),
            Mode::Local => {
                current_location.add_local_cell(combined.clone(), *gen_dim, image_cell_data.clone());
                current_location.add_generator(combined.clone(), image_classifier.clone());
            }
        };

        current_map.insert_raw(gen_tag.clone(), *gen_dim, gen_cell_data, image_classifier);
    }

    current_location.add_map(name, domain, current_map);
    let mut r = attach_result;
    r.context.state = current_state;
    (Some(current_location), r)
}

fn interpret_include(
    context: &Context,
    include_stmt: &ast::IncludeStmt,
    span: Span,
) -> (Option<(GlobalId, LocalId)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &include_stmt.address.inner, include_stmt.address.span);
    match id_opt {
        None => (None, addr_result),
        Some(id) => {
            let name = match &include_stmt.alias {
                Some(alias_node) => alias_node.inner.clone(),
                None => {
                    let module_id = &context.current_module;
                    let tag = Tag::Global(id);
                    match context.state.find_module(module_id)
                        .and_then(|m| m.find_generator_by_tag(&tag))
                    {
                        Some(gen_name) => {
                            if gen_name.contains('.') {
                                let mut r = addr_result;
                                r.add_error(make_error(span,
                                    "Inclusion of non-local types requires an alias"));
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
    location: &Complex,
    attach_stmt: &ast::AttachStmt,
    span: Span,
) -> (Option<(LocalId, PMap, MapDomain)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &attach_stmt.address.inner, attach_stmt.address.span);
    let context_after = addr_result.context.clone();

    let id = match id_opt {
        None => return (None, addr_result),
        Some(i) => i,
    };

    let name = attach_stmt.name.inner.clone();

    match &attach_stmt.along {
        None => {
            let map = PMap::empty().unwrap();
            (Some((name, map, MapDomain::Type(id))), addr_result)
        }
        Some(pmap_node) => {
            let source = match context_after.state.find_type(id) {
                Some(te) => (*te.complex).clone(),
                None => {
                    let mut r = addr_result;
                    r.add_error(make_error(span, format!("Type {} not found", id)));
                    return (None, r);
                }
            };
            let (mc_opt, pmap_result) = interpret_pmap(&context_after, location, &source, pmap_node);
            let combined = InterpResult::combine(addr_result, pmap_result);
            match mc_opt {
                None => (None, combined),
                Some(mc) => (Some((name, mc.map, MapDomain::Type(id))), combined),
            }
        }
    }
}
