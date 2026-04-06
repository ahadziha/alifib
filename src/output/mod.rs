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
        let mut prev_state = Arc::new(GlobalStore::empty());
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

// ---- Rendering helpers ----

fn name_or_empty(s: &str) -> &str {
    if s.is_empty() { "<empty>" } else { s }
}

fn top_labels_rendered(diagram: &Diagram, f: impl Fn(&Tag) -> String) -> String {
    match diagram.labels_at(diagram.top_dim()) {
        Some(labels) if !labels.is_empty() => labels.iter().map(f).collect::<Vec<_>>().join(" "),
        _ => "?".to_string(),
    }
}

pub fn render_diagram(diagram: &Diagram, scope: &Complex) -> String {
    top_labels_rendered(diagram, |tag| {
        scope
            .find_generator_by_tag(tag)
            .filter(|n| !n.is_empty())
            .cloned()
            .unwrap_or_else(|| format!("{}", tag))
    })
}

pub fn render_boundary_partial(boundary: &Diagram, map: &PartialMap, scope: &Complex) -> String {
    top_labels_rendered(boundary, |tag| match map.image(tag) {
        Ok(img) => render_diagram(img, scope),
        Err(_) => "?".to_string(),
    })
}

fn render_hole_bd(bd: &HoleBd) -> String {
    match bd {
        HoleBd::Unknown => "?".to_string(),
        HoleBd::Full(diagram, scope) => render_diagram(diagram, scope),
        HoleBd::Partial { boundary, map, scope } => render_boundary_partial(boundary, map, scope),
    }
}

fn render_cell(name: &str, data: &CellData, complex: &Complex) -> String {
    let label = name_or_empty(name);
    match data {
        CellData::Zero => label.to_owned(),
        CellData::Boundary { boundary_in, boundary_out } => {
            let src = render_diagram(boundary_in, complex);
            let tgt = render_diagram(boundary_out, complex);
            format!("{} : {} -> {}", label, src, tgt)
        }
    }
}

fn render_named_diagram(name: &str, diag: &Diagram, complex: &Complex) -> String {
    let label = name_or_empty(name);
    let Some(k) = diag.top_dim().checked_sub(1) else {
        return label.to_owned();
    };
    let (Ok(src), Ok(tgt)) = (
        Diagram::boundary(Sign::Source, k, diag),
        Diagram::boundary(Sign::Target, k, diag),
    ) else {
        return label.to_owned();
    };
    format!(
        "{} : {} -> {}",
        label,
        render_diagram(&src, complex),
        render_diagram(&tgt, complex),
    )
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
        writeln!(
            f,
            "{} cells, {} types, {} modules",
            self.cells.len(),
            self.types.len(),
            self.modules.len(),
        )?;

        let mut module_entries: Vec<_> = self
            .modules
            .iter()
            .map(|(id, arc)| (id.as_str(), &**arc))
            .collect();
        module_entries.sort_by_key(|(id, _)| *id);

        for (module_id, module_complex) in &module_entries {
            write!(f, "\n* Module {}\n", module_id)?;

            let mut gen_entries: Vec<(&str, &Tag)> = module_complex
                .generators_iter()
                .map(|(name, tag, _)| (name.as_str(), tag))
                .collect();
            gen_entries.sort_by_key(|(name, _)| *name);

            for (i, (gen_name, gen_tag)) in gen_entries.iter().enumerate() {
                if i > 0 {
                    writeln!(f)?;
                }
                let type_label = name_or_empty(gen_name);

                let Tag::Global(gid) = gen_tag else {
                    writeln!(f, "Type {} (local)", type_label)?;
                    continue;
                };
                let Some(type_entry) = self.find_type(*gid) else {
                    writeln!(f, "Type {} (not found)", type_label)?;
                    continue;
                };

                writeln!(f, "Type {}", type_label)?;
                let tc = &type_entry.complex;

                // Cells grouped by dimension, with boundaries
                let mut dims: Vec<usize> = tc.generators_iter().map(|(_, _, dim)| dim).collect();
                dims.sort_unstable();
                dims.dedup();

                if dims.is_empty() {
                    writeln!(f, "  (no cells)")?;
                } else {
                    for dim in &dims {
                        let mut gens: Vec<(&str, &Tag)> = tc
                            .generators_iter()
                            .filter(|(_, _, d)| d == dim)
                            .map(|(name, tag, _)| (name.as_str(), tag))
                            .collect();
                        gens.sort_by_key(|(name, _)| *name);

                        let rendered: Vec<String> = gens
                            .iter()
                            .filter_map(|(name, tag)| {
                                let data = self.cell_data_for_tag(tc, tag)?;
                                Some(render_cell(name, &data, tc))
                            })
                            .collect();

                        if !rendered.is_empty() {
                            writeln!(f, "  [{}] {}", dim, rendered.join(", "))?;
                        }
                    }
                }

                // Diagrams
                let mut diag_entries: Vec<(&str, &Diagram)> =
                    tc.diagrams_iter().map(|(name, diag)| (name.as_str(), diag)).collect();
                if !diag_entries.is_empty() {
                    diag_entries.sort_by_key(|(name, _)| *name);
                    let diags: Vec<String> = diag_entries
                        .iter()
                        .map(|(name, diag)| render_named_diagram(name, diag, tc))
                        .collect();
                    writeln!(f, "  Diagrams: {}", diags.join(", "))?;
                }

                // Maps
                let mut map_entries: Vec<(&str, &MapDomain)> =
                    tc.maps_iter().map(|(name, _, domain)| (name.as_str(), domain)).collect();
                if !map_entries.is_empty() {
                    map_entries.sort_by_key(|(name, _)| *name);
                    let maps: Vec<String> = map_entries
                        .iter()
                        .map(|(name, domain)| {
                            let dom = render_domain(domain, module_complex);
                            format!("{} :: {}", name_or_empty(name), dom)
                        })
                        .collect();
                    writeln!(f, "  Maps: {}", maps.join(", "))?;
                }
            }
        }
        Ok(())
    }
}
