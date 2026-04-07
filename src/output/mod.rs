mod normalize;
mod types;

pub use normalize::{render_boundary_partial, render_diagram};
pub use types::{Cell, Dim, Map, Module, Store, Type};

use crate::aux::loader::{LoadFileError, Loader};
use crate::interpreter::{Context, GlobalStore, HoleInfo, interpret_program};
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
                    normalize::render_hole_bd(&bd.boundary_in),
                    normalize::render_hole_bd(&bd.boundary_out)
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
