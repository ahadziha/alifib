use ariadne::{Color, Label, Report, ReportKind, Source};
use super::ast::Span;
use crate::interpreter::types::HoleInfo;

#[derive(Debug, Clone)]
pub enum Error {
    Syntax { message: String, span: Span },
    Runtime { message: String, span: Span },
}

pub fn report_errors(errors: &[Error], source: &str, filename: &str) {
    for error in errors {
        let (label, message, span) = match error {
            Error::Syntax { message, span } => ("Syntax error", message.as_str(), span),
            Error::Runtime { message, span } => ("Runtime error", message.as_str(), span),
        };
        Report::build(ReportKind::Error, (filename, span.start..span.end))
            .with_message(label)
            .with_label(
                Label::new((filename, span.start..span.end))
                    .with_message(message)
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename, Source::from(source)))
            .unwrap_or_else(|e| eprintln!("could not write diagnostic: {}", e));
    }
}

pub fn report_holes(holes: &[HoleInfo], source: &str, filename: &str) {
    for hole in holes {
        let message = match &hole.boundary {
            Some(bd) => format!("{} -> {}", bd.boundary_in, bd.boundary_out),
            None => "unknown boundary".to_string(),
        };
        Report::build(ReportKind::Advice, (filename, hole.span.start..hole.span.end))
            .with_message("Hole")
            .with_label(
                Label::new((filename, hole.span.start..hole.span.end))
                    .with_message(message)
                    .with_color(Color::Blue),
            )
            .finish()
            .eprint((filename, Source::from(source)))
            .unwrap_or_else(|e| eprintln!("could not write diagnostic: {}", e));
    }
}
