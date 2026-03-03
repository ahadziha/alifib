mod language;

use std::env;
use std::fs;

use ariadne::{Color, Label, Report, ReportKind, Source};
use chumsky::input::Input as _;
use chumsky::prelude::*;

use language::ast::*;
use language::lexer;
use language::parser;

fn count_blocks(program: &Program) -> (usize, usize) {
    let mut type_blocks = 0;
    let mut local_blocks = 0;
    for b in &program.blocks {
        match &b.inner {
            Block::TypeBlock(_) => type_blocks += 1,
            Block::LocalBlock { .. } => local_blocks += 1,
        }
    }
    (type_blocks, local_blocks)
}

fn count_generators(program: &Program) -> usize {
    let mut count = 0;
    for b in &program.blocks {
        if let Block::TypeBlock(insts) = &b.inner {
            for inst in insts {
                if matches!(&inst.inner, TypeInst::Generator(_)) {
                    count += 1;
                }
            }
        }
    }
    count
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <file.ali>", args[0]);
        std::process::exit(1);
    }

    let filename = &args[1];
    let src = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Lex
    let (tokens, lex_errs) = lexer::lexer().parse(&src).into_output_errors();

    for e in &lex_errs {
        Report::build(ReportKind::Error, (filename.as_str(), e.span().into_range()))
            .with_message("Lex error")
            .with_label(
                Label::new((filename.as_str(), e.span().into_range()))
                    .with_message(format!("{}", e.reason()))
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&src)))
            .unwrap();
    }

    let tokens = match tokens {
        Some(t) => t,
        None => {
            eprintln!("Lexing failed, cannot parse.");
            std::process::exit(1);
        }
    };

    // Parse
    let eoi = SimpleSpan::from(src.len()..src.len());
    let token_input = tokens.as_slice().split_token_span(eoi);
    let (ast, parse_errs) = parser::program_parser()
        .parse(token_input)
        .into_output_errors();

    for e in &parse_errs {
        let span = e.span();
        Report::build(ReportKind::Error, (filename.as_str(), span.into_range()))
            .with_message("Parse error")
            .with_label(
                Label::new((filename.as_str(), span.into_range()))
                    .with_message(format!("{}", e.reason()))
                    .with_color(Color::Red),
            )
            .finish()
            .eprint((filename.as_str(), Source::from(&src)))
            .unwrap();
    }

    match ast {
        Some(program) => {
            let (type_blocks, local_blocks) = count_blocks(&program);
            let generators = count_generators(&program);
            println!(
                "Parsed OK: {} block(s) ({} type, {} local), {} generator(s)",
                type_blocks + local_blocks,
                type_blocks,
                local_blocks,
                generators,
            );
        }
        None => {
            eprintln!("Parsing failed.");
            std::process::exit(1);
        }
    }
}
