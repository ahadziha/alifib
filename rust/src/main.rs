mod aux;
mod core;
mod interpreter;
mod language;

use std::fs;
use std::process;
use std::time::Instant;

use aux::loader::{Loader, LoadError, ResolveError, ensure_root_in_loader, resolve_all_modules};
use interpreter::interpreter::{Context, interpret_program};
use interpreter::state::State;

const USAGE: &str = "Usage: alifib2 <input-file> [-o|--output <output-file>] [--ast] [--bench N]";

#[derive(Clone, Copy)]
enum Mode {
    Interpret,
    Ast,
}

struct Args {
    input: String,
    output: Option<String>,
    mode: Mode,
    bench: Option<usize>,
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut mode = Mode::Interpret;
    let mut bench = None;

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
            "--bench" => {
                let n_str = iter.next()
                    .ok_or_else(|| "--bench requires a number".to_string())?;
                let n: usize = n_str.parse()
                    .map_err(|_| format!("--bench: invalid number '{}'", n_str))?;
                bench = Some(n);
            }
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
        bench,
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

// ---- Session runner ----

fn run_file(loader: &Loader, path: &str) -> Option<(Context, String)> {
    let canonical_path = aux::path::canonicalize(path);
    let file_loader = loader.file_loader();

    let contents = match (file_loader.read_file)(&canonical_path) {
        Ok(s) => s,
        Err(LoadError::NotFound) => {
            eprintln!("error: could not load `{}`", path);
            return None;
        }
        Err(LoadError::IoError(reason)) => {
            eprintln!("error: could not load `{}`: {}", path, reason);
            return None;
        }
    };

    let file_loader = ensure_root_in_loader(file_loader, &canonical_path);

    let program = match language::parse(&contents) {
        Ok(p) => p,
        Err(parse_errors) => {
            language::report_errors(&parse_errors, &contents, &canonical_path);
            return None;
        }
    };

    // Pre-resolve all module includes
    let module_store = match resolve_all_modules(&file_loader, &canonical_path, &program) {
        Ok(store) => store,
        Err(e) => {
            report_resolve_error(&e);
            return None;
        }
    };

    let context = Context::new(canonical_path.clone(), State::empty());
    let result = interpret_program(&module_store, context, &program);

    if !result.errors.is_empty() {
        language::report_errors(&result.errors, &contents, &canonical_path);
        return None;
    }

    Some((result.context, contents))
}

fn report_resolve_error(err: &ResolveError) {
    match err {
        ResolveError::NotFound { module_name } => {
            eprintln!("error: module file {}.ali not found in search paths", module_name);
        }
        ResolveError::IoError { path, reason } => {
            eprintln!("error: could not load `{}`: {}", path, reason);
        }
        ResolveError::ParseError { path, source, errors } => {
            language::report_errors(errors, source, path);
        }
        ResolveError::Cycle { path } => {
            eprintln!("error: cyclic module dependency involving `{}`", path);
        }
    }
}

fn run_interpreter(input: &str, output: Option<&str>) -> bool {
    let loader = Loader::default(vec![]);
    let (context, _) = match run_file(&loader, input) {
        Some(pair) => pair,
        None => return false,
    };

    let text = context.state.display();
    if let Err(msg) = write_output(output, &text) {
        eprintln!("error: {}", msg);
        return false;
    }
    true
}

fn run_bench(input: &str, n: usize) -> bool {
    let loader = Loader::default(vec![]);

    // Warmup
    if run_file(&loader, input).is_none() {
        eprintln!("error: benchmark file failed on warmup");
        return false;
    }

    let start = Instant::now();
    for _ in 0..n {
        let loader = Loader::default(vec![]);
        run_file(&loader, input);
    }
    let elapsed = start.elapsed();
    let ms_per_run = elapsed.as_secs_f64() * 1000.0 / n as f64;
    println!("{:.3}", ms_per_run);
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

    let ok = if let Some(n) = args.bench {
        run_bench(&args.input, n)
    } else {
        match args.mode {
            Mode::Ast => run_ast(&args.input, args.output.as_deref()),
            Mode::Interpret => run_interpreter(&args.input, args.output.as_deref()),
        }
    };

    if !ok {
        process::exit(1);
    }
}
