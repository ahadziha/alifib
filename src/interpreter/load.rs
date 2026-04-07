//! Loading and interpretation pipeline for alifib source files.
//!
//! The main entry point is [`InterpretedFile::load`], which reads a source
//! file, resolves its dependencies, and runs the interpreter across all
//! modules in topological order.

use crate::aux::loader::{LoadFileError, Loader};
use crate::language::Error as LangError;
use std::fmt;
use std::sync::Arc;
use super::{Context, GlobalStore, HoleInfo, interpret_program};

// ---- LoadResult ----

/// The outcome of attempting to load and interpret a source file.
///
/// On success the variant is [`LoadResult::Loaded`]. On failure, call
/// [`LoadResult::report`] to print diagnostics to stderr, then handle or
/// discard the error as needed.
#[must_use]
pub enum LoadResult {
    /// Interpretation succeeded with no errors (holes may still be present).
    Loaded(InterpretedFile),
    /// The source file or one of its dependencies could not be read from disk.
    LoadError(LoadFileError),
    /// Parsing or interpretation of a module produced one or more errors.
    InterpError {
        errors: Vec<LangError>,
        /// Original source text of the failing module, for diagnostic rendering.
        source: String,
        /// Path of the failing module.
        path: String,
    },
}

impl LoadResult {
    /// Print diagnostics for the failure case to stderr.
    ///
    /// Does nothing if the result is [`LoadResult::Loaded`].
    pub fn report(&self) {
        match self {
            LoadResult::Loaded(_) => {}
            LoadResult::LoadError(e) => crate::aux::error::report_load_file_error(e),
            LoadResult::InterpError { errors, source, path } => {
                crate::language::report_errors(errors, source, path);
            }
        }
    }

    /// Consume the result, returning `Some(InterpretedFile)` on success and
    /// `None` on any failure. Diagnostics are **not** printed; call
    /// [`report`](Self::report) first if you need them.
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

    /// Print diagnostics and convert to `Result`, returning `Err(())` on failure.
    pub fn into_result(self) -> Result<InterpretedFile, ()> {
        match self {
            LoadResult::Loaded(f) => Ok(f),
            other => { other.report(); Err(()) }
        }
    }
}

// ---- InterpretedFile ----

/// The result of successfully interpreting a single alifib source file.
///
/// Displaying this value (via [`fmt::Display`]) prints the human-readable
/// summary of all types defined in the file. For structural access, call
/// [`state.normalize()`](GlobalStore::normalize).
pub struct InterpretedFile {
    /// Accumulated interpreter state for the file and all its dependencies.
    pub state: Arc<GlobalStore>,
    /// Any unsolved holes (`?`) encountered during interpretation.
    pub holes: Vec<HoleInfo>,
    /// Original source text of the root file, kept for diagnostic rendering.
    pub source: String,
    /// Canonical path of the root file.
    pub path: String,
}

impl InterpretedFile {
    /// Load, parse, and interpret a source file, returning a structured [`LoadResult`].
    ///
    /// The pipeline has three phases:
    /// 1. Parse the root file and discover all transitively included modules.
    /// 2. Resolve the full dependency graph (handled inside `loader.load`).
    /// 3. Interpret every dependency in topological order (leaves first), then
    ///    interpret the root. All results share a single accumulated [`GlobalStore`].
    ///
    /// Call [`LoadResult::report`] to print diagnostics to stderr, then
    /// [`LoadResult::ok`] to extract the file on success.
    pub fn load(loader: &Loader, path: &str) -> LoadResult {
        // Phase 1 + 2: read, parse, resolve all dependencies.
        let loaded = match loader.load(path) {
            Ok(f) => f,
            Err(e) => return LoadResult::LoadError(e),
        };

        let resolutions = Arc::new(loaded.resolutions);

        // Phase 3a: interpret dependency modules in topological order (leaves first).
        let mut prev_state = Arc::new(GlobalStore::default());
        for (dep_path, dep_module) in &loaded.dep_modules {
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

    /// Returns `true` if interpretation left any unsolved holes (`?`).
    pub fn has_holes(&self) -> bool {
        !self.holes.is_empty()
    }
}

impl fmt::Display for InterpretedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.state)
    }
}
