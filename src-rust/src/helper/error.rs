use std::fmt;

#[derive(Debug, Clone)]
pub struct Error {
    pub message: String,
    pub notes: Vec<String>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), notes: vec![] }
    }

    pub fn with_notes(message: impl Into<String>, notes: Vec<String>) -> Self {
        Self { message: message.into(), notes }
    }

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

pub type Checked<T> = Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Lexer,
    Parser,
    Driver,
    Interpreter,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lexer => "lexer",
            Self::Parser => "parser",
            Self::Driver => "driver",
            Self::Interpreter => "interpreter",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Producer {
    pub phase: Phase,
    pub module_path: Option<String>,
}

impl Producer {
    pub fn origin_string(&self) -> String {
        match &self.module_path {
            None => self.phase.as_str().to_owned(),
            Some(path) => format!("{}:{}", self.phase.as_str(), path),
        }
    }
}
