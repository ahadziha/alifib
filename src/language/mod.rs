pub mod ast;
mod ast_fmt;
mod ast_print;
pub mod error;
mod lexer;
mod parser;
mod token;

use std::collections::HashSet;

use chumsky::input::Input as _;
use chumsky::prelude::*;

pub use ast::{Complex, Program};
pub use ast_print::print_program;
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

    let Some(tokens) = tokens else { return Err(errors); };

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

/// Parse a single diagram expression (the right-hand side of a `let` binding).
///
/// Returns the parsed [`ast::Diagram`] node wrapped in a span, or an error message.
/// Used for round-trip typechecking of proof diagrams before storing them.
pub fn parse_diagram(source: &str) -> Result<ast::Spanned<ast::Diagram>, String> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();

    if !lex_errs.is_empty() {
        return Err(lex_errs.iter().map(|e| format!("{}", e.reason())).collect::<Vec<_>>().join("; "));
    }

    let Some(tokens) = tokens else {
        return Err("lex error".to_string());
    };

    let eoi = SimpleSpan::from(source.len()..source.len());
    let (ast, parse_errs) = parser::diagram_parser()
        .then_ignore(chumsky::prelude::end())
        .parse(tokens.as_slice().split_token_span(eoi))
        .into_output_errors();

    if !parse_errs.is_empty() {
        return Err(parse_errs.iter().map(|e| format!("{}", e.reason())).collect::<Vec<_>>().join("; "));
    }

    ast.ok_or_else(|| "parse error".to_string())
}

/// Parse a single `Complex` expression (the part that follows `@` in source files).
///
/// Returns the parsed [`Complex`] AST node, or an error message.  Used by the
/// interactive REPL to parse `@ <expr>` commands without duplicating lexer/parser
/// logic.
pub fn parse_complex(source: &str) -> Result<Complex, String> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();

    if !lex_errs.is_empty() {
        let msg = lex_errs
            .iter()
            .map(|e| format!("{}", e.reason()))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(msg);
    }

    let Some(tokens) = tokens else {
        return Err("lex error".to_string());
    };

    let eoi = SimpleSpan::from(source.len()..source.len());
    let (ast, parse_errs) = parser::complex_parser()
        .then_ignore(chumsky::prelude::end())
        .parse(tokens.as_slice().split_token_span(eoi))
        .into_output_errors();

    if !parse_errs.is_empty() {
        let msg = parse_errs
            .iter()
            .map(|e| format!("{}", e.reason()))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(msg);
    }

    ast.map(|s| s.inner).ok_or_else(|| "parse error".to_string())
}

pub fn report_errors(errors: &[Error], source: &str, filename: &str) {
    error::report_errors(errors, source, filename);
}

/// Collect the names of all modules referenced by `IncludeModule` instructions
/// in `@Type` blocks.
///
/// Only `@Type`-level includes are collected here because those are the ones
/// that require loading an external file before interpretation begins.
/// `@Local`-block `include` statements refer to types already in scope and are
/// resolved at interpretation time — they do not name external files.
pub(crate) fn collect_includes(program: &Program) -> Vec<String> {
    let mut seen = HashSet::new();
    program.blocks.iter()
        .filter_map(|b| match &b.inner { ast::Block::TypeBlock(body) => Some(body), _ => None })
        .flat_map(|body| body.iter())
        .filter_map(|i| match &i.inner {
            ast::TypeInst::IncludeModule(im) => Some(im.name.inner.clone()),
            _ => None,
        })
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

// ---------------------------------------------------------------------------
// Instruction-list parsers (for expanded for-block bodies)
// ---------------------------------------------------------------------------

pub fn parse_complex_instrs(source: &str) -> Result<Vec<ast::Spanned<ast::ComplexInstr>>, Vec<Error>> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();
    let mut errors: Vec<Error> = lex_errs.iter()
        .map(|e| Error::Syntax { message: format!("{}", e.reason()), span: Span { start: e.span().start, end: e.span().end } })
        .collect();
    let Some(tokens) = tokens else { return Err(errors); };
    let eoi = SimpleSpan::from(source.len()..source.len());
    let (ast, parse_errs) = parser::complex_instrs_parser()
        .parse(tokens.as_slice().split_token_span(eoi))
        .into_output_errors();
    errors.extend(parse_errs.iter().map(|e| Error::Syntax { message: format!("{}", e.reason()), span: Span { start: e.span().start, end: e.span().end } }));
    match ast {
        Some(result) if errors.is_empty() => Ok(result),
        _ => { if errors.is_empty() { errors.push(Error::Syntax { message: "parse error".to_string(), span: Span { start: 0, end: 0 } }); } Err(errors) }
    }
}

pub fn parse_type_instrs(source: &str) -> Result<Vec<ast::Spanned<ast::TypeInst>>, Vec<Error>> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();
    let mut errors: Vec<Error> = lex_errs.iter()
        .map(|e| Error::Syntax { message: format!("{}", e.reason()), span: Span { start: e.span().start, end: e.span().end } })
        .collect();
    let Some(tokens) = tokens else { return Err(errors); };
    let eoi = SimpleSpan::from(source.len()..source.len());
    let (ast, parse_errs) = parser::type_instrs_parser()
        .parse(tokens.as_slice().split_token_span(eoi))
        .into_output_errors();
    errors.extend(parse_errs.iter().map(|e| Error::Syntax { message: format!("{}", e.reason()), span: Span { start: e.span().start, end: e.span().end } }));
    match ast {
        Some(result) if errors.is_empty() => Ok(result),
        _ => { if errors.is_empty() { errors.push(Error::Syntax { message: "parse error".to_string(), span: Span { start: 0, end: 0 } }); } Err(errors) }
    }
}

pub fn parse_local_instrs(source: &str) -> Result<Vec<ast::Spanned<ast::LocalInst>>, Vec<Error>> {
    let (tokens, lex_errs) = lexer::lexer().parse(source).into_output_errors();
    let mut errors: Vec<Error> = lex_errs.iter()
        .map(|e| Error::Syntax { message: format!("{}", e.reason()), span: Span { start: e.span().start, end: e.span().end } })
        .collect();
    let Some(tokens) = tokens else { return Err(errors); };
    let eoi = SimpleSpan::from(source.len()..source.len());
    let (ast, parse_errs) = parser::local_instrs_parser()
        .parse(tokens.as_slice().split_token_span(eoi))
        .into_output_errors();
    errors.extend(parse_errs.iter().map(|e| Error::Syntax { message: format!("{}", e.reason()), span: Span { start: e.span().start, end: e.span().end } }));
    match ast {
        Some(result) if errors.is_empty() => Ok(result),
        _ => { if errors.is_empty() { errors.push(Error::Syntax { message: "parse error".to_string(), span: Span { start: 0, end: 0 } }); } Err(errors) }
    }
}

