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

pub fn parse(source: &str) -> Result<Program, Vec<Error>> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();

    let mut errors: Vec<Error> = lex_errs
        .iter()
        .map(|e| Error::Syntax {
            message: format!("{}", e.reason()),
            span: *e.span(),
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
        span: *e.span(),
    }));

    match ast {
        Some(program) if errors.is_empty() => Ok(program),
        _ => {
            if errors.is_empty() {
                errors.push(Error::Syntax {
                    message: "parse error".to_string(),
                    span: SimpleSpan::from(0..0),
                });
            }
            Err(errors)
        }
    }
}

pub fn report_errors(errors: &[Error], source: &str, filename: &str) {
    error::report_errors(errors, source, filename);
}
