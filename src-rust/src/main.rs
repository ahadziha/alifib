mod core;
mod helper;
mod interpreter;
mod language;

use interpreter::session::{Loader, SessionStatus, run as session_run};
use language::{
    ast_pp::program_to_string,
    diagnostics::Report,
    lexer::lex_with_implicit_commas,
    parser::parse,
};

static USAGE: &str = "Usage: alifib <input-file> [-o|--output <output-file>] [--ast]";

#[derive(Debug, Clone, Copy)]
enum Mode { Interpret, Ast }

struct Args {
    input: String,
    output: Option<String>,
    mode: Mode,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut mode = Mode::Interpret;

    let mut iter = raw.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                if output.is_some() {
                    return Err("Output file specified multiple times".to_owned());
                }
                let val = iter.next()
                    .ok_or_else(|| format!("Flag {} requires an argument", arg))?;
                output = Some(val.clone());
            }
            "--ast" => {
                mode = Mode::Ast;
            }
            other if other.starts_with('-') => {
                return Err(format!("Unknown option: {}", other));
            }
            other => {
                if input.is_some() {
                    return Err("Multiple input files specified".to_owned());
                }
                input = Some(other.to_owned());
            }
        }
    }

    match input {
        None => Err(USAGE.to_owned()),
        Some(input) => Ok(Args { input, output, mode }),
    }
}

fn write_output(path: Option<&str>, contents: &str) -> std::io::Result<()> {
    match path {
        None => {
            println!("{}", contents);
            Ok(())
        }
        Some(path) => {
            let mut s = contents.to_owned();
            if !s.ends_with('\n') {
                s.push('\n');
            }
            std::fs::write(path, s)
        }
    }
}

fn has_errors(report: &Report) -> bool {
    report.has_errors()
}

fn run_ast(input_path: &str, output_path: Option<&str>) -> bool {
    let contents = match std::fs::read_to_string(input_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read `{}`: {}", input_path, e);
            return false;
        }
    };

    let (tokens, _lex_errors) = lex_with_implicit_commas(&contents);
    let (ast, report) = parse(tokens, &contents, input_path);

    let rendered = program_to_string(&ast);
    if let Err(e) = write_output(output_path, &rendered) {
        eprintln!("error: could not write output: {}", e);
        return false;
    }

    if !report.is_empty() {
        eprintln!("{}", report);
    }

    !has_errors(&report)
}

fn run_interpreter(input_path: &str, output_path: Option<&str>) -> bool {
    let loader = Loader::default(vec![], None);
    let result = session_run(&loader, input_path);

    let rendered = result.context.state.display();
    if let Err(e) = write_output(output_path, &rendered) {
        eprintln!("error: could not write output: {}", e);
        return false;
    }

    if !result.report.is_empty() {
        eprintln!("{}", result.report);
    }

    match result.status {
        SessionStatus::Success => !has_errors(&result.report),
        SessionStatus::LoadError
        | SessionStatus::ParserError
        | SessionStatus::InterpreterError => false,
    }
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{}", msg);
            std::process::exit(1);
        }
    };

    let success = match args.mode {
        Mode::Ast => run_ast(&args.input, args.output.as_deref()),
        Mode::Interpret => run_interpreter(&args.input, args.output.as_deref()),
    };

    if !success {
        std::process::exit(1);
    }
}
