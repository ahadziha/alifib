use ariadne::{Color, Label, Report, ReportKind, Source};
use super::ast::Span;

pub enum Error {
    Syntax { message: String, span: Span },
    Runtime { message: String },
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
            Error::Runtime { message } => {
                eprintln!("error: {}", message);
            }
        }
    }
}
