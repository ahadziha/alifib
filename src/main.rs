use std::fs;
use std::process;
use std::time::Instant;

use alifib::aux::error::report_load_file_error;
use alifib::aux::loader::Loader;
use alifib::interpreter::{InterpretedFile, LoadResult};
use alifib::language::{self, Program};
use alifib::output;

const USAGE: &str = "Usage: alifib <input-file> [-o|--output <output-file>] [--ast] [--print] [--bench N]";

#[derive(Clone, Copy)]
enum RunMode {
    Interpret,
    Ast,
    Print,
    Bench(usize),
}

struct Args {
    input: String,
    output: Option<String>,
    mode: RunMode,
}

fn parse_args() -> Result<Args, String> {
    let cli_args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut mode = RunMode::Interpret;

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
                mode = RunMode::Bench(run_count);
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
    })
}

fn write_output(path: Option<&str>, text: &str) -> Result<(), ()> {
    match path {
        None => { println!("{}", text); Ok(()) }
        Some(p) => fs::write(p, text).map_err(|e| eprintln!("error: could not write `{}`: {}", p, e)),
    }
}

fn run_parse(loader: &Loader, input: &str, output: Option<&str>, render: impl Fn(&Program) -> String) -> Result<(), ()> {
    let program = loader.load_only_root(input).map_err(|e| report_load_file_error(&e))?;
    write_output(output, &render(&program))
}

fn run_interpreter(loader: &Loader, input: &str, output_path: Option<&str>) -> Result<(), ()> {
    let file = match InterpretedFile::load(loader, input) {
        LoadResult::Loaded(f) => f,
        other => { other.report(); return Err(()); }
    };
    write_output(output_path, &file.to_string())?;
    if file.has_holes() {
        output::report_holes(&file);
    }
    Ok(())
}

fn run_bench(loader: &Loader, input: &str, n: usize) -> Result<(), ()> {
    match InterpretedFile::load(loader, input) {
        LoadResult::Loaded(file) if file.has_holes() => {
            eprintln!("error: benchmark file contains holes");
            return Err(());
        }
        LoadResult::Loaded(_) => {}
        other => {
            other.report();
            eprintln!("error: benchmark file failed on warmup");
            return Err(());
        }
    }
    let start = Instant::now();
    for _ in 0..n {
        let _ = InterpretedFile::load(loader, input);
    }
    let elapsed = start.elapsed();
    println!("{:.3}", elapsed.as_secs_f64() * 1000.0 / n as f64);
    Ok(())
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => { eprintln!("{}", msg); process::exit(1); }
    };
    let loader = Loader::default(vec![]);
    let result = match args.mode {
        RunMode::Ast   => run_parse(&loader, &args.input, args.output.as_deref(), |p| p.to_string()),
        RunMode::Print => run_parse(&loader, &args.input, args.output.as_deref(), language::print_program),
        RunMode::Interpret => run_interpreter(&loader, &args.input, args.output.as_deref()),
        RunMode::Bench(n)  => run_bench(&loader, &args.input, n),
    };
    if result.is_err() {
        process::exit(1);
    }
}
