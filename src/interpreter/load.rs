//! Loading and interpretation pipeline for alifib source files.
//!
//! The main entry point is [`InterpretedFile::load`], which reads a source
//! file, resolves its dependencies, and runs the interpreter across all
//! modules in topological order.

use crate::aux::loader::{LoadFileError, Loader};
use crate::language::{Error as LangError, Program, error::Diagnostic};
use std::fmt;
use std::sync::Arc;
use super::{Context, GlobalStore, interpret_program};

/// A failed load's summary and structured parse or interpretation diagnostics.
#[derive(Debug)]
pub struct LoadFailure {
    pub message: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl LoadFailure {
    fn from_errors(errors: &[LangError], source: &str, path: &str) -> Self {
        let diagnostics: Vec<_> = errors.iter()
            .map(|error| error.to_diagnostic(source, Some(path.to_owned()))).collect();
        let message = diagnostics.iter().map(|d| format!(
            "{} error at line {}:{} — {}", d.kind, d.start.line, d.start.col, d.message,
        )).collect::<Vec<_>>().join("\n");
        Self { message, diagnostics }
    }
}

impl From<LoadFileError> for LoadFailure {
    fn from(error: LoadFileError) -> Self {
        match error {
            LoadFileError::Parse { path, source, errors } if !errors.is_empty() =>
                Self::from_errors(&errors, &source, &path),
            other => Self { message: other.to_string(), diagnostics: vec![] },
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
    /// Original source text of the root file, kept for diagnostic rendering.
    pub source: String,
    /// Canonical path of the root file.
    pub path: String,
    /// The root file's program, as interpreted.
    pub program: Program,
}

impl InterpretedFile {
    /// Load, parse, and interpret a source file, retaining any failure as data
    /// and printing nothing.
    ///
    /// The pipeline has three phases:
    /// 1. Parse the root file and discover all transitively included modules.
    /// 2. Resolve the full dependency graph (handled inside `loader.load`).
    /// 3. Interpret every dependency in topological order (leaves first), then
    ///    interpret the root. All results share a single accumulated [`GlobalStore`].
    pub fn load(loader: &Loader, path: &str) -> Result<Self, LoadFailure> {
        // Phase 1 + 2: read, parse, resolve all dependencies.
        let loaded = loader.load(path)?;

        let resolutions = Arc::new(loaded.resolutions);

        // Phase 3a: interpret dependency modules in topological order (leaves first).
        let mut prev_state = Arc::new(GlobalStore::default());
        for (dep_path, dep_module) in &loaded.dep_modules {
            let dep_context = Context::new_with_resolutions(
                dep_path.clone(),
                Arc::clone(&resolutions),
                Arc::clone(&prev_state),
                Arc::new(dep_module.source.clone()),
            );
            let dep_result = interpret_program(dep_context, &dep_module.program);
            if !dep_result.errors.is_empty() {
                return Err(LoadFailure::from_errors(&dep_result.errors, &dep_module.source, dep_path));
            }
            prev_state = dep_result.context.state;
        }

        // Phase 3b: interpret the root module.
        let root_source = Arc::new(loaded.source.clone());
        let root_context = Context::new_with_resolutions(
            loaded.canonical_path.clone(),
            Arc::clone(&resolutions),
            prev_state,
            Arc::clone(&root_source),
        );
        let result = interpret_program(root_context, &loaded.program);

        if !result.errors.is_empty() {
            return Err(LoadFailure::from_errors(&result.errors, &loaded.source, &loaded.canonical_path));
        }

        Ok(Self {
            state: Arc::clone(&result.context.state),
            source: loaded.source,
            path: loaded.canonical_path,
            program: loaded.program,
        })
    }
}

impl fmt::Display for InterpretedFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aux::loader::LoadError;
    use crate::language::ast::Span;

    #[test]
    fn load_failure_preserves_diagnostics_and_summary() {
        let errors = vec![
            LangError::Syntax { message: "first".into(), span: Span { start: 0, end: 1 } },
            LangError::Runtime {
                message: "second".into(), span: Span { start: 2, end: 3 }, notes: vec!["detail".into()],
            },
        ];
        let parse: LoadFailure = LoadFileError::Parse {
            path: "F.ali".into(), source: "x\ny".into(), errors: errors.clone(),
        }.into();
        let interp = LoadFailure::from_errors(&errors, "x\ny", "F.ali");
        assert_eq!(parse.message, "syntax error at line 1:1 — first\nruntime error at line 2:1 — second");
        assert_eq!(interp.message, parse.message);
        assert_eq!(parse.diagnostics.len(), 2);
        assert_eq!(parse.diagnostics[1].notes, ["detail"]);
        for (parse, interp) in parse.diagnostics.iter().zip(&interp.diagnostics) {
            assert_eq!(parse.path.as_deref(), Some("F.ali"));
            assert_eq!(parse.render(false), interp.render(false));
        }
    }

    #[test]
    fn load_failure_without_diagnostics_uses_batch_wording() {
        let cases = [
            (LoadFileError::Load { path: "F.ali".into(), cause: LoadError::NotFound },
                "could not load `F.ali`"),
            (LoadFileError::Load { path: "F.ali".into(), cause: LoadError::IoError("reason".into()) },
                "could not load `F.ali`: reason"),
            (LoadFileError::Parse { path: "F.ali".into(), source: "".into(), errors: vec![] },
                "could not parse `F.ali`"),
            (LoadFileError::ModuleNotFound { module_name: "Aux".into() },
                "module file Aux.ali not found in search paths"),
            (LoadFileError::ModuleIoError { path: "F.ali".into(), reason: "reason".into() },
                "could not load `F.ali`: reason"),
            (LoadFileError::Cycle { path: "F.ali".into() },
                "cyclic module dependency involving `F.ali`"),
        ];
        for (error, message) in cases {
            assert_eq!(error.to_string(), message);
            let failure = LoadFailure::from(error);
            assert_eq!(failure.message, message);
            assert!(failure.diagnostics.is_empty());
        }
    }
}
