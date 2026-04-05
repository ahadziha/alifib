mod aux;
mod core;
mod interpreter;
mod language;

use std::fs;
use std::process;
use std::time::Instant;

use aux::error::report_load_file_error;
use aux::loader::Loader;
use interpreter::global_store::GlobalStore;
use interpreter::types::HoleInfo;
use interpreter::{Context, interpret_program};

const USAGE: &str = "Usage: alifib <input-file> [-o|--output <output-file>] [--ast] [--bench N]";

#[derive(Clone, Copy)]
enum RunMode {
    Interpret,
    Ast,
}

struct Args {
    input: String,
    output: Option<String>,
    mode: RunMode,
    bench: Option<usize>,
}

fn parse_args() -> Result<Args, String> {
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut mode = RunMode::Interpret;
    let mut bench = None;

    let mut arg_iter = cli_args.iter();
    while let Some(arg) = arg_iter.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                output = Some(
                    arg_iter
                        .next()
                        .ok_or_else(|| format!("{} requires an argument", arg))?
                        .clone(),
                );
            }
            "-h" | "--help" => {
                println!("{}", USAGE);
                process::exit(0);
            }
            "--ast" => mode = RunMode::Ast,
            "--bench" => {
                let run_count_str = arg_iter
                    .next()
                    .ok_or_else(|| "--bench requires a number".to_string())?;
                let run_count: usize = run_count_str
                    .parse()
                    .map_err(|_| format!("--bench: invalid number '{}'", run_count_str))?;
                bench = Some(run_count);
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

fn read_source(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("could not read `{}`: {}", path, error))
}

fn parse_program(path: &str, source: &str) -> Result<language::Program, ()> {
    match language::parse(source) {
        Ok(program) => Ok(program),
        Err(errors) => {
            language::report_errors(&errors, source, path);
            Err(())
        }
    }
}

struct RunResult {
    context: Context,
    source: String,
    canonical_path: String,
    holes: Vec<HoleInfo>,
}

fn execute_file(loader: &Loader, path: &str) -> Option<RunResult> {
    let loaded = match loader.load(path) {
        Ok(f) => f,
        Err(e) => {
            report_load_file_error(&e);
            return None;
        }
    };

    let context = Context::new(loaded.canonical_path.clone(), GlobalStore::empty());
    let result = interpret_program(&loaded.modules, context, &loaded.program);

    if !result.errors.is_empty() {
        language::report_errors(&result.errors, &loaded.source, &loaded.canonical_path);
        return None;
    }

    Some(RunResult {
        context: result.context,
        canonical_path: loaded.canonical_path,
        holes: result.holes,
        source: loaded.source,
    })
}

fn run_interpreter(input: &str, output: Option<&str>) -> bool {
    let loader = Loader::default(vec![]);
    let run_result = match execute_file(&loader, input) {
        Some(run_result) => run_result,
        None => return false,
    };

    let text = run_result.context.state.display();
    if let Err(msg) = write_output(output, &text) {
        eprintln!("error: {}", msg);
        return false;
    }
    if !run_result.holes.is_empty() {
        language::report_holes(
            &run_result.holes,
            &run_result.source,
            &run_result.canonical_path,
        );
    }
    true
}

fn run_bench(input: &str, n: usize) -> bool {
    let loader = Loader::default(vec![]);

    // Warmup
    match execute_file(&loader, input) {
        None => {
            eprintln!("error: benchmark file failed on warmup");
            return false;
        }
        Some(run_result) if !run_result.holes.is_empty() => {
            eprintln!("error: benchmark file contains holes");
            return false;
        }
        _ => {}
    }

    let start = Instant::now();
    for _ in 0..n {
        execute_file(&loader, input);
    }
    let elapsed = start.elapsed();
    let ms_per_run = elapsed.as_secs_f64() * 1000.0 / n as f64;
    println!("{:.3}", ms_per_run);
    true
}

fn run_ast(input: &str, output: Option<&str>) -> bool {
    let source = match read_source(input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("error: {}", error);
            return false;
        }
    };
    let program = match parse_program(input, &source) {
        Ok(program) => program,
        Err(()) => return false,
    };
    if let Err(msg) = write_output(output, &program.to_string()) {
        eprintln!("error: {}", msg);
        return false;
    }
    true
}

fn run(args: Args) -> bool {
    if let Some(n) = args.bench {
        return run_bench(&args.input, n);
    }

    match args.mode {
        RunMode::Ast => run_ast(&args.input, args.output.as_deref()),
        RunMode::Interpret => run_interpreter(&args.input, args.output.as_deref()),
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            eprintln!("{}", msg);
            process::exit(1);
        }
    };

    if !run(args) {
        process::exit(1);
    }
}
