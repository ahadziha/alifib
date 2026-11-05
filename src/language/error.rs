use ariadne::{Color, Config, Label, Report, ReportKind, Source};
use serde::{Deserialize, Serialize};
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

    pub fn kind(&self) -> DiagnosticKind {
        match self {
            Error::Syntax { .. } => DiagnosticKind::Syntax,
            Error::Runtime { .. } => DiagnosticKind::Runtime,
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
        let report_source = Source::from(source);
        let char_span = byte_to_char(source, start.byte)..byte_to_char(source, end.byte);
        let lines = report_source.get_line_range(&char_span);
        let byte = report_source.lines().take(lines.start)
            .map(|line| report_source.get_line_text(line).unwrap().len()).sum();
        let text = report_source.lines().skip(lines.start).take(lines.len())
            .map(|line| report_source.get_line_text(line).unwrap()).collect();
        let excerpt = Excerpt { line: lines.start + 1, byte, text };
        Diagnostic {
            kind: self.kind(),
            message: self.message().to_string(),
            start,
            end,
            excerpt,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub byte: usize,
    pub line: usize,
    pub col: usize,
}

fn byte_to_char(source: &str, byte: usize) -> usize {
    source[..byte.min(source.len())].chars().count()
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

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticKind {
    Syntax,
    Runtime,
}

impl fmt::Display for DiagnosticKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "syntax",
            Self::Runtime => "runtime",
        })
    }
}

/// The source lines a report shows, cut by ariadne's own line rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Excerpt {
    /// Ariadne's one-based number of the first line.
    pub line: usize,
    /// The first line's byte offset in the original source.
    pub byte: usize,
    /// Verbatim source lines, including their terminators.
    pub text: String,
}

/// A medium-neutral report, with editor positions and the source it labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub start: Position,
    pub end: Position,
    pub excerpt: Excerpt,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Diagnostic {
    /// Build the ariadne report from this diagnostic alone.
    pub fn render(&self, colour: bool) -> String {
        // Display-line offsets only change ariadne's header, not its gutter.
        let padding = self.excerpt.line - 1;
        let source = "\n".repeat(padding) + &self.excerpt.text;
        let start = padding + byte_to_char(&self.excerpt.text, self.start.byte - self.excerpt.byte);
        let end = padding + byte_to_char(&self.excerpt.text, self.end.byte - self.excerpt.byte);
        let path = self.path.as_deref().unwrap_or("<source>");
        let label = match self.kind {
            DiagnosticKind::Syntax => "Syntax error",
            DiagnosticKind::Runtime => "Runtime error",
        };
        let mut report = Report::build(ReportKind::Error, (path, start..end))
            .with_config(Config::default().with_color(colour))
            .with_message(label)
            .with_label(Label::new((path, start..end))
                .with_message(&self.message).with_color(Color::Red));
        for note in &self.notes {
            report = report.with_note(note);
        }
        let mut output = Vec::new();
        report.finish().write((path, Source::from(source)), &mut output)
            .expect("writing a diagnostic to a byte buffer cannot fail");
        String::from_utf8(output).expect("ariadne reports are UTF-8")
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
    fn diagnostic_excerpt_and_render() {
        let line_12 = format!("{}let x = foo bar\n", "earlier\n".repeat(11));
        let mut cases = vec![
            (line_12, "foo".to_string(), 12, 88, "let x = foo bar\n".to_string()),
            ("before\nfirst\nsecond\nafter\n".into(), "first\nsecond".into(), 2, 7, "first\nsecond\n".into()),
            ("before\nαβ foo\n".into(), "foo".into(), 2, 7, "αβ foo\n".into()),
            ("before\nlast".into(), "".into(), 2, 7, "last".into()),
            ("before\nlast\n".into(), "".into(), 2, 7, "last\n".into()),
            ("before\r\nlast\r\n".into(), "last".into(), 2, 8, "last\r\n".into()),
            ("before\r\nlast\r\n".into(), "".into(), 2, 8, "last\r\n".into()),
            ("".into(), "".into(), 1, 0, "".into()),
        ];
        for separator in ['\r', '\n', '\x0b', '\x0c', '\u{0085}', '\u{2028}', '\u{2029}'] {
            cases.push((format!("abc{separator}xyz"), "x".into(), 2, 3 + separator.len_utf8(), "xyz".into()));
        }
        for (source, marked, excerpt_line, excerpt_byte, excerpt_text) in cases {
            let start = if marked.is_empty() { source.len() } else { source.find(&marked).unwrap() };
            let span = Span { start, end: start + marked.len() };
            for runtime in [false, true] {
                let error = if runtime {
                    Error::Runtime { message: "oops".into(), span, notes: vec!["a note".into()] }
                } else {
                    Error::Syntax { message: "oops".into(), span }
                };
                for path in [None, Some("F.ali".to_string())] {
                    let diagnostic = error.to_diagnostic(&source, path.clone());
                    let whole = Source::from(source.as_str());
                    let chars = byte_to_char(&source, span.start)..byte_to_char(&source, span.end);
                    assert_eq!(diagnostic.excerpt.line, excerpt_line);
                    assert_eq!(diagnostic.excerpt.byte, excerpt_byte);
                    assert_eq!(diagnostic.excerpt.text, excerpt_text);
                    if marked.is_empty() && source.ends_with('\n') {
                        assert_eq!(diagnostic.start.line, excerpt_line + 1);
                        assert_eq!(diagnostic.start.col, 1);
                    }
                    if source.starts_with("abc") {
                        assert_eq!(diagnostic.excerpt.text, "xyz");
                        assert_eq!(diagnostic.start.byte - diagnostic.excerpt.byte, 0);
                        assert_eq!(diagnostic.start.col, if source.contains('\n') { 1 } else { 5 });
                    }
                    if diagnostic.excerpt.line == 12 {
                        assert!(diagnostic.render(false).contains("12 │"));
                    }
                    for colour in [false, true] {
                        let filename = path.as_deref().unwrap_or("<source>");
                        let mut report = Report::build(ReportKind::Error, (filename, chars.clone()))
                            .with_config(Config::default().with_color(colour))
                            .with_message(if runtime { "Runtime error" } else { "Syntax error" })
                            .with_label(Label::new((filename, chars.clone()))
                                .with_message("oops").with_color(Color::Red));
                        for note in error.notes() { report = report.with_note(note); }
                        let mut output = Vec::new();
                        report.finish().write((filename, whole.clone()), &mut output).unwrap();
                        assert_eq!(diagnostic.render(colour).as_bytes(), output,
                            "source={source:?}, span={span:?}, colour={colour}");
                    }
                }
            }
        }
    }
}
