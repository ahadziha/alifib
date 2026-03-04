#![allow(dead_code)]

use std::sync::Arc;
use crate::aux::{self, GlobalId, LocalId, ModuleId, Tag};
use crate::aux::path;
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign as DiagramSign},
    map::PMap,
};
use super::state::State;
use crate::language::{
    self,
    ast::{self, Span, Spanned, Program, Block, TypeInst, IncludeModule,
          CInstr, NameWithBoundary, LetDiag, DefPMap, LocalInst, AssertStmt,
          PMapBasic, PMSystem, PMapClause, DExpr, DComponent, Address},
    error::Error,
};

// ---- Context ----

#[derive(Debug, Clone)]
pub struct Context {
    pub current_module: ModuleId,
    pub state: Arc<State>,
}

impl Context {
    pub fn new(module_id: ModuleId, state: State) -> Self {
        Self { current_module: module_id, state: Arc::new(state) }
    }

    pub fn new_sharing_state(module_id: ModuleId, other: &Context) -> Self {
        Self { current_module: module_id, state: Arc::clone(&other.state) }
    }

    pub fn with_state(&self, state: State) -> Self {
        Self { current_module: self.current_module.clone(), state: Arc::new(state) }
    }

    /// Get a mutable reference to the state via Arc::make_mut (copy-on-write).
    pub fn state_mut(&mut self) -> &mut State {
        Arc::make_mut(&mut self.state)
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
    pub read_file: Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>,
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

#[derive(Debug, Clone)]
pub struct InterpResult {
    pub context: Context,
    pub errors: Vec<Error>,
}

impl InterpResult {
    fn ok(context: Context) -> Self {
        Self { context, errors: vec![] }
    }

    fn add_error(&mut self, err: Error) {
        self.errors.push(err);
    }

    fn combine(prev: InterpResult, next: InterpResult) -> InterpResult {
        let mut errors = prev.errors;
        errors.extend(next.errors);
        InterpResult { context: next.context, errors }
    }

    fn has_errors(&self) -> bool {
        !self.errors.is_empty()
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
pub struct MapComponent {
    pub map: PMap,
    pub source: Arc<Complex>,
}

#[derive(Debug, Clone)]
pub enum Term {
    MTerm(MapComponent),
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
    MTermPair { fst: PMap, snd: PMap, source: Arc<Complex> },
    DTermPair { fst: Diagram, snd: Diagram },
}

// ---- Error helpers ----

fn unknown_span() -> Span {
    Span { start: 0, end: 0 }
}

fn make_error(span: Span, message: impl Into<String>) -> Error {
    Error::Runtime { message: message.into(), span }
}

// ---- Session ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    LoadError,
    ParserError,
    InterpreterError,
    Success,
}

#[derive(Debug, Clone)]
pub struct SessionResult {
    pub context: Context,
    pub errors: Vec<Error>,
    pub status: SessionStatus,
    pub source: String,
    pub filename: String,
}

pub struct Loader {
    inner: FileLoader,
}

impl Loader {
    fn path_separator() -> char {
        if cfg!(windows) { ';' } else { ':' }
    }

    fn split_paths(value: &str) -> Vec<String> {
        value.split(Self::path_separator())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_owned())
            .collect()
    }

    fn env_search_paths() -> Vec<String> {
        match std::env::var("ALIFIB_PATH") {
            Ok(value) if !value.is_empty() => Self::split_paths(&value),
            _ => vec![],
        }
    }

    pub fn make(
        search_paths: Vec<String>,
        read_file: Option<Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>>,
    ) -> Self {
        let read_file = read_file.unwrap_or_else(|| Arc::new(FileLoader::default_read));
        let search_paths = path::normalize_search_paths(search_paths);
        Self { inner: FileLoader { search_paths, read_file } }
    }

    pub fn default(extra_search_paths: Vec<String>) -> Self {
        let cwd = path::canonicalize(&std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_owned()));
        let env_paths = Self::env_search_paths();
        let combined = std::iter::once(cwd)
            .chain(env_paths)
            .chain(extra_search_paths)
            .collect();
        Self::make(combined, None)
    }

    pub fn file_loader(&self) -> &FileLoader {
        &self.inner
    }
}

fn ensure_root_in_loader(loader: &FileLoader, canonical_path: &str) -> FileLoader {
    let root = std::path::Path::new(canonical_path)
        .parent()
        .and_then(|p| p.to_str())
        .map(path::canonicalize)
        .unwrap_or_else(|| canonical_path.to_owned());
    let mut desired = vec![root];
    desired.extend(loader.search_paths.iter().cloned());
    let normalized = path::normalize_search_paths(desired);
    if normalized == loader.search_paths {
        loader.clone()
    } else {
        FileLoader {
            search_paths: normalized,
            read_file: loader.read_file.clone(),
        }
    }
}

pub fn run(loader: &Loader, path: &str) -> SessionResult {
    let canonical_path = path::canonicalize(path);
    let module_id = canonical_path.clone();
    let base_context = Context::new(module_id, State::empty());

    let file_loader = loader.file_loader();
    let contents = match (file_loader.read_file)(&canonical_path) {
        Err(LoadError::NotFound) => {
            return SessionResult {
                context: base_context,
                errors: vec![],
                status: SessionStatus::LoadError,
                source: String::new(),
                filename: canonical_path,
            };
        }
        Err(LoadError::IoError(reason)) => {
            eprintln!("error: could not load `{}`: {}", path, reason);
            return SessionResult {
                context: base_context,
                errors: vec![],
                status: SessionStatus::LoadError,
                source: String::new(),
                filename: canonical_path,
            };
        }
        Ok(s) => s,
    };

    let file_loader = ensure_root_in_loader(file_loader, &canonical_path);

    // Parse
    let program = match language::parse(&contents) {
        Ok(p) => p,
        Err(parse_errors) => {
            return SessionResult {
                context: base_context,
                errors: parse_errors,
                status: SessionStatus::ParserError,
                source: contents,
                filename: canonical_path,
            };
        }
    };

    // Interpret
    let interp_result = interpret_program(&file_loader, base_context, &program);
    let status = if interp_result.has_errors() {
        SessionStatus::InterpreterError
    } else {
        SessionStatus::Success
    };

    SessionResult {
        context: interp_result.context,
        errors: interp_result.errors,
        status,
        source: contents,
        filename: canonical_path,
    }
}

// ---- Main interpreter ----

pub fn interpret_program(
    loader: &FileLoader,
    mut context: Context,
    program: &Program,
) -> InterpResult {
    let module_id = context.current_module.clone();

    // If the module is already loaded, skip
    if context.state.find_module(&module_id).is_some() {
        return InterpResult::ok(context);
    }

    // Initialize module complex with anonymous root type
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
        let block_result = interpret_block(loader, result.context.clone(), block);
        result = InterpResult::combine(result, block_result);
    }
    result
}

fn interpret_block(loader: &FileLoader, context: Context, block: &Spanned<Block>) -> InterpResult {
    match &block.inner {
        Block::TypeBlock(body) => interpret_block_type(loader, context, body),
        Block::LocalBlock { complex, body } => {
            interpret_block_complex(loader, context, complex, body)
        }
    }
}

fn interpret_block_type(
    loader: &FileLoader,
    context: Context,
    body: &[Spanned<TypeInst>],
) -> InterpResult {
    let mut result = InterpResult::ok(context);
    let (_loc, type_result) = interpret_type_block(loader, &result.context, body);
    result = InterpResult::combine(result, type_result);
    result
}

fn interpret_type_block(
    loader: &FileLoader,
    context: &Context,
    body: &[Spanned<TypeInst>],
) -> (Option<Complex>, InterpResult) {
    let mut acc_result = InterpResult::ok(context.clone());
    let mut any_location: Option<Complex> = None;

    for instr in body {
        let ctx = acc_result.context.clone();
        let (loc_opt, instr_result) = interpret_type_inst(loader, &ctx, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
        if let Some(new_loc) = loc_opt {
            any_location = Some(new_loc);
        }
    }

    (any_location, acc_result)
}

fn interpret_type_inst(
    loader: &FileLoader,
    context: &Context,
    instr: &Spanned<TypeInst>,
) -> (Option<Complex>, InterpResult) {
    match &instr.inner {
        TypeInst::Generator(generator) => interpret_generator_type(context, generator),
        TypeInst::LetDiag(ld) => {
            let module_id = &context.current_module;
            let module_location = context.state.find_module(module_id).cloned().unwrap_or_default();
            let (out, result) = interpret_let_diag(context, &module_location, ld);
            match out {
                None => (None, result),
                Some((name, diagram)) => {
                    let module_id2 = result.context.current_module.clone();
                    let mut current_loc = result.context.state.find_module(&module_id2).cloned().unwrap_or_default();
                    current_loc.add_diagram(name, diagram);
                    let mut r = result;
                    r.context.state_mut().set_module(module_id2, current_loc.clone());
                    (Some(current_loc), r)
                }
            }
        }
        TypeInst::DefPMap(dp) => {
            let module_id = &context.current_module;
            let module_location = context.state.find_module(module_id).cloned().unwrap_or_default();
            let (out, result) = interpret_def_pmap(context, &module_location, dp);
            match out {
                None => (None, result),
                Some((name, map, domain)) => {
                    let module_id2 = result.context.current_module.clone();
                    let mut current_loc = result.context.state.find_module(&module_id2).cloned().unwrap_or_default();
                    current_loc.add_map(name, domain, map);
                    let mut r = result;
                    r.context.state_mut().set_module(module_id2, current_loc.clone());
                    (Some(current_loc), r)
                }
            }
        }
        TypeInst::IncludeModule(include_mod) => {
            interpret_include_module_instr(loader, context, include_mod, instr.span)
        }
    }
}

fn interpret_generator_type(
    context: &Context,
    generator: &ast::Generator,
) -> (Option<Complex>, InterpResult) {
    let name_with_bd = &generator.name;
    let def = &generator.complex;

    let name = name_with_bd.inner.name.inner.clone();
    let name_span = name_with_bd.inner.name.span;

    let module_id = &context.current_module;
    let module_location = match context.state.find_module(module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(name_span, "Module not found"));
            return (None, result);
        }
        Some(m) => m.clone()
    };

    if module_location.name_in_use(&name) {
        let mut result = InterpResult::ok(context.clone());
        result.add_error(make_error(name_span,
            format!("Generator name already in use: {}", name)));
        return (None, result);
    }

    let (boundaries, mut result) = match &name_with_bd.inner.boundary {
        None => (CellData::Zero, InterpResult::ok(context.clone())),
        Some(bounds) => {
            let (bopt, r) = interpret_boundaries(context, &module_location, bounds);
            match bopt {
                None => return (None, r),
                Some(b) => (b, r),
            }
        }
    };

    if !matches!(boundaries, CellData::Zero) {
        result.add_error(make_error(name_span,
            "Higher cells in @Type blocks are not supported"));
        return (None, result);
    }

    let context_after = result.context.clone();
    let (ns_opt, complex_result) = interpret_complex(&context_after, Mode::Global, def);
    result = InterpResult::combine(result, complex_result);

    let mut definition_complex = match ns_opt {
        None => return (None, result),
        Some(ns) => ns.location,
    };

    let context_after = result.context.clone();
    let module_id2 = &context_after.current_module;
    let mut module_location_now = match context_after.state.find_module(module_id2) {
        None => {
            result.add_error(make_error(name_span, "Module not found after processing definition"));
            return (None, result);
        }
        Some(m) => m.clone()
    };

    let new_id = GlobalId::fresh();
    let tag = Tag::Global(new_id);
    let classifier = match Diagram::cell(tag, &CellData::Zero) {
        Ok(d) => d,
        Err(e) => {
            result.add_error(make_error(name_span,
                format!("Failed to create generator cell: {}", e)));
            return (None, result);
        }
    };

    let identity = identity_map(&context_after, &definition_complex);
    definition_complex.add_map(
        name.clone(),
        MapDomain::Type(new_id),
        identity,
    );

    module_location_now.add_generator(name.clone(), classifier.clone());
    module_location_now.add_diagram(name.clone(), classifier);

    {
        let s = result.context.state_mut();
        s.set_type(new_id, CellData::Zero, definition_complex);
        s.set_module(module_id2.clone(), module_location_now.clone());
    }

    (Some(module_location_now), result)
}

fn interpret_include_module_instr(
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
    let included_module_id: ModuleId = canonical_path.clone();
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

fn interpret_block_complex(
    _loader: &FileLoader,
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
    namespace: &Namespace,
    body: &[Spanned<LocalInst>],
) -> (Option<Complex>, InterpResult) {
    let mut current_ns = namespace.clone();
    let mut acc_result = InterpResult::ok(context.clone());

    for instr in body {
        let ctx = acc_result.context.clone();
        let (loc_opt, instr_result) = interpret_local_inst(&ctx, &current_ns, instr);
        acc_result = InterpResult::combine(acc_result, instr_result);
        if let Some(new_loc) = loc_opt {
            current_ns = Namespace { root: current_ns.root, location: new_loc };
        }
        if acc_result.has_errors() {
            break;
        }
    }

    (Some(current_ns.location), acc_result)
}

fn interpret_local_inst(
    context: &Context,
    namespace: &Namespace,
    instr: &Spanned<LocalInst>,
) -> (Option<Complex>, InterpResult) {
    let root = namespace.root;
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
                    r.context.state_mut().modify_type_complex(root, |c| c.add_diagram(name, diagram));
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
                    r.context.state_mut().modify_type_complex(root, |c| c.add_map(name, domain, map));
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

fn check_assert(
    _context: &Context,
    _location: &Complex,
    pair: &TermPair,
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
                let in_first = fst.is_defined_at(tag);
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

// ---- Complex resolution ----

fn interpret_complex(
    context: &Context,
    mode: Mode,
    complex: &Spanned<ast::Complex>,
) -> (Option<Namespace>, InterpResult) {
    let module_id = &context.current_module;
    let complex_span = complex.span;

    let module_space = match context.state.find_module(module_id) {
        None => {
            let mut result = InterpResult::ok(context.clone());
            result.add_error(make_error(complex_span,
                format!("Module `{}` not found", module_id)));
            return (None, result);
        }
        Some(m) => m.clone(),
    };

    let empty_name: LocalId = String::new();

    match &complex.inner {
        ast::Complex::Address(addr) => {
            // Just an address, no block
            if addr.is_empty() {
                // Use root
                match module_space.find_generator(&empty_name) {
                    None => {
                        let mut r = InterpResult::ok(context.clone());
                        r.add_error(make_error(complex_span, "Root generator not found"));
                        (None, r)
                    }
                    Some(entry) => match &entry.tag {
                        Tag::Global(id) => {
                            let root = *id;
                            let type_entry = match context.state.find_type(root) {
                                None => {
                                    let mut r = InterpResult::ok(context.clone());
                                    r.add_error(make_error(complex_span,
                                        format!("Type {} not found", root)));
                                    return (None, r);
                                }
                                Some(te) => te.clone(),
                            };
                            let location = (*type_entry.complex).clone();
                            let ns = Namespace { root, location };
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
                let root = match root_opt {
                    None => return (None, result),
                    Some(r) => r,
                };
                let type_entry = match result.context.state.find_type(root) {
                    None => {
                        result.add_error(make_error(complex_span,
                            format!("Type {} not found in global record", root)));
                        return (None, result);
                    }
                    Some(te) => te.clone(),
                };
                let location = (*type_entry.complex).clone();
                let ns = Namespace { root, location };
                (Some(ns), result)
            }
        }
        ast::Complex::Block { address, body } => {
            // Resolve root from address (or use empty name)
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
            let root = match root_opt {
                None => return (None, result),
                Some(r) => r,
            };

            let type_entry = match result.context.state.find_type(root) {
                None => {
                    result.add_error(make_error(complex_span,
                        format!("Type {} not found in global record", root)));
                    return (None, result);
                }
                Some(te) => te.clone(),
            };

            let initial_location = (*type_entry.complex).clone();

            // Process block
            let (location_opt, block_result) = interpret_c_block(
                &result.context, mode, &initial_location, body
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
    body: &[Spanned<CInstr>],
) -> (Option<Complex>, InterpResult) {
    let mut current_location: Complex = initial_location.clone();
    let mut current_context: Context = context.clone();
    let mut acc_errors: Vec<Error> = Vec::new();

    for instr in body {
        let (new_location, instr_result) =
            interpret_c_instr(current_context, mode, current_location, instr);
        current_location = new_location;
        current_context = instr_result.context;
        acc_errors.extend(instr_result.errors);
    }

    let acc_result = InterpResult { context: current_context, errors: acc_errors };
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

fn interpret_include_instr(
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

fn interpret_attach_instr(
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

// ---- Address resolution ----

fn interpret_address(
    context: &Context,
    address: &Address,
    addr_span: Span,
) -> (Option<GlobalId>, InterpResult) {
    let module_id = &context.current_module;

    let module_space = match context.state.find_module(module_id) {
        None => {
            let mut r = InterpResult::ok(context.clone());
            r.add_error(make_error(addr_span,
                format!("Module `{}` not found", module_id)));
            return (None, r);
        }
        Some(m) => m.clone(),
    };

    let segments: Vec<(Span, String)> = address.iter()
        .map(|n| (n.span, n.inner.clone()))
        .collect();

    let empty_name: LocalId = String::new();
    let base_result = InterpResult::ok(context.clone());

    if segments.is_empty() {
        return match module_space.find_generator(&empty_name) {
            None => {
                let mut r = base_result;
                r.add_error(make_error(addr_span, "Root generator not found"));
                (None, r)
            }
            Some(entry) => match &entry.tag {
                Tag::Global(id) => (Some(*id), base_result),
                Tag::Local(_) => {
                    let mut r = base_result;
                    r.add_error(make_error(addr_span, "Root has local tag"));
                    (None, r)
                }
            }
        };
    }

    let last_idx = segments.len() - 1;
    let prefix = &segments[..last_idx];
    let (last_span, last_name) = &segments[last_idx];

    let mut current_space = module_space.clone();
    for (seg_span, seg_name) in prefix {
        match current_space.find_map(seg_name) {
            None => {
                let mut r = base_result;
                r.add_error(make_error(*seg_span,
                    format!("Partial map `{}` not found", seg_name)));
                return (None, r);
            }
            Some(me) => {
                match &me.domain {
                    MapDomain::Module(mid) => {
                        match context.state.find_module(mid) {
                            Some(m) => current_space = m.clone(),
                            None => {
                                let mut r = base_result;
                                r.add_error(make_error(*seg_span,
                                    format!("Module `{}` not found", mid)));
                                return (None, r);
                            }
                        }
                    }
                    MapDomain::Type(_) => {
                        let mut r = base_result;
                        r.add_error(make_error(*seg_span,
                            format!("Domain of `{}` is not a module", seg_name)));
                        return (None, r);
                    }
                }
            }
        }
    }

    match current_space.find_diagram(last_name) {
        None => {
            let mut r = base_result;
            r.add_error(make_error(*last_span,
                format!("Type `{}` not found", last_name)));
            (None, r)
        }
        Some(diagram) => {
            if !diagram.is_cell() {
                let mut r = base_result;
                r.add_error(make_error(*last_span,
                    format!("`{}` is not a cell", last_name)));
                return (None, r);
            }
            let d = if diagram.dim() < 0 { 0 } else { diagram.dim() as usize };
            match diagram.labels.get(d).and_then(|row| row.first()) {
                None => {
                    let mut r = base_result;
                    r.add_error(make_error(*last_span, "Cell has no top label"));
                    (None, r)
                }
                Some(Tag::Global(id)) => (Some(*id), base_result),
                Some(Tag::Local(_)) => {
                    let mut r = base_result;
                    r.add_error(make_error(*last_span, "Cell has local tag (unexpected)"));
                    (None, r)
                }
            }
        }
    }
}

// ---- Include / Attach helpers ----

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

// ---- PMap interpretation ----

fn interpret_pmap(
    context: &Context,
    location: &Complex,
    source: &Complex,
    pmap: &Spanned<ast::PMap>,
) -> (Option<MapComponent>, InterpResult) {
    match &pmap.inner {
        ast::PMap::Basic(basic) => interpret_pmap_basic(context, location, source, basic, pmap.span),
        ast::PMap::Dot { base, rest } => {
            let (base_opt, base_result) = interpret_pmap_basic(context, location, source, base, pmap.span);
            match base_opt {
                None => (None, base_result),
                Some(base_comp) => {
                    let (rest_opt, rest_result) = interpret_pmap(
                        &base_result.context, &*base_comp.source, source, rest
                    );
                    let combined = InterpResult::combine(base_result, rest_result);
                    match rest_opt {
                        None => (None, combined),
                        Some(rest_comp) => {
                            let composed = PMap::compose(&base_comp.map, &rest_comp.map);
                            (Some(MapComponent { map: composed, source: rest_comp.source }), combined)
                        }
                    }
                }
            }
        }
    }
}

fn interpret_pmap_basic(
    context: &Context,
    location: &Complex,
    _source: &Complex,
    basic: &PMapBasic,
    span: Span,
) -> (Option<MapComponent>, InterpResult) {
    match basic {
        PMapBasic::Name(name) => {
            let base_result = InterpResult::ok(context.clone());
            match location.find_map(name) {
                None => {
                    let mut r = base_result;
                    r.add_error(make_error(span, format!("Partial map not found: `{}`", name)));
                    (None, r)
                }
                Some(entry) => {
                    let source = match &entry.domain {
                        MapDomain::Type(id) => {
                            match context.state.find_type(*id) {
                                Some(te) => Arc::clone(&te.complex),
                                None => {
                                    let mut r = base_result;
                                    r.add_error(make_error(span, format!("Type {} not found", id)));
                                    return (None, r);
                                }
                            }
                        }
                        MapDomain::Module(mid) => {
                            match context.state.find_module_arc(mid) {
                                Some(m) => m,
                                None => {
                                    let mut r = base_result;
                                    r.add_error(make_error(span, format!("Module `{}` not found", mid)));
                                    return (None, r);
                                }
                            }
                        }
                    };
                    (Some(MapComponent { map: entry.map.clone(), source }), base_result)
                }
            }
        }
        PMapBasic::System(pm_system) => {
            interpret_pm_system(context, location, _source, pm_system, span)
        }
    }
}

fn interpret_pm_system(
    context: &Context,
    location: &Complex,
    source: &Complex,
    pm_system: &PMSystem,
    span: Span,
) -> (Option<MapComponent>, InterpResult) {
    // Start with prefix map or empty map
    let (initial_mc, prefix_result) = match &pm_system.extend {
        None => {
            let map = PMap::empty().unwrap();
            (MapComponent { map, source: Arc::new(source.clone()) }, InterpResult::ok(context.clone()))
        }
        Some(prefix) => {
            let (mc_opt, r) = interpret_pmap(context, location, source, prefix);
            match mc_opt {
                None => return (None, r),
                Some(mc) => (mc, r),
            }
        }
    };

    // Apply each clause
    let mut current_map = initial_mc.map;
    let effective_source = &*initial_mc.source;
    let mut acc_result = prefix_result;

    for clause in &pm_system.clauses {
        let ctx = acc_result.context.clone();
        let (m_opt, clause_result) = interpret_pm_clause(
            &ctx, location, effective_source, current_map, clause, span
        );
        acc_result = InterpResult::combine(acc_result, clause_result);
        match m_opt {
            None => return (None, acc_result),
            Some(new_m) => current_map = new_m,
        }
        if acc_result.has_errors() {
            return (Some(MapComponent { map: current_map, source: initial_mc.source }), acc_result);
        }
    }

    (Some(MapComponent { map: current_map, source: initial_mc.source }), acc_result)
}

fn interpret_pm_clause(
    context: &Context,
    location: &Complex,
    source: &Complex,
    map: PMap,
    clause: &Spanned<PMapClause>,
    _span: Span,
) -> (Option<PMap>, InterpResult) {
    let (left_opt, left_result) = interpret_diagram_as_term(context, source, &clause.inner.lhs);
    match left_opt {
        None => return (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) = interpret_diagram_as_term(&left_result.context, location, &clause.inner.rhs);
            let combined = InterpResult::combine(left_result, right_result);
            match right_opt {
                None => (None, combined),
                Some(right_term) => {
                    match interpret_assign(
                        &combined.context,
                        map,
                        source,
                        location,
                        &left_term,
                        &right_term,
                        clause.span,
                    ) {
                        Ok(new_m) => (Some(new_m), combined),
                        Err(e) => {
                            let mut r = combined;
                            r.add_error(make_error(clause.span, e.to_string()));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

/// Handle assignment of a term to another term in a map clause.
/// Supports both diagram-to-diagram (via smart_extend) and map-to-map assignments.
fn interpret_assign(
    context: &Context,
    map: PMap,
    source: &Complex,
    target: &Complex,
    left: &Term,
    right: &Term,
    span: Span,
) -> Result<PMap, aux::Error> {
    match (left, right) {
        (Term::DTerm(d_left), Term::DTerm(d_right)) => {
            smart_extend(context, map, source, target, d_left, d_right, span)
        }
        (Term::MTerm(mc_left), Term::MTerm(mc_right)) => {
            if !Arc::ptr_eq(&mc_left.source, &mc_right.source) {
                return Err(aux::Error::new("Not a well-formed assignment"));
            }
            let src_complex = &*mc_left.source;
            let mut generators: Vec<(usize, Tag, LocalId)> = src_complex
                .generator_names()
                .into_iter()
                .filter_map(|name| {
                    src_complex.find_generator(&name).map(|entry| {
                        (entry.dim, entry.tag.clone(), name)
                    })
                })
                .collect();
            generators.sort_by_key(|(dim, _, _)| *dim);

            let mut extended = map;
            for (_dim, tag, name) in &generators {
                let defined_left = mc_left.map.is_defined_at(tag);
                let defined_right = mc_right.map.is_defined_at(tag);
                if defined_left && defined_right {
                    let left_image = mc_left.map.image(tag)?;
                    if left_image.is_cell() {
                        let right_image = mc_right.map.image(tag)?;
                        extended = smart_extend(
                            context, extended, source, target,
                            left_image, right_image, span,
                        )?;
                    } else {
                        // Non-cell left image: check all its labels are already defined
                        let all_defined = left_image.labels.iter()
                            .flat_map(|row| row.iter())
                            .all(|t| extended.is_defined_at(t));
                        if !all_defined {
                            return Err(aux::Error::new(
                                "Failed to extend map (not enough information)",
                            ));
                        }
                    }
                } else if defined_left && !defined_right {
                    return Err(aux::Error::new(format!(
                        "{} is in the domain of definition of the first map, but not the second map",
                        name
                    )));
                } else if defined_right && !defined_left {
                    return Err(aux::Error::new(format!(
                        "{} is in the domain of definition of the second map, but not the first map",
                        name
                    )));
                }
                // else: neither defined, skip
            }
            Ok(extended)
        }
        _ => Err(aux::Error::new("Not a well-formed assignment")),
    }
}

/// Smart extension of a map: adds a mapping from a source cell to a target diagram,
/// recursively extending for boundary cells as needed.
fn smart_extend(
    context: &Context,
    map: PMap,
    source: &Complex,
    target: &Complex,
    source_diag: &Diagram,
    target_diag: &Diagram,
    span: Span,
) -> Result<PMap, aux::Error> {
    if !source_diag.is_cell() {
        return Err(aux::Error::new("Left-hand side of map instruction must be a cell"));
    }
    let d = if source_diag.dim() < 0 { 0 } else { source_diag.dim() as usize };
    let tag = source_diag.labels.get(d).and_then(|r| r.first())
        .ok_or_else(|| aux::Error::new("Source cell has no top label"))?
        .clone();

    if map.is_defined_at(&tag) {
        let current = map.image(&tag)?;
        if Diagram::isomorphic(current, target_diag) {
            return Ok(map);
        } else {
            return Err(aux::Error::new("The same generator is mapped to multiple diagrams"));
        }
    }

    let cell_data = get_cell_data(context, source, &tag)
        .ok_or_else(|| aux::Error::new("Cannot find cell data for generator"))?;

    let dim = if source_diag.dim() < 0 { 0 } else { source_diag.dim() as usize };

    let missing = match &cell_data {
        CellData::Zero => vec![],
        CellData::Boundary { boundary_in, boundary_out } => {
            let mut missing = vec![];
            for (bd, sign) in &[(boundary_in, DiagramSign::Input), (boundary_out, DiagramSign::Output)] {
                let bd_d = if bd.dim() < 0 { 0 } else { bd.dim() as usize };
                if let Some(row) = bd.labels.get(bd_d) {
                    for t in row {
                        if !map.is_defined_at(t) {
                            missing.push((t.clone(), *sign));
                        }
                    }
                }
            }
            missing
        }
    };

    let mut current = map;

    for (focus, sign) in &missing {
        if current.is_defined_at(focus) {
            continue;
        }
        let dim_minus_one = dim - 1;
        let cell_data_focus = get_cell_data(context, source, focus)
            .ok_or_else(|| aux::Error::new(format!("Cannot find cell data for boundary cell {}", focus)))?;

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
            current = smart_extend(context, current, source, target, sub_source, &target_boundary, span)?;
        } else {
            match crate::core::diagram::isomorphism_of(&source_boundary.shape, &target_boundary.shape) {
                Err(_) => return Err(aux::Error::new("Failed to extend map (boundary shapes don't match)")),
                Ok(embedding) => {
                    let bd_d = if source_boundary.dim() < 0 { 0 } else { source_boundary.dim() as usize };
                    let bd_labels = &source_boundary.labels;
                    let target_labels = &target_boundary.labels;
                    let embed_map = &embedding.map;

                    let mut image_tag: Option<Tag> = None;
                    let mut consistent = true;

                    if let Some(row) = bd_labels.get(bd_d) {
                        if let Some(map_row) = embed_map.get(bd_d) {
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
                        return Err(aux::Error::new("The same generator is mapped to multiple diagrams"));
                    }

                    let mapped_tag = image_tag
                        .ok_or_else(|| aux::Error::new("Failed to extend map (no image found)"))?;

                    let gen_name = target.find_generator_by_tag(&mapped_tag)
                        .ok_or_else(|| aux::Error::new("Image tag not found in target complex"))?
                        .clone();
                    let d_focus = target.classifier(&gen_name)
                        .ok_or_else(|| aux::Error::new("Classifier not found for image generator"))?
                        .clone();

                    let focus_source = match source_boundary.labels.get(bd_d).and_then(|r| {
                        r.iter().position(|t| t == focus)
                    }) {
                        Some(_) => Diagram::cell(focus.clone(), &cell_data_focus)?,
                        None => continue,
                    };

                    current = smart_extend(context, current, source, target, &focus_source, &d_focus, span)?;
                }
            }
        }
    }

    PMap::extend(current, tag, dim, cell_data, target_diag.clone())
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
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Diagram>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::Principal(exprs) => interpret_principal(context, location, exprs, diagram.span),
        ast::Diagram::Paste { lhs, dim, rhs } => {
            let k = match dim.inner.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error(dim.span,
                        format!("Invalid paste dimension: {}", dim.inner)));
                    return (None, r);
                }
            };
            interpret_diagram_paste(context, location, diagram.span, lhs, k, rhs)
        }
    }
}

fn interpret_principal(
    context: &Context,
    location: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Diagram>, InterpResult) {
    if exprs.is_empty() {
        let mut r = InterpResult::ok(context.clone());
        r.add_error(make_error(span, "Empty diagram expression"));
        return (None, r);
    }

    // Interpret first expression
    let (first_opt, first_result) = interpret_d_expr(context, location, &exprs[0]);
    match first_opt {
        None => return (None, first_result),
        Some(Term::MTerm(_)) if exprs.len() == 1 => {
            let mut r = first_result;
            r.add_error(make_error(exprs[0].span, "Not a diagram"));
            return (None, r);
        }
        Some(Term::MTerm(_)) => {
            let mut r = first_result;
            r.add_error(make_error(exprs[0].span, "Not a diagram"));
            return (None, r);
        }
        Some(Term::DTerm(d_first)) => {
            if exprs.len() == 1 {
                return (Some(d_first), first_result);
            }

            // Fold left-to-right
            let mut acc = d_first;
            let mut result = first_result;

            for expr in &exprs[1..] {
                let (term_opt, expr_result) = interpret_d_expr(&result.context, location, expr);
                result = InterpResult::combine(result, expr_result);
                match term_opt {
                    None => return (None, result),
                    Some(Term::MTerm(_)) => {
                        result.add_error(make_error(expr.span, "Not a diagram"));
                        return (None, result);
                    }
                    Some(Term::DTerm(d_right)) => {
                        let k = (acc.dim().max(0) as usize)
                            .min(d_right.dim().max(0) as usize)
                            .saturating_sub(1);
                        match Diagram::paste(k, &acc, &d_right) {
                            Ok(d) => acc = d,
                            Err(e) => {
                                result.add_error(make_error(span,
                                    format!("Failed to paste diagrams: {}", e)));
                                return (None, result);
                            }
                        }
                    }
                }
            }

            (Some(acc), result)
        }
    }
}

fn interpret_diagram_paste(
    context: &Context,
    location: &Complex,
    span: Span,
    left: &Spanned<ast::Diagram>,
    k: usize,
    right: &[Spanned<DExpr>],
) -> (Option<Diagram>, InterpResult) {
    // Process right side first (as in old code)
    let (right_opt, right_result) = interpret_principal(context, location, right, span);
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
                            r.add_error(make_error(span,
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
    d_expr: &Spanned<DExpr>,
) -> (Option<Term>, InterpResult) {
    match &d_expr.inner {
        DExpr::Component(comp) => {
            let (comp_opt, result) = interpret_d_comp(context, location, comp, d_expr.span);
            match comp_opt {
                None => (None, result),
                Some(Component::Hole) | Some(Component::Bd(_)) => {
                    let mut r = result;
                    r.add_error(make_error(d_expr.span, "Not a diagram or map"));
                    (None, r)
                }
                Some(Component::Term(t)) => (Some(t), result),
            }
        }
        DExpr::Dot { base, field } => {
            let (left_opt, left_result) = interpret_d_expr(context, location, base);
            match left_opt {
                None => (None, left_result),
                Some(Term::DTerm(diagram)) => {
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context, location, &field.inner, field.span
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
                                    r.add_error(make_error(field.span, e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        Some(Component::Term(_)) | Some(Component::Hole) => {
                            let mut r = combined;
                            r.add_error(make_error(field.span, "Not a well-formed diagram expression"));
                            (None, r)
                        }
                    }
                }
                Some(Term::MTerm(mc)) => {
                    let (comp_opt, comp_result) = interpret_d_comp(
                        &left_result.context, &*mc.source, &field.inner, field.span
                    );
                    let combined = InterpResult::combine(left_result, comp_result);
                    match comp_opt {
                        None => (None, combined),
                        Some(Component::Hole) | Some(Component::Bd(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(field.span, "Not a diagram or map"));
                            (None, r)
                        }
                        Some(Component::Term(Term::DTerm(d))) => {
                            match PMap::apply(&mc.map, &d) {
                                Ok(d_img) => (Some(Term::DTerm(d_img)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error(field.span, e.to_string()));
                                    (None, r)
                                }
                            }
                        }
                        Some(Component::Term(Term::MTerm(right_mc))) => {
                            let composed = PMap::compose(&mc.map, &right_mc.map);
                            (Some(Term::MTerm(MapComponent { map: composed, source: right_mc.source })), combined)
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
    d_comp: &DComponent,
    span: Span,
) -> (Option<Component>, InterpResult) {
    match d_comp {
        DComponent::Name(name) => {
            let base_result = InterpResult::ok(context.clone());
            if let Some(diagram) = location.find_diagram(name) {
                return (Some(Component::Term(Term::DTerm(diagram.clone()))), base_result);
            }
            if let Some(entry) = location.find_map(name) {
                let source = match &entry.domain {
                    MapDomain::Type(id) => match context.state.find_type(*id) {
                        Some(te) => Arc::clone(&te.complex),
                        None => {
                            let mut r = base_result;
                            r.add_error(make_error(span, format!("Type {} not found", id)));
                            return (None, r);
                        }
                    },
                    MapDomain::Module(mid) => match context.state.find_module_arc(mid) {
                        Some(m) => m,
                        None => {
                            let mut r = base_result;
                            r.add_error(make_error(span, format!("Module `{}` not found", mid)));
                            return (None, r);
                        }
                    },
                };
                return (Some(Component::Term(Term::MTerm(MapComponent {
                    map: entry.map.clone(),
                    source,
                }))), base_result);
            }
            let mut r = base_result;
            r.add_error(make_error(span, format!("Name `{}` not found", name)));
            (None, r)
        }
        DComponent::In => {
            (Some(Component::Bd(DiagramSign::Input)), InterpResult::ok(context.clone()))
        }
        DComponent::Out => {
            (Some(Component::Bd(DiagramSign::Output)), InterpResult::ok(context.clone()))
        }
        DComponent::Paren(inner_diag) => {
            let (d_opt, result) = interpret_diagram(context, location, inner_diag);
            match d_opt {
                None => (None, result),
                Some(d) => (Some(Component::Term(Term::DTerm(d))), result),
            }
        }
        DComponent::Hole => {
            (Some(Component::Hole), InterpResult::ok(context.clone()))
        }
    }
}

// ---- Assert ----

fn interpret_assert(
    context: &Context,
    location: &Complex,
    assert_stmt: &AssertStmt,
) -> (Option<TermPair>, InterpResult) {
    let (left_opt, left_result) = interpret_diagram_as_term(context, location, &assert_stmt.lhs);
    match left_opt {
        None => (None, left_result),
        Some(left_term) => {
            let (right_opt, right_result) = interpret_diagram_as_term(&left_result.context, location, &assert_stmt.rhs);
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
                                fst: mc1.map,
                                snd: mc2.map,
                                source: mc1.source,
                            }), combined)
                        }
                        _ => {
                            let span = assert_stmt.lhs.span;
                            let mut r = combined;
                            r.add_error(make_error(span, "The two sides of the equation are incomparable"));
                            (None, r)
                        }
                    }
                }
            }
        }
    }
}

/// Interpret a diagram expression as a Term (can be DTerm or MTerm).
/// This handles both Principal and Paste forms.
fn interpret_diagram_as_term(
    context: &Context,
    location: &Complex,
    diagram: &Spanned<ast::Diagram>,
) -> (Option<Term>, InterpResult) {
    match &diagram.inner {
        ast::Diagram::Principal(exprs) => {
            interpret_principal_as_term(context, location, exprs, diagram.span)
        }
        ast::Diagram::Paste { lhs, dim, rhs } => {
            let k = match dim.inner.parse::<usize>() {
                Ok(n) => n,
                Err(_) => {
                    let mut r = InterpResult::ok(context.clone());
                    r.add_error(make_error(dim.span,
                        format!("Invalid paste dimension: {}", dim.inner)));
                    return (None, r);
                }
            };
            // Right side first
            let (right_opt, right_result) = interpret_principal_as_term(context, location, rhs, diagram.span);
            match right_opt {
                None => (None, right_result),
                Some(Term::MTerm(_)) => {
                    let mut r = right_result;
                    r.add_error(make_error(diagram.span, "Not a diagram"));
                    (None, r)
                }
                Some(Term::DTerm(d_right)) => {
                    let (left_opt, left_result) = interpret_diagram_as_term(&right_result.context, location, lhs);
                    let combined = InterpResult::combine(right_result, left_result);
                    match left_opt {
                        None => (None, combined),
                        Some(Term::MTerm(_)) => {
                            let mut r = combined;
                            r.add_error(make_error(diagram.span, "Not a diagram"));
                            (None, r)
                        }
                        Some(Term::DTerm(d_left)) => {
                            match Diagram::paste(k, &d_left, &d_right) {
                                Ok(d) => (Some(Term::DTerm(d)), combined),
                                Err(e) => {
                                    let mut r = combined;
                                    r.add_error(make_error(diagram.span,
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

/// Interpret principal diagram as a Term (allowing first element to be a map).
fn interpret_principal_as_term(
    context: &Context,
    location: &Complex,
    exprs: &[Spanned<DExpr>],
    span: Span,
) -> (Option<Term>, InterpResult) {
    if exprs.is_empty() {
        let mut r = InterpResult::ok(context.clone());
        r.add_error(make_error(span, "Empty diagram expression"));
        return (None, r);
    }

    let (first_opt, first_result) = interpret_d_expr(context, location, &exprs[0]);
    match first_opt {
        None => return (None, first_result),
        Some(term) => {
            if exprs.len() == 1 {
                return (Some(term), first_result);
            }

            // Multiple exprs: must all be diagrams
            let d_first = match term {
                Term::DTerm(d) => d,
                Term::MTerm(_) => {
                    let mut r = first_result;
                    r.add_error(make_error(exprs[0].span, "Not a diagram"));
                    return (None, r);
                }
            };

            let mut acc = d_first;
            let mut result = first_result;

            for expr in &exprs[1..] {
                let (term_opt, expr_result) = interpret_d_expr(&result.context, location, expr);
                result = InterpResult::combine(result, expr_result);
                match term_opt {
                    None => return (None, result),
                    Some(Term::MTerm(_)) => {
                        result.add_error(make_error(expr.span, "Not a diagram"));
                        return (None, result);
                    }
                    Some(Term::DTerm(d_right)) => {
                        let k = (acc.dim().max(0) as usize)
                            .min(d_right.dim().max(0) as usize)
                            .saturating_sub(1);
                        match Diagram::paste(k, &acc, &d_right) {
                            Ok(d) => acc = d,
                            Err(e) => {
                                result.add_error(make_error(span,
                                    format!("Failed to paste diagrams: {}", e)));
                                return (None, result);
                            }
                        }
                    }
                }
            }

            (Some(Term::DTerm(acc)), result)
        }
    }
}

// ---- Boundaries ----

fn interpret_boundaries(
    context: &Context,
    location: &Complex,
    boundaries: &Spanned<ast::Boundary>,
) -> (Option<CellData>, InterpResult) {
    let (in_opt, src_result) = interpret_diagram(context, location, &boundaries.inner.source);
    match in_opt {
        None => (None, src_result),
        Some(boundary_in) => {
            let (out_opt, tgt_result) = interpret_diagram(&src_result.context, location, &boundaries.inner.target);
            let combined = InterpResult::combine(src_result, tgt_result);
            match out_opt {
                None => (None, combined),
                Some(boundary_out) => {
                    (Some(CellData::Boundary { boundary_in: Arc::new(boundary_in), boundary_out: Arc::new(boundary_out) }), combined)
                }
            }
        }
    }
}

// ---- Diagram naming ----

fn interpret_let_diag(
    context: &Context,
    location: &Complex,
    ld: &LetDiag,
) -> (Option<(LocalId, Diagram)>, InterpResult) {
    let (diag_opt, diag_result) = interpret_diagram(context, location, &ld.value);
    match diag_opt {
        None => (None, diag_result),
        Some(diagram) => {
            let name = ld.name.inner.clone();
            let context_after = diag_result.context.clone();

            match &ld.boundary {
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
                            let bound_span = bounds.span;
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
                                r.add_error(make_error(bound_span, msg));
                                return (None, r);
                            }
                            if let Err(msg) = check_boundary(DiagramSign::Output, &boundary_out) {
                                r.add_error(make_error(bound_span, msg));
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

// ---- Partial map naming ----

fn interpret_def_pmap(
    context: &Context,
    location: &Complex,
    dp: &DefPMap,
) -> (Option<(LocalId, PMap, MapDomain)>, InterpResult) {
    let (id_opt, addr_result) = interpret_address(context, &dp.address.inner, dp.address.span);
    match id_opt {
        None => (None, addr_result),
        Some(id) => {
            let context_after = addr_result.context.clone();
            let source = match context_after.state.find_type(id) {
                None => {
                    let mut r = addr_result;
                    r.add_error(make_error(dp.address.span,
                        format!("Type {} not found", id)));
                    return (None, r);
                }
                Some(te) => (*te.complex).clone(),
            };
            let (mc_opt, m_result) = interpret_pmap(&context_after, location, &source, &dp.value);
            let combined = InterpResult::combine(addr_result, m_result);
            match mc_opt {
                None => (None, combined),
                Some(mc) => {
                    let name = dp.name.inner.clone();
                    (Some((name, mc.map, MapDomain::Type(id))), combined)
                }
            }
        }
    }
}

// ---- Identity map ----

fn identity_map(context: &Context, domain: &Complex) -> PMap {
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
    PMap::of_entries(entries, true)
}
