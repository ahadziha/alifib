use ariadne::{Color, Label, Report, ReportKind, Source};
use super::ast::Span;

#[derive(Debug, Clone)]
pub enum Error {
    Syntax { message: String, span: Span },
    Runtime { message: String, span: Span },
}

pub fn report_errors(errors: &[Error], source: &str, filename: &str) {
    for error in errors {
        match error {
            Error::Syntax { message, span } => {
                Report::build(ReportKind::Error, (filename, span.start..span.end))
                    .with_message("Syntax error")
                    .with_label(
                        Label::new((filename, span.start..span.end))
                            .with_message(message)
                            .with_color(Color::Red),
                    )
                    .finish()
                    .eprint((filename, Source::from(source)))
                    .unwrap();
            }
            Error::Runtime { message, span } => {
                Report::build(ReportKind::Error, (filename, span.start..span.end))
                    .with_message("Runtime error")
                    .with_label(
                        Label::new((filename, span.start..span.end))
                            .with_message(message)
                            .with_color(Color::Red),
                    )
                    .finish()
                    .eprint((filename, Source::from(source)))
                    .unwrap();
            }
        }
    }
}
