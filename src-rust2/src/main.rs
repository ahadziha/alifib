mod language;

use std::fs;
use std::process;

const USAGE: &str = "Usage: alifib2 <input-file> [-o|--output <output-file>] [--ast]";

#[derive(Clone, Copy)]
enum Mode {
    Interpret,
    Ast,
}

struct Args {
    input: String,
    output: Option<String>,
    mode: Mode,
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut mode = Mode::Interpret;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                output = Some(
                    iter.next()
                        .ok_or_else(|| format!("{} requires an argument", arg))?
                        .clone(),
                );
            }
            "--ast" => mode = Mode::Ast,
            s if s.starts_with('-') => return Err(format!("Unknown option: {}", s)),
            s => {
                if input.is_some() {
                    return Err("Multiple input files specified".to_string());
                }
                input = Some(s.to_string());
            }
        }
    }

    Ok(Args {
        input: input.ok_or(USAGE)?,
        output,
        mode,
    })
}

fn write_output(path: Option<&str>, text: &str) -> Result<(), String> {
    match path {
        None => {
            println!("{}", text);
            Ok(())
        }
        Some(p) => fs::write(p, text).map_err(|e| format!("could not write `{}`: {}", p, e)),
    }
}

fn read_and_parse(input: &str) -> Result<language::Program, ()> {
    let source = fs::read_to_string(input).map_err(|e| {
        eprintln!("error: could not read `{}`: {}", input, e);
    })?;
    language::parse(&source).map_err(|errors| {
        language::report_errors(&errors, &source, input);
    })
}

fn run_ast(input: &str, output: Option<&str>) -> bool {
    let program = match read_and_parse(input) {
        Ok(p) => p,
        Err(()) => return false,
    };
    if let Err(msg) = write_output(output, &program.to_string()) {
        eprintln!("error: {}", msg);
        return false;
    }
    true
}

fn run_interpreter(input: &str, output: Option<&str>) -> bool {
    let _program = match read_and_parse(input) {
        Ok(p) => p,
        Err(()) => return false,
    };
    // TODO: interpret program
    let _ = output;
    eprintln!("Interpreter not yet implemented");
    true
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{}", msg);
            process::exit(1);
        }
    };

    let ok = match args.mode {
        Mode::Ast => run_ast(&args.input, args.output.as_deref()),
        Mode::Interpret => run_interpreter(&args.input, args.output.as_deref()),
    };

    if !ok {
        process::exit(1);
    }
}
