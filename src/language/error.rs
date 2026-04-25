use ariadne::{Color, Label, Report, ReportKind, Source};
use serde::Serialize;
use std::fmt;
use super::ast::Span;

#[derive(Debug, Clone)]
pub enum Error {
    Syntax { message: String, span: Span },
    Runtime { message: String, span: Span, notes: Vec<String> },
}

impl Error {
    pub fn message(&self) -> &str {
        match self {
            Error::Syntax { message, .. } | Error::Runtime { message, .. } => message,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Error::Syntax { span, .. } | Error::Runtime { span, .. } => *span,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Error::Syntax { .. } => "syntax",
            Error::Runtime { .. } => "runtime",
        }
    }

    pub fn notes(&self) -> &[String] {
        match self {
            Error::Syntax { .. } => &[],
            Error::Runtime { notes, .. } => notes,
        }
    }

    /// Build a structured [`Diagnostic`] suitable for the web error protocol.
    ///
    /// `source` is the original text the byte offsets in `span` refer to;
    /// `path` is the file the error came from (used when the frontend may
    /// surface errors from non-root modules).
    pub fn to_diagnostic(&self, source: &str, path: Option<String>) -> Diagnostic {
        let span = self.span();
        let start = Position::from_byte(source, span.start);
        let end = Position::from_byte(source, span.end.max(span.start));
        let snippet = build_snippet(source, &start, &end);
        Diagnostic {
            kind: self.kind(),
            message: self.message().to_string(),
            start,
            end,
            snippet,
            notes: self.notes().to_vec(),
            path,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Syntax { message, span } =>
                write!(f, "syntax error at {}..{}: {}", span.start, span.end, message),
            Error::Runtime { message, span, .. } =>
                write!(f, "runtime error at {}..{}: {}", span.start, span.end, message),
        }
    }
}

/// One-indexed (line, column) position together with the original byte offset.
///
/// `col` counts Unicode scalar values from the start of the line, matching
/// the convention used by most editors.
#[derive(Debug, Clone, Serialize)]
pub struct Position {
    pub byte: usize,
    pub line: usize,
    pub col: usize,
}

impl Position {
    pub fn from_byte(source: &str, byte: usize) -> Self {
        let byte = byte.min(source.len());
        let prefix = &source[..byte];
        let line = 1 + prefix.bytes().filter(|&b| b == b'\n').count();
        let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = source[line_start..byte].chars().count() + 1;
        Position { byte, line, col }
    }
}

/// Structured diagnostic emitted across the web boundary.
///
/// The frontend renders this directly: `kind` selects styling, `start`/`end`
/// drive line:column display and editor highlighting, and `snippet` is a
/// pre-rendered source line with a caret underline ready to be shown in a
/// monospace block.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub kind: &'static str,
    pub message: String,
    pub start: Position,
    pub end: Position,
    pub snippet: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

fn build_snippet(source: &str, start: &Position, end: &Position) -> String {
    let line = source.lines().nth(start.line.saturating_sub(1)).unwrap_or("");
    let caret_start = start.col.saturating_sub(1);
    let caret_len = if start.line == end.line {
        end.col.saturating_sub(start.col).max(1)
    } else {
        line.chars().count().saturating_sub(caret_start).max(1)
    };
    let pad: String = std::iter::repeat(' ').take(caret_start).collect();
    let carets: String = std::iter::repeat('^').take(caret_len).collect();
    format!("{line}\n{pad}{carets}")
}

pub fn report_errors(errors: &[Error], source: &str, filename: &str) {
    for error in errors {
        let (label, message, span, notes): (&str, &str, &Span, &[String]) = match error {
            Error::Syntax { message, span } => ("Syntax error", message.as_str(), span, &[]),
            Error::Runtime { message, span, notes } => ("Runtime error", message.as_str(), span, notes),
        };
        let mut report = Report::build(ReportKind::Error, (filename, span.start..span.end))
            .with_message(label)
            .with_label(
                Label::new((filename, span.start..span.end))
                    .with_message(message)
                    .with_color(Color::Red),
            );
        for note in notes {
            report = report.with_note(note);
        }
        report
            .finish()
            .eprint((filename, Source::from(source)))
            .unwrap_or_else(|e| eprintln!("could not write diagnostic: {}", e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_first_byte_is_1_1() {
        let p = Position::from_byte("hello\nworld", 0);
        assert_eq!((p.line, p.col), (1, 1));
    }

    #[test]
    fn position_after_newline_is_next_line_col_1() {
        let p = Position::from_byte("hello\nworld", 6);
        assert_eq!((p.line, p.col), (2, 1));
    }

    #[test]
    fn position_uses_char_count_for_column() {
        // "αβ" is 2 chars but 4 bytes; column 3 means after both chars.
        let p = Position::from_byte("αβx", 4);
        assert_eq!((p.line, p.col), (1, 3));
    }

    #[test]
    fn position_clamped_to_source_len() {
        let p = Position::from_byte("abc", 999);
        assert_eq!((p.line, p.col), (1, 4));
    }

    #[test]
    fn diagnostic_snippet_carets_align_with_span() {
        let source = "let x = foo bar\n";
        let span = Span { start: 8, end: 11 };
        let err = Error::Syntax { message: "oops".into(), span };
        let d = err.to_diagnostic(source, None);
        assert_eq!(d.start.line, 1);
        assert_eq!(d.start.col, 9);
        assert_eq!(d.end.col, 12);
        assert_eq!(d.snippet, "let x = foo bar\n        ^^^");
    }
}

pub(crate) fn report_hole(span: Span, message: &str, source: &str, filename: &str) {
    Report::build(ReportKind::Advice, (filename, span.start..span.end))
        .with_message("Hole")
        .with_label(
            Label::new((filename, span.start..span.end))
                .with_message(message)
                .with_color(Color::Blue),
        )
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap_or_else(|e| eprintln!("could not write diagnostic: {}", e));
}
