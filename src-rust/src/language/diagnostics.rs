use crate::helper::{error::{Error, Phase, Producer}, positions::Span};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error   => write!(f, "error"),
            Self::Warning => write!(f, "warning"),
            Self::Info    => write!(f, "info"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub error: Error,
    pub span: Span,
    pub producer: Producer,
    pub details: Vec<String>,
    pub code: Option<String>,
}

impl Diagnostic {
    pub fn new(
        severity: Severity,
        producer: Producer,
        span: Span,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            error: Error::new(message),
            span,
            producer,
            details: vec![],
            code: None,
        }
    }

    pub fn error(producer: Producer, span: Span, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, producer, span, message)
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.error.notes.push(note.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.details.push(detail.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.severity)?;
        if let Some(code) = &self.code {
            write!(f, " [{}]", code)?;
        }
        write!(f, ": {}", self.error.message)?;
        write!(f, "\n  origin: {}", self.producer.origin_string())?;
        write!(f, "\n  span: {}", self.span)?;
        for note in &self.error.notes {
            write!(f, "\n  note: {}", note)?;
        }
        for detail in &self.details {
            write!(f, "\n  {}", detail)?;
        }
        Ok(())
    }
}

/// A collection of diagnostics.
#[derive(Debug, Clone, Default)]
pub struct Report(Vec<Diagnostic>);

impl Report {
    pub fn empty() -> Self {
        Self(vec![])
    }

    pub fn add(&mut self, d: Diagnostic) {
        self.0.push(d);
    }

    pub fn append(&mut self, mut other: Report) {
        self.0.append(&mut other.0);
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.0
    }

    pub fn has_errors(&self) -> bool {
        self.0.iter().any(|d| d.is_error())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return write!(f, "no diagnostics");
        }
        for (i, d) in self.0.iter().enumerate() {
            if i > 0 { writeln!(f)?; }
            write!(f, "{}", d)?;
        }
        Ok(())
    }
}

/// Create an interpreter-phase producer for a given module.
pub fn interpreter_producer(module_path: Option<String>) -> Producer {
    Producer { phase: Phase::Interpreter, module_path }
}

pub fn parser_producer() -> Producer {
    Producer { phase: Phase::Parser, module_path: None }
}

pub fn driver_producer(module_path: Option<String>) -> Producer {
    Producer { phase: Phase::Driver, module_path }
}
