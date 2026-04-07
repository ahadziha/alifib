//! Generic error type and load-error reporting.

use std::fmt;

use super::loader::{LoadError, LoadFileError};

/// A simple structured error with an optional list of notes.
///
/// Used throughout the interpreter to accumulate diagnostics before they are
/// turned into user-facing error messages.
#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
    pub notes: Vec<String>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), notes: vec![] }
    }

    /// Attach an additional note to this error, returning `self` for chaining.
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        for note in &self.notes {
            write!(f, "\n  note: {}", note)?;
        }
        Ok(())
    }
}

/// Print a human-readable diagnostic for a [`LoadFileError`] to stderr.
pub fn report_load_file_error(err: &LoadFileError) {
    match err {
        LoadFileError::Load { path, cause } => match cause {
            LoadError::NotFound => eprintln!("error: could not load `{}`", path),
            LoadError::IoError(reason) => eprintln!("error: could not load `{}`: {}", path, reason),
        },
        LoadFileError::Parse { path, source, errors } => {
            crate::language::report_errors(errors, source, path);
        }
        LoadFileError::ModuleNotFound { module_name } => {
            eprintln!("error: module file {}.ali not found in search paths", module_name);
        }
        LoadFileError::ModuleIoError { path, reason } => {
            eprintln!("error: could not load `{}`: {}", path, reason);
        }
        LoadFileError::Cycle { path } => {
            eprintln!("error: cyclic module dependency involving `{}`", path);
        }
    }
}
