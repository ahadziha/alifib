use std::sync::Arc;
use crate::helper::{error::Error, GlobalId, LocalId, ModuleId, Tag};
use crate::helper::positions::Span;
use crate::core::{
    complex::{Complex, MorphismDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    morphism::Morphism,
    state::State,
};
use crate::language::{
    ast::*,
    diagnostics::{Diagnostic, Report},
};

// ---- Context ----

/// `state` is wrapped in `Arc` so that `Context::clone()` is O(1).
/// All the immutable read operations (find_cell, find_type, etc.) deref transparently.
/// To mutate state, clone the inner value: `(*context.state).clone().set_cell(...)`.
#[derive(Debug, Clone)]
pub struct Context {
    pub current_module: ModuleId,
    pub state: Arc<State>,
}

impl Context {
    pub fn new(module_id: ModuleId, state: State) -> Self {
        Self { current_module: module_id, state: Arc::new(state) }
    }

    /// Create a context that shares the same Arc<State> as `other`.
    pub fn new_sharing_state(module_id: ModuleId, other: &Context) -> Self {
        Self { current_module: module_id, state: Arc::clone(&other.state) }
    }

    pub fn with_state(&self, state: State) -> Self {
        Self { current_module: self.current_module.clone(), state: Arc::new(state) }
    }
}

// ---- File loader ----

#[derive(Debug, Clone)]
pub enum LoadError {
    NotFound,
    IoError(String),
}

#[derive(Clone)]
pub struct FileLoader {
    pub search_paths: Vec<String>,
    pub read_file: std::sync::Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>,
}

impl FileLoader {
    pub fn default_read(path: &str) -> Result<String, LoadError> {
        if !std::path::Path::new(path).exists() {
            return Err(LoadError::NotFound);
        }
        std::fs::read_to_string(path).map_err(|e| LoadError::IoError(e.to_string()))
    }
}

// ---- Interpretation result ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Ok, Error }

#[derive(Debug, Clone)]
pub struct InterpResult {
    pub context: Context,
    pub report: Report,
    pub status: Status,
}

impl InterpResult {
    fn ok(context: Context) -> Self {
        Self { context, report: Report::empty(), status: Status::Ok }
    }

    fn add_error(&mut self, diag: Diagnostic) {
        self.status = Status::Error;
        self.report.add(diag);
    }

    fn combine(prev: InterpResult, next: InterpResult) -> InterpResult {
        let mut report = prev.report;
        report.append(next.report);
        let status = if prev.status == Status::Error || next.status == Status::Error {
            Status::Error
        } else {
            Status::Ok
        };
        InterpResult { context: next.context, report, status }
    }

    fn has_errors(&self) -> bool {
        self.status == Status::Error
    }
}

// ---- Mode ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode { Global, Local }

// ---- Namespace ----

#[derive(Debug, Clone)]
pub struct Namespace {
    pub root: GlobalId,
    pub location: Complex,
}

// ---- Term types ----

#[derive(Debug, Clone)]
pub struct MorphismComponent {
    pub morphism: Morphism,
    pub source: Arc<Complex>,
}

#[derive(Debug, Clone)]
pub enum Term {
    MTerm(MorphismComponent),
    DTerm(Diagram),
}

#[derive(Debug, Clone)]
pub enum Component {
    Term(Term),
    Hole,
    Bd(DiagramSign),
}

#[derive(Debug, Clone)]
pub enum TermPair {
    MTermPair { fst: Morphism, snd: Morphism, source: Arc<Complex> },
    DTermPair { fst: Diagram, snd: Diagram },
}

// ---- Producers ----

fn interp_producer() -> crate::helper::error::Producer {
    crate::helper::error::Producer {
        phase: crate::helper::error::Phase::Interpreter,
        module_path: Some("interpreter".to_owned()),
    }
}

fn unknown_span() -> Span {
    Span::unknown()
}

fn span_or(opt: Option<&Span>) -> Span {
    opt.cloned().unwrap_or_else(unknown_span)
}

fn make_error_diag(span: Span, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(interp_producer(), span, message)
}

// ---- Main interpreter ----

pub fn interpret_program(
    loader: &FileLoader,
    context: Context,
    program: &Program,
) -> InterpResult {
    let module_id = context.current_module.clone();

    // If the module is already in the state (previously loaded via include), skip
    // processing entirely — matching OCaml's interpret_program early-return on
    // `State.find_module` finding the module_id.
    if context.state.find_module(&module_id).is_some() {
        return InterpResult::ok(context);
    }

    // Initialize module complex ONCE at startup (matching OCaml's interpret_program).
    // This creates a single anonymous type (empty name "") that serves as the root
    // for all definition complexes in this module.
    let context = if context.state.find_module(&module_id).is_none() {
        let empty_id = GlobalId::fresh();
        let empty_tag = Tag::Global(empty_id);
        let empty_diagram = match Diagram::cell(empty_tag, &CellData::Zero) {
            Ok(d) => d,
            Err(e) => {
                let mut r = InterpResult::ok(context);
                r.add_error(make_error_diag(unknown_span(),
                    format!("Failed to create empty type cell: {}", e)));
                return r;
            }
        };
        let empty_name: LocalId = String::new();
        let mut module_complex = Complex::empty();
        module_complex = module_complex.add_generator(empty_name.clone(), empty_diagram.clone());
        module_complex = module_complex.add_diagram(empty_name, empty_diagram);
        let new_state = (*context.state).clone()
            .set_type(empty_id, CellData::Zero, Complex::empty())
            .set_module(module_id.clone(), module_complex);
        context.with_state(new_state)
    } else {
        context
    };

    let mut result = InterpResult::ok(context);
    for block in &program.value.blocks {
        let block_result = interpret_block(loader, result.context.clone(), block);
        result = InterpResult::combine(result, block_result);
    }
    result
}

fn interpret_block(loader: &FileLoader, context: Context, block: &Block) -> InterpResult {
    match &block.value {
        BlockDesc::Type { body } => interpret_block_type(loader, context, body.as_ref()),
        BlockDesc::Complex { complex, local } => {
            interpret_block_complex(loader, context, complex, local.as_ref())
        }
    }
}

fn interpret_block_type(loader: &FileLoader, context: Context, body: Option<&CBlockType>) -> InterpResult {
    // In the OCaml architecture, @Type blocks do NOT create a new anonymous type.
    // The anonymous type was already created once in interpret_program.
    // We just process the instructions in the body, each of which adds a named
    // generator directly to the module complex.
    let mut result = InterpResult::ok(context);
    if let Some(body) = body {
        let (_, body_result) = interpret_c_block_type(loader, &result.context, body);
        result = InterpResult::combine(result, body_result);
    }
    result
}

fn interpret_c_block_type(
    loader: &FileLoader,
    context: &Context,
    block: &CBlockType,
) -> (Option<Complex>, InterpResult) {
    let mut acc_result = InterpResult::ok(context.clone());
    let mut any_location: Option<Complex> = None;

    for instr in &block.value {
        let ctx = acc_result.context.clone();
        let (loc_opt, instr_result) = interpret_c_instr_type(loader, &ctx, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
        if let Some(new_loc) = loc_opt {
            any_location = Some(new_loc);
        }
        if acc_result.has_errors() {
            // Continue to process remaining instructions even on error
        }
    }

    (any_location, acc_result)
}

fn interpret_c_instr_type(
    loader: &FileLoader,
    context: &Context,
    instr: &CInstrType,
) -> (Option<Complex>, InterpResult) {
    match &instr.value {
        CInstrTypeDesc::Generator(gen_type) => {
            interpret_generator_type(context, gen_type)
        }
        CInstrTypeDesc::Dnamer(dnamer) => {
            // Dnamer in a @Type block: look up the module complex, add diagram to it, update state
            let module_id = &context.current_module;
            let module_location = context.state.find_module(module_id).cloned().unwrap_or_default();
            let (out, result) = interpret_dnamer(context, &module_location, dnamer);
            match out {
                None => (None, result),
                Some((name, diagram)) => {
                    let ctx_after = result.context.clone();
                    let module_id2 = &ctx_after.current_module;
                    let mut current_loc = ctx_after.state.find_module(module_id2).cloned().unwrap_or_default();
                    current_loc = current_loc.add_diagram(name, diagram);
                    let new_state = (*ctx_after.state).clone().set_module(module_id2.clone(), current_loc.clone());
                    let mut r = result;
                    r.context = r.context.with_state(new_state);
                    (Some(current_loc), r)
                }
            }
        }
        CInstrTypeDesc::Mnamer(mnamer) => {
            // Mnamer in a @Type block: look up the module complex, add morphism to it, update state
            let module_id = &context.current_module;
            let module_location = context.state.find_module(module_id).cloned().unwrap_or_default();
            let (out, result) = interpret_mnamer(context, &module_location, mnamer);
            match out {
                None => (None, result),
                Some((name, morphism, domain)) => {
                    let ctx_after = result.context.clone();
                    let module_id2 = &ctx_after.current_module;
                    let mut current_loc = ctx_after.state.find_module(module_id2).cloned().unwrap_or_default();
                    current_loc = current_loc.add_morphism(name, domain, morphism);
                    let new_state = (*ctx_after.state).clone().set_module(module_id2.clone(), current_loc.clone());
                    let mut r = result;
                    r.context = r.context.with_state(new_state);
                    (Some(current_loc), r)
                }
            }
        }
        CInstrTypeDesc::IncludeModule(include_mod) => {
            interpret_include_module_instr(loader, context, include_mod)
        }
    }
}

fn interpret_generator_type(
    context: &Context,
    gen_type: &GeneratorType,
) -> (Option<Complex>, InterpResult) {
    let gen = &gen_type.value.generator;
    let def = &gen_type.value.definition;

    let name = gen.value.name.value.clone();
    let name_span = span_or(gen.value.name.span.as_ref());

    // Look up the current module complex (used for boundary resolution and name collision check)
    let module_id = &context.current_module;
    let module_location = match context.state.find_module(module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error_diag(name_span, "Module not found"));
            return (None, result);
        }
        Some(m) => m.clone()
    };

    // Check for name collision in the module complex
    if module_location.name_in_use(&name) {
        let mut result = InterpResult::ok(context.clone());
        result.add_error(make_error_diag(name_span,
            format!("Generator name already in use: {}", name)));
        return (None, result);
    }

    // Parse boundaries (resolved against the module complex)
    let (boundaries, mut result) = match &gen.value.boundaries {
        None => (CellData::Zero, InterpResult::ok(context.clone())),
        Some(bounds) => {
            let (bopt, r) = interpret_boundaries(context, &module_location, bounds);
            match bopt {
                None => return (None, r),
                Some(b) => (b, r),
            }
        }
    };

    // Only 0-cells (no boundaries) are supported in @Type blocks in the current OCaml implementation
    if !matches!(boundaries, CellData::Zero) {
        result.add_error(make_error_diag(name_span,
            "Higher cells in @Type blocks are not supported"));
        return (None, result);
    }

    // Process the definition complex.
    // interpret_complex resolves the root via the empty name "" in the module complex,
    // which is the anonymous type created at startup.  Then it processes the block
    // (e.g. `{ pt, ob: pt -> pt }`) to build the definition complex for this generator.
    let context_after = result.context.clone();
    let (ns_opt, complex_result) = interpret_complex(&context_after, Mode::Global, def);
    result = InterpResult::combine(result, complex_result);

    let definition_complex = match ns_opt {
        None => return (None, result),
        Some(ns) => ns.location,
    };

    // Re-fetch the module complex after interpret_complex (which may have updated state)
    let context_after = result.context.clone();
    let module_id2 = &context_after.current_module;
    let module_location_now = match context_after.state.find_module(module_id2) {
        None => {
            result.add_error(make_error_diag(name_span, "Module not found after processing definition"));
            return (None, result);
        }
        Some(m) => m.clone()
    };

    // Create a fresh global ID for this named generator (e.g. Ob, Magma, ...)
    let new_id = GlobalId::fresh();
    let tag = Tag::Global(new_id);
    let classifier = match Diagram::cell(tag, &CellData::Zero) {
        Ok(d) => d,
        Err(e) => {
            result.add_error(make_error_diag(name_span,
                format!("Failed to create generator cell: {}", e)));
            return (None, result);
        }
    };

    // Add the identity morphism to the definition complex (maps the generator to itself)
    let identity = identity_morphism(&context_after, &definition_complex);
    let definition_with_identity = definition_complex.add_morphism(
        name.clone(),
        MorphismDomain::Type(new_id),
        identity,
    );

    // Add this named generator to the MODULE complex
    let updated_module = module_location_now
        .add_generator(name.clone(), classifier.clone())
        .add_diagram(name.clone(), classifier);

    // Update global state:
    //   - Register the type entry for new_id with the definition complex
    //   - Update the module complex
    let new_state = (*context_after.state).clone()
        .set_type(new_id, CellData::Zero, definition_with_identity)
        .set_module(module_id2.clone(), updated_module.clone());
    result.context = result.context.with_state(new_state);

    (Some(updated_module), result)
}

fn interpret_include_module_instr(
    loader: &FileLoader,
    context: &Context,
    include_mod: &Node<IncludeModuleDesc>,
) -> (Option<Complex>, InterpResult) {
    use crate::helper::path;
    use crate::language::{lexer::lex_with_implicit_commas, parser::parse};

    let desc = &include_mod.value;
    let module_name: LocalId = desc.name.value.clone();
    let alias: LocalId = desc.alias.as_ref().map(|a| a.value.clone()).unwrap_or_else(|| module_name.clone());
    let span = span_or(include_mod.span.as_ref());

    // Get the current module complex
    let module_id = context.current_module.clone();
    let location = match context.state.find_module(&module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error_diag(span, "Module not found"));
            return (None, result);
        }
        Some(m) => m.clone(),
    };

    // Check alias is not already in use
    if location.name_in_use(&alias) {
        let mut result = InterpResult::ok(context.clone());
        result.add_error(make_error_diag(span, format!("Map name already in use: {}", alias)));
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
            result.add_error(make_error_diag(span, msg));
            return (None, result);
        }
    };

    // Build a new file loader that also searches in the included module's directory
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

    // Lex and parse the module file
    let (tokens, _lex_errors) = lex_with_implicit_commas(&contents);
    let (program, parse_report) = parse(tokens, &contents, &canonical_path);

    if parse_report.has_errors() {
        let mut result = InterpResult::ok(context.clone());
        result.report.append(parse_report);
        result.status = Status::Error;
        return (None, result);
    }

    // Interpret the included module with a fresh module_id
    let included_module_id: ModuleId = canonical_path.clone();
    let include_context = Context::new_sharing_state(included_module_id.clone(), context);
    let include_result = interpret_program(&loader_for_module, include_context, &program);

    // Restore back to original module_id, carry over state
    let mut result = InterpResult::ok(context.clone());
    result.report.append(parse_report);
    result.report.append(include_result.report.clone());

    if include_result.has_errors() {
        result.status = Status::Error;
        return (None, result);
    }

    let updated_state = &*include_result.context.state;  // &State borrow, avoids clone

    // Get the included module's complex
    let included_location = match updated_state.find_module(&included_module_id) {
        Some(loc) => loc.clone(),
        None => {
            result.add_error(make_error_diag(span, "Included module complex not found"));
            return (None, result);
        }
    };

    // Get the current module complex (now updated with state from included module's interpret)
    let mut current_location = match updated_state.find_module(&module_id) {
        Some(loc) => loc.clone(),
        None => location.clone(),
    };

    // Copy generators from included module into current module, with alias prefix
    for gen_name in included_location.generator_names() {
        if gen_name.is_empty() {
            continue;
        }
        let gen_entry = match included_location.find_generator(&gen_name) {
            Some(e) => e.clone(),
            None => continue,
        };
        // Skip if already present by tag
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
        current_location = current_location.add_generator(combined_name, classifier);
    }

    // Build identity morphism for the included module and register under alias
    let inclusion = identity_morphism(&include_result.context, &included_location);
    let final_location = current_location.add_morphism(
        alias,
        MorphismDomain::Module(included_module_id),
        inclusion,
    );

    // Update state
    let final_state = updated_state.clone().set_module(module_id, final_location.clone());
    result.context = result.context.with_state(final_state);

    (Some(final_location), result)
}

fn interpret_block_complex(
    _loader: &FileLoader,
    context: Context,
    complex: &Node<ComplexDesc>,
    local: Option<&CBlockLocal>,
) -> InterpResult {
    let (ns_opt, complex_result) = interpret_complex(&context, Mode::Global, complex);
    let mut result = complex_result;

    let namespace = match ns_opt {
        None => return result,
        Some(ns) => ns,
    };

    if let Some(local_block) = local {
        let (_, local_result) = interpret_c_block_local(
            &result.context,
            &namespace,
            local_block,
        );
        result = InterpResult::combine(result, local_result);
    }

    result
}

fn interpret_c_block_local(
    context: &Context,
    namespace: &Namespace,
    block: &CBlockLocal,
) -> (Option<Complex>, InterpResult) {
    let mut current_ns = namespace.clone();
    let mut acc_result = InterpResult::ok(context.clone());

    for instr in &block.value {
        let ctx = acc_result.context.clone();
        let (loc_opt, instr_result) = interpret_c_instr_local(&ctx, &current_ns, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
        if let Some(new_loc) = loc_opt {
            current_ns = Namespace { root: current_ns.root, location: new_loc };  // no clone
        }
        if acc_result.has_errors() {
            break;
        }
    }

    (Some(current_ns.location), acc_result)
}

fn interpret_c_instr_local(
    context: &Context,
    namespace: &Namespace,
    instr: &CInstrLocal,
) -> (Option<Complex>, InterpResult) {
    let root = namespace.root;
    let location = &namespace.location;

    match &instr.value {
        CInstrLocalDesc::Dnamer(dnamer) => {
            let (out, result) = interpret_dnamer(context, location, dnamer);
            let context_after = result.context.clone();
            match out {
                None => (None, result),
                Some((name, diagram)) => {
                    if location.name_in_use(&name) {
                        let span = span_or(dnamer.value.name.span.as_ref());
                        let mut r = result;
                        r.add_error(make_error_diag(span,
                            format!("Diagram name already in use: {}", name)));
                        return (None, r);
                    }
                    if diagram.has_local_labels() {
                        let span = span_or(dnamer.value.body.span.as_ref());
                        let mut r = result;
                        r.add_error(make_error_diag(span,
                            "Named diagrams must contain only global cells"));
                        return (None, r);
                    }
                    let new_location = location.clone().add_diagram(name.clone(), diagram.clone());
                    // Update root type complex
                    let root_complex = match context_after.state.find_type(root) {
                        Some(te) => (*te.complex).clone().add_diagram(name, diagram),
                        None => return (None, result),
                    };
                    let new_state = (*context_after.state).clone().update_type_complex(root, root_complex);
                    let mut r = result;
                    r.context = r.context.with_state(new_state);
                    (Some(new_location), r)
                }
            }
        }
        CInstrLocalDesc::Mnamer(mnamer) => {
            let (out, result) = interpret_mnamer(context, location, mnamer);
            let context_after = result.context.clone();
            match out {
                None => (None, result),
                Some((name, morphism, domain)) => {
                    if location.name_in_use(&name) {
                        let span = span_or(mnamer.value.name.span.as_ref());
                        let mut r = result;
                        r.add_error(make_error_diag(span,
                            format!("Map name already in use: {}", name)));
                        return (None, r);
                    }
                    if morphism.has_local_labels() {
                        let span = span_or(mnamer.value.definition.span.as_ref());
                        let mut r = result;
                        r.add_error(make_error_diag(span,
                            "Named maps must only be valued in global cells"));
                        return (None, r);
                    }
                    let new_location = location.clone().add_morphism(name.clone(), domain.clone(), morphism.clone());
                    let root_complex = match context_after.state.find_type(root) {
                        Some(te) => (*te.complex).clone().add_morphism(name, domain, morphism),
                        None => return (None, result),
                    };
                    let new_state = (*context_after.state).clone().update_type_complex(root, root_complex);
                    let mut r = result;
                    r.context = r.context.with_state(new_state);
                    (Some(new_location), r)
                }
            }
        }
        CInstrLocalDesc::Assert(assert_stmt) => {
            let (term_pair_opt, assert_result) = interpret_assert(context, location, assert_stmt);
            let span = span_or(assert_stmt.span.as_ref());
            match term_pair_opt {
                None => (None, assert_result),
                Some(term_pair) => {
                    let check_result = check_assert(&assert_result.context, location, &term_pair, span);
                    match check_result {
                        Ok(_) => (Some(location.clone()), assert_result),
                        Err(msg) => {
                            let mut r = assert_result;
                            r.add_error(make_error_diag(span_or(instr.span.as_ref()), msg));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

fn check_assert(
    _context: &Context,
    _location: &Complex,
    pair: &TermPair,
    _span: Span,
) -> Result<(), String> {
    match pair {
        TermPair::DTermPair { fst, snd } => {
            if Diagram::isomorphic(fst, snd) { Ok(()) }
            else { Err("The diagrams are not equal".into()) }
        }
        TermPair::MTermPair { fst, snd, source } => {
            let generators: Vec<_> = {
                let mut gens: Vec<(usize, LocalId, Tag)> = source.generator_names()
                    .into_iter()
                    .filter_map(|name| {
                        source.find_generator(&name).map(|e| (e.dim, name, e.tag.clone()))
                    })
                    .collect();
                gens.sort_by_key(|(dim, _, _)| *dim);
                gens
            };

            for (_, gen_name, tag) in &generators {
                let in_first  = fst.is_defined_at(tag);
                let in_second = snd.is_defined_at(tag);
                if in_first && !in_second {
                    return Err(format!(
                        "`{}` is in the domain of the first map but not the second",
                        gen_name
                    ));
                }
                if in_second && !in_first {
                    return Err(format!(
                        "`{}` is in the domain of the second map but not the first",
                        gen_name
                    ));
                }
                if in_first {
                    let img1 = fst.image(tag).map_err(|e| e.to_string())?;
                    let img2 = snd.image(tag).map_err(|e| e.to_string())?;
                    if !Diagram::isomorphic(img1, img2) {
                        return Err(format!("The maps differ on `{}`", gen_name));
                    }
                }
            }
            Ok(())
        }
    }
}

fn interpret_complex(
    context: &Context,
    mode: Mode,
    complex: &Node<ComplexDesc>,
) -> (Option<Namespace>, InterpResult) {
    let module_id = &context.current_module;
    let complex_span = span_or(complex.span.as_ref());

    let module_space = match context.state.find_module(module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error_diag(complex_span.clone(),
                format!("Module `{}` not found", module_id)));
            return (None, result);
        }
        Some(m) => m.clone(),
    };

    let empty_name: LocalId = String::new();

    // Resolve the root (address → global id)
    let (root_opt, root_result) = match &complex.value.address {
        None => {
            match module_space.find_generator(&empty_name) {
                None => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error_diag(complex_span.clone(), "Root generator not found"));
                    (None, r)
                }
                Some(entry) => match &entry.tag {
                    Tag::Global(id) => (Some(*id), InterpResult::ok(context.clone())),
                    Tag::Local(_) => {
                        let mut r = InterpResult::ok(context.clone());
                        r.add_error(make_error_diag(complex_span.clone(), "Root has local tag (unexpected)"));
                        (None, r)
                    }
                }
            }
        }
        Some(addr) => interpret_address(context, addr),
    };

    let mut result = root_result;
    let root = match root_opt {
        None => return (None, result),
        Some(r) => r,
    };

    let type_entry = match result.context.state.find_type(root) {
        None => {
            result.add_error(make_error_diag(complex_span,
                format!("Type {} not found in global record", root)));
            return (None, result);
        }
        Some(te) => te.clone(),
    };

    let initial_location = (*type_entry.complex).clone();

    match &complex.value.block {
        None => {
            let ns = Namespace { root, location: initial_location };
            (Some(ns), result)
        }
        Some(block) => {
            let (location_opt, block_result) = interpret_c_block(
                &result.context, mode, &initial_location, block
            );
            result = InterpResult::combine(result, block_result);
            let location = location_opt.unwrap_or(initial_location);
            let ns = Namespace { root, location };
            (Some(ns), result)
        }
    }
}

fn interpret_c_block(
    context: &Context,
    mode: Mode,
    initial_location: &Complex,
    block: &CBlock,
) -> (Option<Complex>, InterpResult) {
    // Pass both context and location by value so:
    //  - Arc::make_mut can mutate State in-place (from iteration 2+, refcount=1)
    //  - Complex::add_* methods don't need a prior clone (location is owned)
    let mut current_location: Complex = initial_location.clone();
    let mut current_context: Context = context.clone();
    let mut acc_report = Report::empty();
    let mut acc_status = Status::Ok;

    for instr in &block.value {
        let (new_location, instr_result) =
            interpret_c_instr(current_context, mode, current_location, instr);
        current_location = new_location;
        current_context = instr_result.context;
        acc_report.append(instr_result.report);
        if instr_result.status == Status::Error {
            acc_status = Status::Error;
        }
    }

    let acc_result = InterpResult { context: current_context, report: acc_report, status: acc_status };
    (Some(current_location), acc_result)
}

fn interpret_c_instr(
    context: Context,
    mode: Mode,
    location: Complex,
    instr: &CInstr,
) -> (Complex, InterpResult) {
    match &instr.value {
        CInstrDesc::Generator(gen) => {
            // Move both context and location into interpret_generator_instr so it can:
            //  - use Arc::make_mut for in-place State mutation
            //  - call add_generator/add_diagram without a prior clone of location
            interpret_generator_instr(context, mode, location, gen)
        }
        CInstrDesc::Dnamer(dnamer) => {
            let (out, result) = interpret_dnamer(&context, &location, dnamer);
            match out {
                None => (location, result),
                Some((name, diagram)) => {
                    if location.name_in_use(&name) {
                        let span = span_or(dnamer.value.name.span.as_ref());
                        let mut r = result;
                        r.add_error(make_error_diag(span, format!("Diagram name already in use: {}", name)));
                        return (location, r);
                    }
                    // Consume owned location — no clone needed.
                    let new_location = location.add_diagram(name, diagram);
                    (new_location, result)
                }
            }
        }
        CInstrDesc::Mnamer(mnamer) => {
            let (out, result) = interpret_mnamer(&context, &location, mnamer);
            match out {
                None => (location, result),
                Some((name, morphism, domain)) => {
                    if location.name_in_use(&name) {
                        let span = span_or(mnamer.value.name.span.as_ref());
                        let mut r = result;
                        r.add_error(make_error_diag(span, format!("Map name already in use: {}", name)));
                        return (location, r);
                    }
                    // Consume owned location — no clone needed.
                    let new_location = location.add_morphism(name, domain, morphism);
                    (new_location, result)
                }
            }
        }
        CInstrDesc::Include(include_stmt) => {
            let (loc_opt, result) = interpret_include_instr(&context, mode, &location, include_stmt);
            // Return updated location if successful, otherwise original (no clone either way).
            (loc_opt.unwrap_or(location), result)
        }
        CInstrDesc::Attach(attach_stmt) => {
            let (loc_opt, result) = interpret_attach_instr(&context, mode, &location, attach_stmt);
            (loc_opt.unwrap_or(location), result)
        }
    }
}

fn interpret_generator_instr(
    context: Context,
    mode: Mode,
    location: Complex,
    gen: &Generator,
) -> (Complex, InterpResult) {
    let name = gen.value.name.value.clone();
    let name_span = span_or(gen.value.name.span.as_ref());

    if location.name_in_use(&name) {
        let mut result = InterpResult::ok(context);
        result.add_error(make_error_diag(name_span, format!("Generator name already in use: {}", name)));
        return (location, result);
    }

    // Borrow context for boundary interpretation, then drop it so that
    // result.context.state has Arc refcount=1, enabling in-place mutation below.
    let (boundaries, mut result) = match &gen.value.boundaries {
        None => {
            // Move context directly into the result (no extra Arc clone).
            (CellData::Zero, InterpResult::ok(context))
        }
        Some(bounds) => {
            let (bopt, r) = interpret_boundaries(&context, &location, bounds);
            // Drop the original context so the Arc refcount falls to 1 in r.context.
            drop(context);
            match bopt {
                None => return (location, r),
                Some(b) => (b, r),
            }
        }
    };
    // `context` is no longer accessible (moved or dropped above).

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

    let bounds_span = gen.value.boundaries.as_ref()
        .and_then(|b| b.span.as_ref())
        .or(gen.span.as_ref())
        .cloned()
        .unwrap_or_else(unknown_span);

    let classifier = match Diagram::cell(tag.clone(), &boundaries) {
        Ok(d) => d,
        Err(e) => {
            result.add_error(make_error_diag(bounds_span,
                format!("Failed to create generator cell: {}", e)));
            return (location, result);
        }
    };

    // Consume owned location — add_generator/add_diagram take self by value,
    // so no clone is needed.
    let mut new_location = location
        .add_generator(name.clone(), classifier.clone())
        .add_diagram(name.clone(), classifier.clone());

    if mode == Mode::Local {
        new_location = new_location.add_local_cell(name.clone(), dim, boundaries.clone());
    }

    if let (Mode::Global, Some(id)) = (mode, new_id_opt) {
        // Use Arc::make_mut for in-place mutation when the Arc is unique (refcount=1),
        // which is the case from the second generator onward in a block.
        Arc::make_mut(&mut result.context.state).set_cell_mut(id, dim, boundaries);
    }

    (new_location, result)
}

fn interpret_include_instr(
    context: &Context,
    _mode: Mode,
    location: &Complex,
    include_stmt: &IncludeStatement,
) -> (Option<Complex>, InterpResult) {
    let (include_out, include_result) = interpret_include(context, include_stmt);
    let context_after = include_result.context.clone();

    let (id, name) = match include_out {
        None => return (None, include_result),
        Some(pair) => pair,
    };

    if location.name_in_use(&name) {
        let span = span_or(include_stmt.span.as_ref());
        let mut r = include_result;
        r.add_error(make_error_diag(span, format!("Map name already in use: {}", name)));
        return (None, r);
    }

    let subtype = match context_after.state.find_type(id) {
        None => {
            let span = span_or(include_stmt.span.as_ref());
            let mut r = include_result;
            r.add_error(make_error_diag(span,
                format!("Type {} not found in global record", id)));
            return (None, r);
        }
        Some(te) => (*te.complex).clone(),
    };

    // Copy generators from subtype into current location with name prefix
    let mut new_location = location.clone();
    for gen_name in subtype.generator_names() {
        if let Some(gen_entry) = subtype.find_generator(&gen_name) {
            // Skip if already present (by tag)
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
            new_location = new_location.add_generator(combined, classifier);
        }
    }

    let inclusion = identity_morphism(&context_after, &subtype);
    let new_location = new_location.add_morphism(name, MorphismDomain::Type(id), inclusion);

    (Some(new_location), include_result)
}

fn interpret_attach_instr(
    context: &Context,
    mode: Mode,
    location: &Complex,
    attach_stmt: &AttachStatement,
) -> (Option<Complex>, InterpResult) {
    let (attach_out, attach_result) = interpret_attach(context, location, attach_stmt);
    let context_after = attach_result.context.clone();

    let (name, morphism, domain) = match attach_out {
        None => return (None, attach_result),
        Some(triple) => triple,
    };

    if location.name_in_use(&name) {
        let span = span_or(attach_stmt.value.name.span.as_ref());
        let mut r = attach_result;
        r.add_error(make_error_diag(span, format!("Map name already in use: {}", name)));
        return (None, r);
    }

    let attachment_id = match &domain {
        MorphismDomain::Type(id) => *id,
        MorphismDomain::Module(_) => {
            let mut r = attach_result;
            r.add_error(make_error_diag(unknown_span(), "Unexpected module domain in attach"));
            return (None, r);
        }
    };

    let attachment = match context_after.state.find_type(attachment_id) {
        None => {
            let span = span_or(attach_stmt.value.name.span.as_ref());
            let mut r = attach_result;
            r.add_error(make_error_diag(span,
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
    let mut current_state = (*context_after.state).clone();
    let mut current_morphism = morphism.clone();

    for (gen_dim, gen_name, gen_tag) in &generators {
        if current_morphism.is_defined_at(gen_tag) {
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
                let image_in = match Morphism::apply(&current_morphism, boundary_in) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let image_out = match Morphism::apply(&current_morphism, boundary_out) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                CellData::Boundary { boundary_in: image_in, boundary_out: image_out }
            }
        };

        let base_name = name.as_str();
        let gen_name_str = gen_name.as_str();
        let combined = if base_name.is_empty() { gen_name_str.to_owned() }
            else if gen_name_str.is_empty() { base_name.to_owned() }
            else { format!("{}.{}", base_name, gen_name_str) };

        // Compute image_tag without cloning current_location.
        let image_tag = match mode {
            Mode::Global => {
                let image_id = GlobalId::fresh();
                current_state = current_state.set_cell(image_id, *gen_dim, image_cell_data.clone());
                Tag::Global(image_id)
            }
            Mode::Local => Tag::Local(combined.clone()),
        };

        let image_classifier = match Diagram::cell(image_tag.clone(), &image_cell_data) {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Update current_location in-place (no clone) using owned value.
        current_location = match mode {
            Mode::Global => current_location.add_generator(combined.clone(), image_classifier.clone()),
            Mode::Local => current_location
                .add_local_cell(combined.clone(), *gen_dim, image_cell_data.clone())
                .add_generator(combined.clone(), image_classifier.clone()),
        };

        current_morphism.insert_raw(gen_tag.clone(), *gen_dim, gen_cell_data, image_classifier);
    }

    let final_location = current_location.add_morphism(name, domain, current_morphism);
    let mut r = attach_result;
    r.context = r.context.with_state(current_state);
    (Some(final_location), r)
}

// ---- Address resolution ----

fn interpret_address(
    context: &Context,
    address: &Address,
) -> (Option<GlobalId>, InterpResult) {
    let module_id = &context.current_module;
    let addr_span = span_or(address.span.as_ref());

    let module_space = match context.state.find_module(module_id) {
        None => {
            let mut r = InterpResult::ok(context.clone());
            r.add_error(make_error_diag(addr_span,
                format!("Module `{}` not found", module_id)));
            return (None, r);
        }
        Some(m) => m.clone(),
    };

    let segments: Vec<(Span, String)> = address.value.iter()
        .map(|n| (span_or(n.span.as_ref()), n.value.clone()))
        .collect();

    let empty_name: LocalId = String::new();
    let base_result = InterpResult::ok(context.clone());

    if segments.is_empty() {
        // Address is empty: use root
        return match module_space.find_generator(&empty_name) {
            None => {
                let mut r = base_result;
                r.add_error(make_error_diag(addr_span, "Root generator not found"));
                (None, r)
            }
            Some(entry) => match &entry.tag {
                Tag::Global(id) => (Some(*id), base_result),
                Tag::Local(_) => {
                    let mut r = base_result;
                    r.add_error(make_error_diag(addr_span, "Root has local tag"));
                    (None, r)
                }
            }
        };
    }

    // Split into prefix path and the last segment
    let last_idx = segments.len() - 1;
    let prefix = &segments[..last_idx];
    let (last_span, last_name) = &segments[last_idx];

    // Traverse the prefix (following morphism domains)
    let mut current_space = module_space.clone();
    for (seg_span, seg_name) in prefix {
        match current_space.find_morphism(seg_name) {
            None => {
                let mut r = base_result;
                r.add_error(make_error_diag(seg_span.clone(),
                    format!("Map `{}` not found", seg_name)));
                return (None, r);
            }
            Some(me) => {
                match &me.domain {
                    MorphismDomain::Module(mid) => {
                        match context.state.find_module(mid) {
                            Some(m) => current_space = m.clone(),
                            None => {
                                let mut r = base_result;
                                r.add_error(make_error_diag(seg_span.clone(),
                                    format!("Module `{}` not found", mid)));
                                return (None, r);
                            }
                        }
                    }
                    MorphismDomain::Type(_) => {
                        let mut r = base_result;
                        r.add_error(make_error_diag(seg_span.clone(),
                            format!("Domain of `{}` is not a module", seg_name)));
                        return (None, r);
                    }
                }
            }
        }
    }

    // Look up the last segment as a diagram
    match current_space.find_diagram(last_name) {
        None => {
            let mut r = base_result;
            r.add_error(make_error_diag(last_span.clone(),
                format!("Type `{}` not found", last_name)));
            (None, r)
        }
        Some(diagram) => {
            if !diagram.is_cell() {
                let mut r = base_result;
                r.add_error(make_error_diag(last_span.clone(),
                    format!("`{}` is not a cell", last_name)));
                return (None, r);
            }
            let d = diagram.dim();
            let d = if d < 0 { 0 } else { d as usize };
            match diagram.labels.get(d).and_then(|row| row.first()) {
                None => {
                    let mut r = base_result;
                    r.add_error(make_error_diag(last_span.clone(), "Cell has no top label"));
                    (None, r)
                }
                Some(Tag::Global(id)) => (Some(*id), base_result),
                Some(Tag::Local(_)) => {
                    let mut r = base_result;
                    r.add_error(make_error_diag(last_span.clone(), "Cell has local tag (unexpected)"));
                    (None, r)
                }
            }
        }
    }
}

// ---- Include / attach helpers ----

fn interpret_include(
    context: &Context,
    include_stmt: &IncludeStatement,
) -> (Option<(GlobalId, LocalId)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &include_stmt.value.address);
    match id_opt {
        None => (None, addr_result),
        Some(id) => {
            let name = match &include_stmt.value.alias {
                Some(alias_node) => alias_node.value.clone(),
                None => {
                    // Derive name from the type's generator name in the current module
                    let module_id = &context.current_module;
                    let tag = Tag::Global(id);
                    match context.state.find_module(module_id)
                        .and_then(|m| m.find_generator_by_tag(&tag))
                    {
                        Some(gen_name) => {
                            if gen_name.contains('.') {
                                let span = span_or(include_stmt.span.as_ref());
                                let mut r = addr_result;
                                r.add_error(make_error_diag(span,
                                    "Inclusion of non-local types requires an alias"));
                                return (None, r);
                            }
                            gen_name.clone()
                        }
                        None => {
                            let span = span_or(include_stmt.span.as_ref());
                            let mut r = addr_result;
                            r.add_error(make_error_diag(span, "Could not infer include alias"));
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
    attach_stmt: &AttachStatement,
) -> (Option<(LocalId, Morphism, MorphismDomain)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &attach_stmt.value.address);
    let context_after = addr_result.context.clone();

    let id = match id_opt {
        None => return (None, addr_result),
        Some(i) => i,
    };

    let name = attach_stmt.value.name.value.clone();

    match &attach_stmt.value.along {
        None => {
            let morphism = Morphism::init().unwrap();
            (Some((name, morphism, MorphismDomain::Type(id))), addr_result)
        }
        Some(m_def) => {
            let source = match context_after.state.find_type(id) {
                Some(te) => (*te.complex).clone(),
                None => {
                    let span = span_or(attach_stmt.span.as_ref());
                    let mut r = addr_result;
                    r.add_error(make_error_diag(span, format!("Type {} not found", id)));
                    return (None, r);
                }
            };
            let (morph_opt, m_def_result) = interpret_m_def(&context_after, location, &source, m_def);
            let combined = InterpResult::combine(addr_result, m_def_result);
            match morph_opt {
                None => (None, combined),
                Some(morphism) => (Some((name, morphism, MorphismDomain::Type(id))), combined),
            }
        }
    }
}

// ---- Morphism interpretation ----

fn interpret_m_def(
    context: &Context,
    location: &Complex,
    source: &Complex,
    m_def: &MDef,
) -> (Option<Morphism>, InterpResult) {
    match &m_def.value {
        MDefDesc::Morphism(morph_node) => {
            let (out, result) = interpret_morphism(context, location, morph_node);
            match out {
                None => (None, result),
                Some(mc) => (Some(mc.morphism), result),
            }
        }
        MDefDesc::Ext(ext_node) => {
            interpret_m_ext(context, location, source, ext_node)
        }
    }
}

fn interpret_morphism(
    context: &Context,
    location: &Complex,
    morphism: &Node<MorphismDesc>,
) -> (Option<MorphismComponent>, InterpResult) {
    match &morphism.value {
        MorphismDesc::Single(mc) => interpret_m_comp(context, location, mc),
        MorphismDesc::Concat { left, right } => {
            let (left_opt, left_result) = interpret_morphism(context, location, left);
            match left_opt {
                None => (None, left_result),
                Some(left_comp) => {
                    let (right_opt, right_result) = interpret_m_comp(
                        &left_result.context, &*left_comp.source, right
                    );
                    let combined = InterpResult::combine(left_result, right_result);
                    match right_opt {
                        None => (None, combined),
                        Some(right_comp) => {
                            let composed = Morphism::compose(&left_comp.morphism, &right_comp.morphism);
                            (Some(MorphismComponent { morphism: composed, source: right_comp.source }), combined)
                        }
                    }
                }
            }
        }
    }
}

fn interpret_m_comp(
    context: &Context,
    location: &Complex,
    m_comp: &MComp,
) -> (Option<MorphismComponent>, InterpResult) {
    match &m_comp.value {
        MCompDesc::Term(m_term) => interpret_m_term(context, location, m_term),
        MCompDesc::Name(name_node) => {
            let name = &name_node.value;
            let span = span_or(name_node.span.as_ref());
            let base_result = InterpResult::ok(context.clone());
            match location.find_morphism(name) {
                None => {
                    let mut r = base_result;
                    r.add_error(make_error_diag(span, format!("Map not found: `{}`", name)));
                    (None, r)
                }
                Some(entry) => {
                    let source = match &entry.domain {
                        MorphismDomain::Type(id) => {
                            match context.state.find_type(*id) {
                                Some(te) => Arc::clone(&te.complex),
                                None => return {
                                    let mut r = base_result;
                                    r.add_error(make_error_diag(span, format!("Type {} not found", id)));
                                    (None, r)
                                }
                            }
                        }
                        MorphismDomain::Module(mid) => {
                            match context.state.find_module_arc(mid) {
                                Some(m) => m,
                                None => return {
                                    let mut r = base_result;
                                    r.add_error(make_error_diag(span, format!("Module `{}` not found", mid)));
                                    (None, r)
                                }
                            }
                        }
                    };
                    (Some(MorphismComponent { morphism: entry.morphism.clone(), source }), base_result)
                }
            }
        }
    }
}

fn interpret_m_term(
    context: &Context,
    location: &Complex,
    m_term: &MTerm,
) -> (Option<MorphismComponent>, InterpResult) {
    let target_complex = &m_term.value.target;
    let (ns_opt, complex_result) = interpret_complex(context, Mode::Local, target_complex);
    match ns_opt {
        None => (None, complex_result),
        Some(namespace) => {
            let source_complex = namespace.location;
            let ext_node = &m_term.value.ext;
            let (ext_opt, ext_result) = interpret_m_ext(
                &complex_result.context, location, &source_complex, ext_node
            );
            let combined = InterpResult::combine(complex_result, ext_result);
            match ext_opt {
                None => (None, combined),
                Some(morphism) => {
                    let source = Arc::new(source_complex);
                    (Some(MorphismComponent { morphism, source }), combined)
                }
            }
        }
    }
}

fn interpret_m_ext(
    context: &Context,
    location: &Complex,
    source: &Complex,
    m_ext: &MExt,
) -> (Option<Morphism>, InterpResult) {
    let _span = span_or(m_ext.span.as_ref());

    // Parse prefix morphism
    let (morph_opt, prefix_result) = match &m_ext.value.prefix {
        None => {
            (Some(MorphismComponent {
                morphism: Morphism::init().unwrap(),
                source: Arc::new(source.clone()),
            }), InterpResult::ok(context.clone()))
        }
        Some(morph_node) => interpret_morphism(context, location, morph_node),
    };

    match morph_opt {
        None => (None, prefix_result),
        Some(mc) => {
            let morphism = mc.morphism;
            // If the morphism source differs from the target, validate compatibility
            // (we skip the ptr-eq short-circuit from OCaml and just proceed)

            match &m_ext.value.block {
                None => (Some(morphism), prefix_result),
                Some(block_node) => {
                    let (block_opt, block_result) = interpret_m_block(
                        &prefix_result.context, location, source, morphism, block_node
                    );
                    let combined = InterpResult::combine(prefix_result, block_result);
                    (block_opt, combined)
                }
            }
        }
    }
}

fn interpret_m_block(
    context: &Context,
    location: &Complex,
    source: &Complex,
    initial_morphism: Morphism,
    m_block: &MBlock,
) -> (Option<Morphism>, InterpResult) {
    let mut current_morphism = initial_morphism;
    let mut acc_result = InterpResult::ok(context.clone());

    for instr in &m_block.value {
        let ctx = acc_result.context.clone();
        let (m_opt, instr_result) = interpret_m_instr(&ctx, location, source, current_morphism, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
        match m_opt {
            None => return (None, acc_result),
            Some(new_m) => current_morphism = new_m,
        }
        if acc_result.has_errors() {
            return (Some(current_morphism), acc_result);
        }
    }
    (Some(current_morphism), acc_result)
}

fn interpret_m_instr(
    context: &Context,
    location: &Complex,
    source: &Complex,
    morphism: Morphism,
    m_instr: &MInstr,
) -> (Option<Morphism>, InterpResult) {
    let span = span_or(m_instr.span.as_ref());

    let (left_opt, left_result) = interpret_pasting(context, source, &m_instr.value.source);
    match left_opt {
        None => return (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) = interpret_pasting(&left_result.context, location, &m_instr.value.target);
            let combined = InterpResult::combine(left_result, right_result);
            match right_opt {
                None => (None, combined),
                Some(right_term) => {
                    // left is from source, right is the target; extend morphism
                    match (left_term, right_term) {
                        (Term::DTerm(left_diag), Term::DTerm(right_diag)) => {
                            // Use smart_extend to map left -> right
                            match smart_extend(
                                &combined.context,
                                morphism,  // moved (owned)
                                source,
                                location,
                                &left_diag,
                                &right_diag,
                                span.clone(),
                            ) {
                                Ok(new_m) => (Some(new_m), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error_diag(span, e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        (Term::MTerm(left_mc), Term::MTerm(right_mc)) => {
                            // Both sides are morphisms: match generators through both morphisms.
                            // This handles syntax like `[ Ob => Ob ]` where both sides refer to
                            // a named type-morphism in their respective complexes.
                            // We iterate generators of the left morphism's source (sorted by dim)
                            // and for each one defined in both morphisms, smart_extend the current
                            // morphism from the left image's tag to the right image.
                            let src_left = &left_mc.source;

                            let mut generators: Vec<(usize, LocalId, Tag)> = src_left.generator_names()
                                .into_iter()
                                .filter_map(|n| {
                                    src_left.find_generator(&n).map(|e| (e.dim, n, e.tag.clone()))
                                })
                                .collect();
                            generators.sort_by_key(|(dim, _, _)| *dim);

                            let ctx_after = combined.context.clone();
                            let mut extended_morphism = morphism;
                            let mut r = combined;

                            for (_dim, gen_name, tag) in &generators {
                                let defined_left = left_mc.morphism.is_defined_at(tag);
                                let defined_right = right_mc.morphism.is_defined_at(tag);

                                if defined_left && defined_right {
                                    let left_image = match left_mc.morphism.image(tag) {
                                        Ok(d) => d.clone(),
                                        Err(e) => {
                                            r.add_error(make_error_diag(span.clone(), e.to_string()));
                                            return (None, r);
                                        }
                                    };
                                    if left_image.is_cell() {
                                        let d = if left_image.dim() < 0 { 0 } else { left_image.dim() as usize };
                                        let tag_left = match left_image.labels.get(d).and_then(|row| row.first()) {
                                            Some(t) => t.clone(),
                                            None => {
                                                r.add_error(make_error_diag(span.clone(), "Left image cell has no top label"));
                                                return (None, r);
                                            }
                                        };
                                        let right_image = match right_mc.morphism.image(tag) {
                                            Ok(d) => d.clone(),
                                            Err(e) => {
                                                r.add_error(make_error_diag(span.clone(), e.to_string()));
                                                return (None, r);
                                            }
                                        };
                                        // Build a synthetic cell diagram for tag_left
                                        let tag_left_cell_data = get_cell_data(&ctx_after, source, &tag_left)
                                            .unwrap_or(CellData::Zero);
                                        let left_cell_diag = match Diagram::cell(tag_left.clone(), &tag_left_cell_data) {
                                            Ok(d) => d,
                                            Err(e) => {
                                                r.add_error(make_error_diag(span.clone(), e.to_string()));
                                                return (None, r);
                                            }
                                        };
                                        match smart_extend(
                                            &ctx_after,
                                            extended_morphism,
                                            source,
                                            location,
                                            &left_cell_diag,
                                            &right_image,
                                            span.clone(),
                                        ) {
                                            Ok(updated) => extended_morphism = updated,
                                            Err(e) => {
                                                r.add_error(make_error_diag(span.clone(), e.to_string()));
                                                return (None, r);
                                            }
                                        }
                                    } else {
                                        // left_image is not a cell — check if all its labels are already defined
                                        let all_defined = {
                                            let mut all = true;
                                            for row in &left_image.labels {
                                                for t in row {
                                                    if !extended_morphism.is_defined_at(t) {
                                                        all = false;
                                                        break;
                                                    }
                                                }
                                            }
                                            all
                                        };
                                        if !all_defined {
                                            r.add_error(make_error_diag(span.clone(),
                                                "Failed to extend map (not enough information)"));
                                            return (None, r);
                                        }
                                    }
                                } else if defined_left && !defined_right {
                                    r.add_error(make_error_diag(span.clone(),
                                        format!("`{}` is in the domain of the first map but not the second", gen_name)));
                                    return (None, r);
                                } else if defined_right && !defined_left {
                                    r.add_error(make_error_diag(span.clone(),
                                        format!("`{}` is in the domain of the second map but not the first", gen_name)));
                                    return (None, r);
                                }
                                // else: neither defines it — skip
                            }

                            (Some(extended_morphism), r)
                        }
                        _ => {
                            let mut r = combined;
                            r.add_error(make_error_diag(span, "Not a well-formed assignment"));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

/// Smart extension of a morphism: adds a mapping from a source cell to a target diagram,
/// recursively extending for boundary cells as needed.
fn smart_extend(
    context: &Context,
    morphism: Morphism,
    source: &Complex,
    target: &Complex,
    source_diag: &Diagram,
    target_diag: &Diagram,
    span: Span,
) -> Result<Morphism, Error> {
    // The source diagram should be a cell; extract its top label
    if !source_diag.is_cell() {
        return Err(Error::new("Left-hand side of morphism instruction must be a cell"));
    }
    let d = if source_diag.dim() < 0 { 0 } else { source_diag.dim() as usize };
    let tag = source_diag.labels.get(d).and_then(|r| r.first())
        .ok_or_else(|| Error::new("Source cell has no top label"))?
        .clone();

    if morphism.is_defined_at(&tag) {
        let current = morphism.image(&tag)?;
        if Diagram::isomorphic(current, target_diag) {
            return Ok(morphism);
        } else {
            return Err(Error::new("The same generator is mapped to multiple diagrams"));
        }
    }

    let cell_data = get_cell_data(context, source, &tag)
        .ok_or_else(|| Error::new("Cannot find cell data for generator"))?;

    let dim = if source_diag.dim() < 0 { 0 } else { source_diag.dim() as usize };

    // Collect missing boundary tags
    let missing = match &cell_data {
        CellData::Zero => vec![],
        CellData::Boundary { boundary_in, boundary_out } => {
            let mut missing = vec![];
            for (bd, sign) in &[(boundary_in, DiagramSign::Input), (boundary_out, DiagramSign::Output)] {
                let bd_d = if bd.dim() < 0 { 0 } else { bd.dim() as usize };
                if let Some(row) = bd.labels.get(bd_d) {
                    for t in row {
                        if !morphism.is_defined_at(t) {
                            missing.push((t.clone(), *sign));
                        }
                    }
                }
            }
            missing
        }
    };

    let mut current = morphism;

    // Recursively extend for missing boundary tags
    for (focus, sign) in &missing {
        if current.is_defined_at(focus) {
            continue;
        }
        let dim_minus_one = dim - 1;
        let cell_data_focus = get_cell_data(context, source, focus)
            .ok_or_else(|| Error::new(format!("Cannot find cell data for boundary cell {}", focus)))?;

        // Find the image of the focus in the target
        let target_boundary = match sign {
            DiagramSign::Input => Diagram::boundary(DiagramSign::Input, dim_minus_one, target_diag)?,
            DiagramSign::Output => Diagram::boundary(DiagramSign::Output, dim_minus_one, target_diag)?,
        };

        let source_boundary = match (&cell_data, sign) {
            (CellData::Boundary { boundary_in, .. }, DiagramSign::Input) => boundary_in.clone(),
            (CellData::Boundary { boundary_out, .. }, DiagramSign::Output) => boundary_out.clone(),
            _ => continue,
        };

        if source_boundary.is_cell() {
            let sub_source = &source_boundary;
            current = smart_extend(context, current, source, target, sub_source, &target_boundary, span.clone())?;
        } else {
            // Try to determine the image by isomorphism
            match crate::core::ogposet::isomorphism_of(&source_boundary.shape, &target_boundary.shape) {
                Err(_) => return Err(Error::new("Failed to extend map (boundary shapes don't match)")),
                Ok(embedding) => {
                    let bd_d = if source_boundary.dim() < 0 { 0 } else { source_boundary.dim() as usize };
                    let bd_labels = &source_boundary.labels;
                    let target_labels = &target_boundary.labels;
                    let map = &embedding.map;

                    let mut image_tag: Option<Tag> = None;
                    let mut consistent = true;

                    if let Some(row) = bd_labels.get(bd_d) {
                        if let Some(map_row) = map.get(bd_d) {
                            for (idx, t) in row.iter().enumerate() {
                                if t == focus {
                                    if let Some(&mapped_idx) = map_row.get(idx) {
                                        if let Some(target_row) = target_labels.get(bd_d) {
                                            if let Some(mapped_t) = target_row.get(mapped_idx) {
                                                match &image_tag {
                                                    None => image_tag = Some(mapped_t.clone()),
                                                    Some(existing) => {
                                                        if existing != mapped_t {
                                                            consistent = false;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if !consistent {
                        return Err(Error::new("The same generator is mapped to multiple diagrams"));
                    }

                    let mapped_tag = image_tag
                        .ok_or_else(|| Error::new("Failed to extend map (no image found)"))?;

                    let gen_name = target.find_generator_by_tag(&mapped_tag)
                        .ok_or_else(|| Error::new("Image tag not found in target complex"))?
                        .clone();
                    let d_focus = target.classifier(&gen_name)
                        .ok_or_else(|| Error::new("Classifier not found for image generator"))?
                        .clone();

                    let focus_source = match source_boundary.labels.get(bd_d).and_then(|r| {
                        r.iter().position(|t| t == focus)
                    }) {
                        Some(_) => {
                            // Build a trivial single-cell diagram for the focus
                            Diagram::cell(focus.clone(), &cell_data_focus)?
                        }
                        None => continue,
                    };

                    current = smart_extend(context, current, source, target, &focus_source, &d_focus, span.clone())?;
                }
            }
        }
    }

    // Extend with the main tag
    Morphism::extend(current, tag, dim, cell_data, target_diag.clone())
}

fn get_cell_data(context: &Context, source: &Complex, tag: &Tag) -> Option<CellData> {
    match tag {
        Tag::Global(gid) => {
            context.state.find_cell(*gid)
                .map(|e| e.data.clone())
                .or_else(|| context.state.find_type(*gid).map(|e| e.data.clone()))
        }
        Tag::Local(name) => {
            source.find_local_cell(name).map(|e| e.data.clone())
        }
    }
}

// ---- Diagram interpretation ----

fn interpret_diagram(
    context: &Context,
    location: &Complex,
    diagram: &Node<DiagramDesc>,
) -> (Option<Diagram>, InterpResult) {
    match &diagram.value {
        DiagramDesc::Single(concat) => interpret_d_concat(context, location, concat),
        DiagramDesc::Paste { left, nat, right } => {
            interpret_diagram_paste(context, location, diagram.span.as_ref(), left, nat.value, right)
        }
    }
}

fn interpret_diagram_paste(
    context: &Context,
    location: &Complex,
    span: Option<&crate::helper::positions::Span>,
    left: &Node<DiagramDesc>,
    k: usize,
    right: &DConcat,
) -> (Option<Diagram>, InterpResult) {
    let (right_opt, right_result) = interpret_d_concat(context, location, right);
    match right_opt {
        None => (None, right_result),
        Some(d_right) => {
            let (left_opt, left_result) = interpret_diagram(&right_result.context, location, left);
            let combined = InterpResult::combine(right_result, left_result);
            match left_opt {
                None => (None, combined),
                Some(d_left) => {
                    match Diagram::paste(k, &d_left, &d_right) {
                        Ok(d) => (Some(d), combined),
                        Err(e) => {
                            let mut r = combined;
                            r.add_error(make_error_diag(
                                span_or(span),
                                format!("Failed to paste diagrams: {}", e),
                            ));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

fn interpret_d_concat(
    context: &Context,
    location: &Complex,
    concat: &DConcat,
) -> (Option<Diagram>, InterpResult) {
    match &concat.value {
        DConcatDesc::Single(expr) => {
            let (term_opt, result) = interpret_d_expr(context, location, expr);
            match term_opt {
                None => (None, result),
                Some(Term::DTerm(d)) => (Some(d), result),
                Some(Term::MTerm(_)) => {
                    let mut r = result;
                    r.add_error(make_error_diag(span_or(expr.span.as_ref()), "Not a diagram"));
                    (None, r)
                }
            }
        }
        DConcatDesc::Concat { left, right } => {
            interpret_concat_concat(context, location, concat.span.as_ref(), left, right)
        }
    }
}

fn interpret_concat_concat(
    context: &Context,
    location: &Complex,
    span: Option<&crate::helper::positions::Span>,
    left: &DConcat,
    right: &DExpr,
) -> (Option<Diagram>, InterpResult) {
    let (right_term_opt, right_result) = interpret_d_expr(context, location, right);
    match right_term_opt {
        None => (None, right_result),
        Some(Term::MTerm(_)) => {
            let mut r = right_result;
            r.add_error(make_error_diag(span_or(span), "Not a diagram"));
            (None, r)
        }
        Some(Term::DTerm(d_right)) => {
            let (left_opt, left_result) = interpret_d_concat(&right_result.context, location, left);
            let combined = InterpResult::combine(right_result, left_result);
            match left_opt {
                None => (None, combined),
                Some(d_left) => {
                    let k = (d_left.dim().max(0) as usize).min(d_right.dim().max(0) as usize).saturating_sub(1);
                    match Diagram::paste(k, &d_left, &d_right) {
                        Ok(d) => (Some(d), combined),
                        Err(e) => {
                            let mut r = combined;
                            r.add_error(make_error_diag(span_or(span),
                                format!("Failed to paste diagrams: {}", e)));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

fn interpret_d_expr(
    context: &Context,
    location: &Complex,
    d_expr: &DExpr,
) -> (Option<Term>, InterpResult) {
    match &d_expr.value {
        DExprDesc::Single(comp) => {
            let (comp_opt, result) = interpret_d_comp(context, location, comp);
            match comp_opt {
                None => (None, result),
                Some(Component::Hole) | Some(Component::Bd(_)) => {
                    let mut r = result;
                    r.add_error(make_error_diag(span_or(comp.span.as_ref()), "Not a diagram or map"));
                    (None, r)
                }
                Some(Component::Term(t)) => (Some(t), result),
            }
        }
        DExprDesc::Dot { left, right } => {
            let (left_opt, left_result) = interpret_d_expr(context, location, left);
            match left_opt {
                None => (None, left_result),
                Some(Term::DTerm(diagram)) => {
                    // .in or .out — extract boundary
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context, location, right
                    );
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Bd(sign)) => {
                            let k = (diagram.dim().max(0) as usize).saturating_sub(1);
                            match Diagram::boundary(sign, k, &diagram) {
                                Ok(bd) => (Some(Term::DTerm(bd)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error_diag(span_or(right.span.as_ref()), e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        Some(Component::Term(_)) | Some(Component::Hole) => {
                            let mut r = combined;
                            r.add_error(make_error_diag(span_or(right.span.as_ref()), "Not a well-formed diagram expression"));
                            (None, r)
                        }
                    }
                }
                Some(Term::MTerm(mc)) => {
                    // apply morphism to right operand
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context, &*mc.source, right
                    );
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Hole) | Some(Component::Bd(_)) => {
                            let mut r = combined;
                            r.add_error(make_error_diag(span_or(right.span.as_ref()), "Not a diagram or map"));
                            (None, r)
                        }
                        Some(Component::Term(Term::DTerm(d))) => {
                            match Morphism::apply(&mc.morphism, &d) {
                                Ok(d_img) => (Some(Term::DTerm(d_img)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error_diag(span_or(right.span.as_ref()), e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        Some(Component::Term(Term::MTerm(right_mc))) => {
                            let composed = Morphism::compose(&mc.morphism, &right_mc.morphism);
                            (Some(Term::MTerm(MorphismComponent { morphism: composed, source: right_mc.source })), combined)
                        }
                    }
                }
            }
        }
    }
}

fn interpret_d_comp(
    context: &Context,
    location: &Complex,
    d_comp: &DComp,
) -> (Option<Component>, InterpResult) {
    match &d_comp.value {
        DCompDesc::Mterm(m_term) => {
            let (mc_opt, result) = interpret_m_term(context, location, m_term);
            match mc_opt {
                None => (None, result),
                Some(mc) => (Some(Component::Term(Term::MTerm(mc))), result),
            }
        }
        DCompDesc::Dterm(d_term) => {
            let (d_opt, result) = interpret_d_term(context, location, d_term);
            match d_opt {
                None => (None, result),
                Some(d) => (Some(Component::Term(Term::DTerm(d))), result),
            }
        }
        DCompDesc::Name(name_node) => {
            let name = &name_node.value;
            let span = span_or(name_node.span.as_ref());
            let base_result = InterpResult::ok(context.clone());
            if let Some(diagram) = location.find_diagram(name) {
                return (Some(Component::Term(Term::DTerm(diagram.clone()))), base_result);
            }
            if let Some(entry) = location.find_morphism(name) {
                let source = match &entry.domain {
                    MorphismDomain::Type(id) => match context.state.find_type(*id) {
                        Some(te) => Arc::clone(&te.complex),
                        None => {
                            let mut r = base_result;
                            r.add_error(make_error_diag(span, format!("Type {} not found", id)));
                            return (None, r);
                        }
                    },
                    MorphismDomain::Module(mid) => match context.state.find_module_arc(mid) {
                        Some(m) => m,
                        None => {
                            let mut r = base_result;
                            r.add_error(make_error_diag(span, format!("Module `{}` not found", mid)));
                            return (None, r);
                        }
                    },
                };
                return (Some(Component::Term(Term::MTerm(MorphismComponent {
                    morphism: entry.morphism.clone(),
                    source,
                }))), base_result);
            }
            let mut r = base_result;
            r.add_error(make_error_diag(span, format!("Name `{}` not found", name)));
            (None, r)
        }
        DCompDesc::Bd(bd) => {
            (Some(Component::Bd(bd.value)), InterpResult::ok(context.clone()))
        }
        DCompDesc::Hole => {
            (Some(Component::Hole), InterpResult::ok(context.clone()))
        }
    }
}

fn interpret_d_term(
    context: &Context,
    location: &Complex,
    d_term: &DTerm,
) -> (Option<Diagram>, InterpResult) {
    match &d_term.value {
        DTermDesc::Indexed { diagram, nat, tail } => {
            interpret_diagram_paste(context, location, d_term.span.as_ref(), diagram, nat.value, tail)
        }
        DTermDesc::Pair { concat, expr } => {
            interpret_concat_concat(context, location, d_term.span.as_ref(), concat, expr)
        }
    }
}

// ---- Pasting ----

fn interpret_pasting(
    context: &Context,
    location: &Complex,
    pasting: &Pasting,
) -> (Option<Term>, InterpResult) {
    match &pasting.value {
        PastingDesc::Single(concat) => interpret_concat(context, location, concat),
        PastingDesc::Paste { left, nat, right } => {
            interpret_pasting_paste(context, location, pasting.span.as_ref(), left, nat.value, right)
        }
    }
}

fn interpret_pasting_paste(
    context: &Context,
    location: &Complex,
    span: Option<&crate::helper::positions::Span>,
    left: &Pasting,
    k: usize,
    right: &Concat,
) -> (Option<Term>, InterpResult) {
    let (right_opt, right_result) = interpret_concat(context, location, right);
    match right_opt {
        None => (None, right_result),
        Some(Term::MTerm(_)) => {
            let mut r = right_result;
            r.add_error(make_error_diag(span_or(span), "Not a diagram"));
            (None, r)
        }
        Some(Term::DTerm(d_right)) => {
            let (left_opt, left_result) = interpret_pasting(&right_result.context, location, left);
            let combined = InterpResult::combine(right_result, left_result);
            match left_opt {
                None => (None, combined),
                Some(Term::MTerm(_)) => {
                    let mut r = combined;
                    r.add_error(make_error_diag(span_or(span), "Not a diagram"));
                    (None, r)
                }
                Some(Term::DTerm(d_left)) => {
                    match Diagram::paste(k, &d_left, &d_right) {
                        Ok(d) => (Some(Term::DTerm(d)), combined),
                        Err(e) => {
                            let mut r = combined;
                            r.add_error(make_error_diag(span_or(span),
                                format!("Failed to paste diagrams: {}", e)));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

fn interpret_concat(
    context: &Context,
    location: &Complex,
    concat: &Concat,
) -> (Option<Term>, InterpResult) {
    match &concat.value {
        ConcatDesc::Single(expr) => interpret_d_expr(context, location, expr),
        ConcatDesc::Concat { left, right } => {
            let (right_opt, right_result) = interpret_d_expr(context, location, right);
            match right_opt {
                None => (None, right_result),
                Some(Term::MTerm(_)) => {
                    let mut r = right_result;
                    r.add_error(make_error_diag(span_or(right.span.as_ref()), "Not a diagram"));
                    (None, r)
                }
                Some(Term::DTerm(d_right)) => {
                    let (left_opt, left_result) = interpret_concat(&right_result.context, location, left);
                    let combined = InterpResult::combine(right_result, left_result);
                    match left_opt {
                        None => (None, combined),
                        Some(Term::MTerm(_)) => {
                            let mut r = combined;
                            r.add_error(make_error_diag(span_or(left.span.as_ref()), "Not a diagram"));
                            (None, r)
                        }
                        Some(Term::DTerm(d_left)) => {
                            let k = (d_left.dim().max(0) as usize)
                                .min(d_right.dim().max(0) as usize)
                                .saturating_sub(1);
                            match Diagram::paste(k, &d_left, &d_right) {
                                Ok(d) => (Some(Term::DTerm(d)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error_diag(span_or(concat.span.as_ref()),
                                        format!("Failed to paste diagrams: {}", e)));
                                    (None, r)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- Assert ----

fn interpret_assert(
    context: &Context,
    location: &Complex,
    assert_stmt: &AssertStatement,
) -> (Option<TermPair>, InterpResult) {
    let (left_opt, left_result) = interpret_pasting(context, location, &assert_stmt.value.left);
    match left_opt {
        None => (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) = interpret_pasting(&left_result.context, location, &assert_stmt.value.right);
            let combined = InterpResult::combine(left_result, right_result);
            match right_opt {
                None => (None, combined),
                Some(right_term) => {
                    match (left_term, right_term) {
                        (Term::DTerm(d1), Term::DTerm(d2)) => {
                            (Some(TermPair::DTermPair { fst: d1, snd: d2 }), combined)
                        }
                        (Term::MTerm(mc1), Term::MTerm(mc2)) => {
                            (Some(TermPair::MTermPair {
                                fst: mc1.morphism,
                                snd: mc2.morphism,
                                source: mc1.source,
                            }), combined)
                        }
                        _ => {
                            let span = span_or(assert_stmt.span.as_ref());
                            let mut r = combined;
                            r.add_error(make_error_diag(span, "The two sides of the equation are incomparable"));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

// ---- Boundaries ----

fn interpret_boundaries(
    context: &Context,
    location: &Complex,
    boundaries: &Boundaries,
) -> (Option<CellData>, InterpResult) {
    let (in_opt, src_result) = interpret_diagram(context, location, &boundaries.value.source);
    match in_opt {
        None => (None, src_result),
        Some(boundary_in) => {
            let (out_opt, tgt_result) = interpret_diagram(&src_result.context, location, &boundaries.value.target);
            let combined = InterpResult::combine(src_result, tgt_result);
            match out_opt {
                None => (None, combined),
                Some(boundary_out) => {
                    (Some(CellData::Boundary { boundary_in, boundary_out }), combined)
                }
            }
        }
    }
}

// ---- Diagram naming ----

fn interpret_dnamer(
    context: &Context,
    location: &Complex,
    dnamer: &Dnamer,
) -> (Option<(LocalId, Diagram)>, InterpResult) {
    let (diag_opt, diag_result) = interpret_diagram(context, location, &dnamer.value.body);
    match diag_opt {
        None => (None, diag_result),
        Some(diagram) => {
            let name = dnamer.value.name.value.clone();
            let context_after = diag_result.context.clone();

            match &dnamer.value.boundaries {
                None => (Some((name, diagram)), diag_result),
                Some(bounds) => {
                    let (bounds_opt, bounds_result) = interpret_boundaries(
                        &context_after, location, bounds
                    );
                    let combined = InterpResult::combine(diag_result, bounds_result);
                    match bounds_opt {
                        None => (None, combined),
                        Some(CellData::Zero) => {
                            (Some((name, diagram)), combined)
                        }
                        Some(CellData::Boundary { boundary_in, boundary_out }) => {
                            let bound_span = span_or(bounds.span.as_ref());
                            let dim = diagram.dim();
                            let dim = if dim <= 0 { 0 } else { dim as usize - 1 };

                            let check_boundary = |sign: DiagramSign, expected: &Diagram| -> Result<(), String> {
                                let actual = Diagram::boundary_normal(sign, dim, &diagram)
                                    .map_err(|e| e.to_string())?;
                                if Diagram::isomorphic(&actual, expected) { Ok(()) }
                                else {
                                    let side = match sign { DiagramSign::Input => "input", DiagramSign::Output => "output" };
                                    Err(format!("Diagram does not match {} boundary annotation", side))
                                }
                            };

                            let mut r = combined;
                            if let Err(msg) = check_boundary(DiagramSign::Input, &boundary_in) {
                                r.add_error(make_error_diag(bound_span, msg));
                                return (None, r);
                            }
                            if let Err(msg) = check_boundary(DiagramSign::Output, &boundary_out) {
                                r.add_error(make_error_diag(bound_span, msg));
                                return (None, r);
                            }
                            (Some((name, diagram)), r)
                        }
                    }
                }
            }
        }
    }
}

// ---- Morphism naming ----

fn interpret_mnamer(
    context: &Context,
    location: &Complex,
    mnamer: &Mnamer,
) -> (Option<(LocalId, Morphism, MorphismDomain)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &mnamer.value.address);
    match id_opt {
        None => (None, addr_result),
        Some(id) => {
            let context_after = addr_result.context.clone();
            let source = match context_after.state.find_type(id) {
                None => {
                    let mut r = addr_result;
                    r.add_error(make_error_diag(span_or(mnamer.value.address.span.as_ref()),
                        format!("Type {} not found", id)));
                    return (None, r);
                }
                Some(te) => (*te.complex).clone(),
            };
            let (m_opt, m_result) = interpret_m_def(&context_after, location, &source, &mnamer.value.definition);
            let combined = InterpResult::combine(addr_result, m_result);
            match m_opt {
                None => (None, combined),
                Some(morphism) => {
                    let name = mnamer.value.name.value.clone();
                    (Some((name, morphism, MorphismDomain::Type(id))), combined)
                }
            }
        }
    }
}

// ---- Identity morphism ----

fn identity_morphism(context: &Context, domain: &Complex) -> Morphism {
    let entries: Vec<(Tag, usize, CellData, Diagram)> = domain.generator_names()
        .into_iter()
        .filter_map(|name| {
            let gen_entry = domain.find_generator(&name)?;
            let tag = gen_entry.tag.clone();
            let dim = gen_entry.dim;
            let cell_data = match &tag {
                Tag::Global(gid) => {
                    context.state.find_cell(*gid).map(|e| e.data.clone())
                        .or_else(|| context.state.find_type(*gid).map(|e| e.data.clone()))?
                }
                Tag::Local(local_name) => {
                    domain.find_local_cell(local_name).map(|e| e.data.clone())?
                }
            };
            let image = domain.classifier(&name)?.clone();
            Some((tag, dim, cell_data, image))
        })
        .collect();
    Morphism::of_entries(entries, true)
}
