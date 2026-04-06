use ariadne::{Color, Label, Report, ReportKind, Source};
use std::fmt;
use super::ast::Span;

#[derive(Debug, Clone)]
pub enum Error {
    Syntax { message: String, span: Span },
    Runtime { message: String, span: Span, notes: Vec<String> },
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
