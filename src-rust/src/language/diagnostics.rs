use crate::helper::{error::{Error, Phase, Producer}, positions::Span};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
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

pub fn parser_producer() -> Producer {
    Producer { phase: Phase::Parser, module_path: None }
}

pub fn driver_producer(module_path: Option<String>) -> Producer {
    Producer { phase: Phase::Driver, module_path }
}
