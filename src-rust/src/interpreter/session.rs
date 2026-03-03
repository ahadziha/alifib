use crate::helper::path;
use crate::core::state::State;
use crate::language::{
    diagnostics::{self, Diagnostic, Report, Severity},
    lexer::lex_with_implicit_commas,
    parser::parse,
};
use super::interpreter::{Context, FileLoader, InterpResult, LoadError, Status, interpret_program};

// ---- Session status ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    LoadError,
    ParserError,
    InterpreterError,
    Success,
}

// ---- Session result ----

#[derive(Debug, Clone)]
pub struct SessionResult {
    pub context: Context,
    pub report: Report,
    pub status: SessionStatus,
}

// ---- Loader ----

pub struct Loader {
    inner: FileLoader,
}

impl Loader {
    fn path_separator() -> char {
        if cfg!(windows) { ';' } else { ':' }
    }

    fn split_paths(value: &str) -> Vec<String> {
        value
            .split(Self::path_separator())
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
        read_file: Option<std::sync::Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>>,
    ) -> Self {
        let read_file = read_file.unwrap_or_else(|| {
            std::sync::Arc::new(FileLoader::default_read)
        });
        let search_paths = path::normalize_search_paths(search_paths);
        Self { inner: FileLoader { search_paths, read_file } }
    }

    pub fn default(
        extra_search_paths: Vec<String>,
        read_file: Option<std::sync::Arc<dyn Fn(&str) -> Result<String, LoadError> + Send + Sync>>,
    ) -> Self {
        let cwd = path::canonicalize(&std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_owned()));
        let env_paths = Self::env_search_paths();
        let combined = std::iter::once(cwd)
            .chain(env_paths)
            .chain(extra_search_paths)
            .collect();
        Self::make(combined, read_file)
    }

    pub fn with_search_paths(mut self, paths: Vec<String>) -> Self {
        self.inner.search_paths = path::normalize_search_paths(paths);
        self
    }

    pub fn prepend_search_paths(mut self, paths: Vec<String>) -> Self {
        let combined = paths.into_iter().chain(self.inner.search_paths).collect();
        self.inner.search_paths = path::normalize_search_paths(combined);
        self
    }

    pub fn append_search_paths(mut self, paths: Vec<String>) -> Self {
        let combined: Vec<_> = self.inner.search_paths.into_iter().chain(paths).collect();
        self.inner.search_paths = path::normalize_search_paths(combined);
        self
    }

    pub fn file_loader(&self) -> &FileLoader {
        &self.inner
    }
}

// ---- Helpers ----

fn driver_error_diag(message: impl Into<String>) -> Diagnostic {
    let producer = diagnostics::driver_producer(Some("interpreter.session".to_owned()));
    let span = crate::helper::positions::Span::unknown();
    Diagnostic::error(producer, span, message)
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

// ---- Run a file ----

pub fn run(loader: &Loader, path: &str) -> SessionResult {
    let canonical_path = path::canonicalize(path);
    let module_id = canonical_path.clone();
    let base_context = Context::new(module_id, State::empty());

    let file_loader = loader.file_loader();
    let contents = match (file_loader.read_file)(&canonical_path) {
        Err(LoadError::NotFound) => {
            let mut report = Report::empty();
            report.add(driver_error_diag(format!("Could not load `{}`: file not found", path)));
            return SessionResult {
                context: base_context,
                report,
                status: SessionStatus::LoadError,
            };
        }
        Err(LoadError::IoError(reason)) => {
            let mut report = Report::empty();
            report.add(driver_error_diag(format!("Could not load `{}`: {}", path, reason)));
            return SessionResult {
                context: base_context,
                report,
                status: SessionStatus::LoadError,
            };
        }
        Ok(s) => s,
    };

    let file_loader = ensure_root_in_loader(file_loader, &canonical_path);

    // Lex
    let (tokens, lex_errors) = lex_with_implicit_commas(&contents);
    // (Lex errors are non-fatal; the parser will surface them too)
    let _ = lex_errors; // already embedded in token stream gaps

    // Parse
    let (program, parse_report) = parse(tokens, &contents, &canonical_path);

    if parse_report.has_errors() {
        return SessionResult {
            context: base_context,
            report: parse_report,
            status: SessionStatus::ParserError,
        };
    }

    // Interpret
    let interp_result = interpret_program(&file_loader, base_context, &program);
    let mut report = parse_report;
    report.append(interp_result.report);

    let status = if interp_result.status == Status::Ok {
        SessionStatus::Success
    } else {
        SessionStatus::InterpreterError
    };

    SessionResult {
        context: interp_result.context,
        report,
        status,
    }
}
