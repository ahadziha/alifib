use crate::aux::{loader::{LoadFileError, Loader}, Tag};
use crate::core::{
    complex::{Complex, MapDomain},
    diagram::{CellData, Diagram, Sign},
    partial_map::PartialMap,
};
use crate::interpreter::{Context, GlobalStore, HoleBd, HoleInfo, interpret_program};
use crate::language::Error as LangError;
use std::fmt;
use std::sync::Arc;

// ---- LoadResult ----

/// The outcome of attempting to load and interpret a source file.
///
/// Callers that only want success can call `.ok()` or pattern-match.
/// Callers that want to print diagnostics can call `.report()`.
#[must_use]
pub enum LoadResult {
    /// Interpretation succeeded.
    Loaded(InterpretedFile),
    /// File loading or dependency resolution failed.
    LoadError(LoadFileError),
    /// Parsing or interpretation of a module produced errors.
    InterpError { errors: Vec<LangError>, source: String, path: String },
}

impl LoadResult {
    /// Print diagnostics to stderr, mirroring the previous `Option`-based behaviour.
    pub fn report(&self) {
        match self {
            LoadResult::Loaded(_) => {}
            LoadResult::LoadError(e) => crate::aux::error::report_load_file_error(e),
            LoadResult::InterpError { errors, source, path } => {
                crate::language::report_errors(errors, source, path);
            }
        }
    }

    /// Consume, returning `Some(InterpretedFile)` on success, `None` otherwise.
    /// Diagnostics are NOT printed; call `report()` first if you need them.
    pub fn ok(self) -> Option<InterpretedFile> {
        match self {
            LoadResult::Loaded(f) => Some(f),
            _ => None,
        }
    }

    /// Returns `true` if interpretation succeeded.
    pub fn is_ok(&self) -> bool {
        matches!(self, LoadResult::Loaded(_))
    }
}

// ---- InterpretedFile ----

/// The result of interpreting a single alifib source file, ready for display.
pub struct InterpretedFile {
    pub state: Arc<GlobalStore>,
    pub holes: Vec<HoleInfo>,
    pub source: String,
    pub path: String,
}

impl InterpretedFile {
    /// Load, parse, and interpret a source file, returning a structured [`LoadResult`].
    ///
    /// The pipeline has three phases:
    /// 1. Parse the root file and discover all transitively included modules.
    /// 2. Resolve the full dependency graph (handled inside `loader.load`).
    /// 3. Interpret every dependency in topological order (leaves first), then
    ///    interpret the root.  All results share a single accumulated `GlobalStore`.
    ///
    /// Call [`LoadResult::report`] to print diagnostics to stderr, then
    /// [`LoadResult::ok`] to extract the file on success.
    pub fn load(loader: &Loader, path: &str) -> LoadResult {
        // Phase 1 + 2: read, parse, resolve all dependencies.
        let loaded = match loader.load(path) {
            Ok(f) => f,
            Err(e) => return LoadResult::LoadError(e),
        };

        let (resolutions, topo_modules) = loaded.modules.into_parts();
        let resolutions = Arc::new(resolutions);

        // Phase 3a: interpret dependency modules in topological order (leaves first).
        let mut prev_state = Arc::new(GlobalStore::default());
        for (dep_path, dep_module) in &topo_modules {
            let dep_context = Context::new_with_resolutions(
                dep_path.clone(),
                Arc::clone(&resolutions),
                Arc::clone(&prev_state),
            );
            let dep_result = interpret_program(dep_context, &dep_module.program);
            if !dep_result.errors.is_empty() {
                return LoadResult::InterpError {
                    errors: dep_result.errors,
                    source: dep_module.source.clone(),
                    path: dep_path.clone(),
                };
            }
            prev_state = dep_result.context.state;
        }

        // Phase 3b: interpret the root module.
        let root_context = Context::new_with_resolutions(
            loaded.canonical_path.clone(),
            Arc::clone(&resolutions),
            prev_state,
        );
        let result = interpret_program(root_context, &loaded.program);

        if !result.errors.is_empty() {
            return LoadResult::InterpError {
                errors: result.errors,
                source: loaded.source,
                path: loaded.canonical_path,
            };
        }

        LoadResult::Loaded(Self {
            state: Arc::clone(&result.context.state),
            holes: result.holes,
            source: loaded.source,
            path: loaded.canonical_path,
        })
    }

    pub fn has_holes(&self) -> bool {
        !self.holes.is_empty()
    }

    /// Print hole diagnostics to stderr using ariadne.
    pub fn report_holes(&self) {
        for hole in &self.holes {
            let message = match &hole.boundary {
                Some(bd) => format!(
                    "{} -> {}",
                    render_hole_bd(&bd.boundary_in),
                    render_hole_bd(&bd.boundary_out)
                ),
                None => "unknown boundary".to_string(),
            };
            crate::language::error::report_hole(hole.span, &message, &self.source, &self.path);
        }
    }
}

impl fmt::Display for InterpretedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.state)
    }
}

// ---- Store ----

/// A name-keyed view of a [`GlobalStore`], free of opaque [`crate::aux::GlobalId`]s.
///
/// Suitable for structural equality tests and as the intermediate form for the
/// string renderer. Produced by [`GlobalStore::normalize`].
#[derive(Debug, PartialEq)]
pub struct Store {
    pub cells_count: usize,
    pub types_count: usize,
    pub modules: Vec<Module>,
}

/// A single module section in a [`Store`], in load order.
#[derive(Debug, PartialEq)]
pub struct Module {
    pub path: String,
    /// Types (named generators) sorted by name.
    pub types: Vec<Type>,
}

/// A single type within a [`Module`].
#[derive(Debug, PartialEq)]
pub struct Type {
    /// Empty string for the unnamed root type (displayed as `<empty>`).
    pub name: String,
    /// Generators grouped by dimension in ascending order.
    pub dims: Vec<Dim>,
    /// Named diagrams, sorted by name.
    pub diagrams: Vec<Cell>,
    /// Named maps to other types or modules, sorted by name.
    pub maps: Vec<Map>,
}

/// Generators of a single dimension within a [`Type`].
#[derive(Debug, PartialEq)]
pub struct Dim {
    pub dim: usize,
    /// Generators at this dimension, sorted by name.
    pub cells: Vec<Cell>,
}

/// A named generator or diagram, with its source and target boundary expressed
/// as lists of generator names. Both lists are empty for 0-dimensional cells.
#[derive(Debug, PartialEq)]
pub struct Cell {
    pub name: String,
    pub src: Vec<String>,
    pub tgt: Vec<String>,
}

/// A named map to another type or module.
#[derive(Debug, PartialEq)]
pub struct Map {
    pub name: String,
    pub domain: String,
}

impl GlobalStore {
    /// Convert this store into a [`Store`]: a plain, name-keyed tree with
    /// no opaque IDs, suitable for `assert_eq!` in tests and as the renderer's input.
    ///
    /// Panics if an interpreter invariant is violated (e.g. a module generator
    /// has no corresponding type entry). Those are interpreter bugs, not caller errors.
    pub fn normalize(&self) -> Store {
        let modules = self
            .modules_iter()
            .map(|(path, mc)| normalize_module(self, path, mc))
            .collect();
        Store {
            cells_count: self.cells_count(),
            types_count: self.types_count(),
            modules,
        }
    }
}

fn normalize_module(store: &GlobalStore, path: &str, mc: &Complex) -> Module {
    let mut gen_entries: Vec<(&str, &Tag)> = mc
        .generators_iter()
        .map(|(name, tag, _)| (name.as_str(), tag))
        .collect();
    gen_entries.sort_by_key(|(name, _)| *name);

    let types = gen_entries
        .iter()
        .map(|(gen_name, gen_tag)| {
            let Tag::Global(gid) = gen_tag else {
                panic!(
                    "interpreter invariant violated: module generator '{}' has a local tag",
                    gen_name
                );
            };
            let type_entry = store
                .find_type(*gid)
                .expect("interpreter invariant violated: module generator has no type entry");
            normalize_type(store, gen_name, mc, &type_entry.complex)
        })
        .collect();

    Module { path: path.to_owned(), types }
}

fn normalize_type(
    store: &GlobalStore,
    name: &str,
    module_complex: &Complex,
    tc: &Complex,
) -> Type {
    let mut dim_set: Vec<usize> = tc.generators_iter().map(|(_, _, d)| d).collect();
    dim_set.sort_unstable();
    dim_set.dedup();

    let dims = dim_set
        .iter()
        .map(|&dim| {
            let mut gens: Vec<(&str, &Tag)> = tc
                .generators_iter()
                .filter(|(_, _, d)| *d == dim)
                .map(|(n, tag, _)| (n.as_str(), tag))
                .collect();
            gens.sort_by_key(|(n, _)| *n);
            let cells = gens
                .iter()
                .map(|(n, tag)| {
                    let data = store
                        .cell_data_for_tag(tc, tag)
                        .expect("interpreter invariant violated: generator has no cell data");
                    cell_from_data(n, &data, tc)
                })
                .collect();
            Dim { dim, cells }
        })
        .collect();

    let mut diag_entries: Vec<(&str, &Diagram)> =
        tc.diagrams_iter().map(|(n, d)| (n.as_str(), d)).collect();
    diag_entries.sort_by_key(|(n, _)| *n);
    let diagrams = diag_entries
        .iter()
        .map(|(n, d)| cell_from_diagram(n, d, tc))
        .collect();

    let mut map_entries: Vec<(&str, &MapDomain)> =
        tc.maps_iter().map(|(n, _, dom)| (n.as_str(), dom)).collect();
    map_entries.sort_by_key(|(n, _)| *n);
    let maps = map_entries
        .iter()
        .map(|(n, dom)| Map { name: n.to_string(), domain: render_domain(dom, module_complex) })
        .collect();

    Type { name: name.to_owned(), dims, diagrams, maps }
}

// ---- Display for Store ----

impl fmt::Display for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} cells, {} types, {} modules",
                 self.cells_count, self.types_count, self.modules.len())?;
        for module in &self.modules {
            write!(f, "{}", module)?;
        }
        Ok(())
    }
}

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\n* Module {}\n", self.path)?;
        for (i, t) in self.types.iter().enumerate() {
            if i > 0 { writeln!(f)?; }
            write!(f, "{}", t)?;
        }
        Ok(())
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        writeln!(f, "Type {}", label)?;
        if self.dims.is_empty() {
            writeln!(f, "  (no cells)")?;
        } else {
            for dg in &self.dims {
                let cells = dg.cells.iter().map(|c| c.to_string()).collect::<Vec<_>>();
                writeln!(f, "  [{}] {}", dg.dim, cells.join(", "))?;
            }
        }
        if !self.diagrams.is_empty() {
            let diagrams = self.diagrams.iter().map(|d| d.to_string()).collect::<Vec<_>>();
            writeln!(f, "  Diagrams: {}", diagrams.join(", "))?;
        }
        if !self.maps.is_empty() {
            let maps = self.maps.iter().map(|m| m.to_string()).collect::<Vec<_>>();
            writeln!(f, "  Maps: {}", maps.join(", "))?;
        }
        Ok(())
    }
}

impl fmt::Display for Cell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        if self.src.is_empty() && self.tgt.is_empty() {
            write!(f, "{}", label)
        } else {
            write!(f, "{} : {} -> {}", label, self.src.join(" "), self.tgt.join(" "))
        }
    }
}

impl fmt::Display for Map {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = if self.name.is_empty() { "<empty>" } else { &self.name };
        write!(f, "{} :: {}", label, self.domain)
    }
}

// ---- Rendering helpers ----

fn name_or_empty(s: &str) -> &str {
    if s.is_empty() { "<empty>" } else { s }
}

/// Resolve the top-level labels of `diagram` to generator names in `scope`.
fn diagram_labels(diagram: &Diagram, scope: &Complex) -> Vec<String> {
    match diagram.labels_at(diagram.top_dim()) {
        Some(labels) if !labels.is_empty() => labels
            .iter()
            .map(|tag| {
                scope
                    .find_generator_by_tag(tag)
                    .filter(|n| !n.is_empty())
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag))
            })
            .collect(),
        _ => vec!["?".to_string()],
    }
}

pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    diagram_labels(diagram, scope).join(" ")
}

pub fn render_boundary_partial(boundary: &Diagram, map: &PartialMap, scope: &Complex) -> String {
    match boundary.labels_at(boundary.top_dim()) {
        Some(labels) if !labels.is_empty() => labels
            .iter()
            .map(|tag| match map.image(tag) {
                Ok(img) => render_diagram(img, scope),
                Err(_) => "?".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => "?".to_string(),
    }
}

fn render_hole_bd(bd: &HoleBd) -> String {
    match bd {
        HoleBd::Unknown => "?".to_string(),
        HoleBd::Full(diagram, scope) => render_diagram(diagram, scope),
        HoleBd::Partial { boundary, map, scope } => render_boundary_partial(boundary, map, scope),
    }
}

fn cell_from_data(name: &str, data: &CellData, complex: &Complex) -> Cell {
    match data {
        CellData::Zero => Cell { name: name.to_owned(), src: vec![], tgt: vec![] },
        CellData::Boundary { boundary_in, boundary_out } => Cell {
            name: name.to_owned(),
            src: diagram_labels(boundary_in, complex),
            tgt: diagram_labels(boundary_out, complex),
        },
    }
}

fn cell_from_diagram(name: &str, diag: &Diagram, complex: &Complex) -> Cell {
    let Some(k) = diag.top_dim().checked_sub(1) else {
        return Cell { name: name.to_owned(), src: vec![], tgt: vec![] };
    };
    let (Ok(src_diag), Ok(tgt_diag)) = (
        Diagram::boundary(Sign::Source, k, diag),
        Diagram::boundary(Sign::Target, k, diag),
    ) else {
        return Cell { name: name.to_owned(), src: vec![], tgt: vec![] };
    };
    Cell {
        name: name.to_owned(),
        src: diagram_labels(&src_diag, complex),
        tgt: diagram_labels(&tgt_diag, complex),
    }
}

fn render_domain(domain: &MapDomain, module_complex: &Complex) -> String {
    match domain {
        MapDomain::Type(gid) => {
            let tag = Tag::Global(*gid);
            module_complex
                .find_generator_by_tag(&tag)
                .map(|n| name_or_empty(n).to_owned())
                .unwrap_or_else(|| format!("{}", gid))
        }
        MapDomain::Module(mid) => mid.clone(),
    }
}

// ---- Display for GlobalStore ----

impl fmt::Display for GlobalStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalize())
    }
}
