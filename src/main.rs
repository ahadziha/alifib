use std::fs;
use std::process;
use std::time::Instant;

use alifib::aux::error::report_load_file_error;
use alifib::aux::loader::Loader;
use alifib::interpreter::{InterpretedFile, LoadResult};
use alifib::language;
use alifib::output;

const USAGE: &str = "Usage: alifib <input-file> [-o|--output <output-file>] [--ast] [--print] [--bench N]";

#[derive(Clone, Copy)]
enum RunMode {
    Interpret,
    Ast,
    Print,
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
            "--print" => mode = RunMode::Print,
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

fn run_ast(loader: &Loader, input: &str, output: Option<&str>) -> bool {
    match loader.load_only_root(input) {
        Err(e) => { report_load_file_error(&e); false }
        Ok(program) => {
            if let Err(msg) = write_output(output, &program.to_string()) {
                eprintln!("error: {}", msg);
                return false;
            }
            true
        }
    }
}

fn run_print(loader: &Loader, input: &str, output: Option<&str>) -> bool {
    match loader.load_only_root(input) {
        Err(e) => { report_load_file_error(&e); false }
        Ok(program) => {
            if let Err(msg) = write_output(output, &language::print_program(&program)) {
                eprintln!("error: {}", msg);
                return false;
            }
            true
        }
    }
}

fn run_interpreter(loader: &Loader, input: &str, output_path: Option<&str>) -> bool {
    let result = InterpretedFile::load(loader, input);
    let file = match result {
        LoadResult::Loaded(f) => f,
        other => { other.report(); return false; }
    };
    if let Err(msg) = write_output(output_path, &file.to_string()) {
        eprintln!("error: {}", msg);
        return false;
    }
    if file.has_holes() {
        output::report_holes(&file);
    }
    true
}

fn run_bench(loader: &Loader, input: &str, n: usize) -> bool {
    match InterpretedFile::load(loader, input) {
        LoadResult::Loaded(file) if file.has_holes() => {
            eprintln!("error: benchmark file contains holes");
            return false;
        }
        LoadResult::Loaded(_) => {}
        other => {
            other.report();
            eprintln!("error: benchmark file failed on warmup");
            return false;
        }
    }
    let start = Instant::now();
    for _ in 0..n {
        let _ = InterpretedFile::load(loader, input);
    }
    let elapsed = start.elapsed();
    println!("{:.3}", elapsed.as_secs_f64() * 1000.0 / n as f64);
    true
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            eprintln!("{}", msg);
            process::exit(1);
        }
    };
    let loader = Loader::default(vec![]);
    let ok = if let Some(n) = args.bench {
        run_bench(&loader, &args.input, n)
    } else {
        match args.mode {
            RunMode::Ast => run_ast(&loader, &args.input, args.output.as_deref()),
            RunMode::Print => run_print(&loader, &args.input, args.output.as_deref()),
            RunMode::Interpret => run_interpreter(&loader, &args.input, args.output.as_deref()),
        }
    };
    if !ok {
        process::exit(1);
    }
}
