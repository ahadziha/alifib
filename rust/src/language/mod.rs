pub mod ast;
mod ast_fmt;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod token;

use chumsky::input::Input as _;
use chumsky::prelude::*;

pub use ast::Program;
pub use error::Error;

use ast::Span;

pub fn parse(source: &str) -> Result<Program, Vec<Error>> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();

    let mut errors: Vec<Error> = lex_errs
        .iter()
        .map(|e| Error::Syntax {
            message: format!("{}", e.reason()),
            span: Span { start: e.span().start, end: e.span().end },
        })
        .collect();

    let tokens = match tokens {
        Some(t) => t,
        None => return Err(errors),
    };

    let eoi = SimpleSpan::from(source.len()..source.len());
    let (ast, parse_errs) = parser::program_parser()
        .parse(tokens.as_slice().split_token_span(eoi))
        .into_output_errors();

    errors.extend(parse_errs.iter().map(|e| Error::Syntax {
        message: format!("{}", e.reason()),
        span: Span { start: e.span().start, end: e.span().end },
    }));

    errors.dedup_by(|a, b| match (a, b) {
        (
            Error::Syntax { message: ma, span: sa },
            Error::Syntax { message: mb, span: sb },
        ) => sa == sb && ma == mb,
        _ => false,
    });

    match ast {
        Some(program) if errors.is_empty() => Ok(program),
        _ => {
            if errors.is_empty() {
                errors.push(Error::Syntax {
                    message: "parse error".to_string(),
                    span: Span { start: 0, end: 0 },
                });
            }
            Err(errors)
        }
    }
}

pub fn report_errors(errors: &[Error], source: &str, filename: &str) {
    error::report_errors(errors, source, filename);
}

pub fn report_holes(holes: &[crate::interpreter::types::HoleInfo], source: &str, filename: &str) {
    error::report_holes(holes, source, filename);
}
