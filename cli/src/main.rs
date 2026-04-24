use std::fs;
use std::process;
use std::time::Instant;

use alifib::aux::error::report_load_file_error;
use alifib::aux::loader::Loader;
use alifib::interactive::cli::{RewriteCommand, ReplArgs, ServeArgs, SessionArgs, WebArgs, parse_rewrite_args, parse_repl_args, parse_serve_args, parse_session_args, parse_web_args, run_rewrite, run_repl_cmd, run_serve_cmd, run_session_cmd};
use alifib::interpreter::InterpretedFile;
use alifib::language;
use alifib::output;

const USAGE: &str = "\
Usage: alifib <input-file> [-o|--output <output-file>] [--ast] [--print] [--bench N]
       alifib rewrite <subcommand> [options]  (run 'alifib rewrite --help' for details)
       alifib repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]
       alifib session <file> --type <t> [--emacs]
       alifib web [--bind <addr>]
       alifib serve [<file> --type <t> --source <s> [--target <t>]]";

enum RunMode {
    Interpret,
    Ast,
    Print,
    Bench(usize),
    Rewrite(RewriteCommand),
    Repl(ReplArgs),
    Session(SessionArgs),
    Web(WebArgs),
    Serve(ServeArgs),
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

    // Check for top-level subcommands that consume all remaining args.
    match cli_args.first().map(|s| s.as_str()) {
        Some("rewrite") => {
            let cmd = parse_rewrite_args(&cli_args[1..])?;
            return Ok(Args { input: String::new(), output: None, mode: RunMode::Rewrite(cmd) });
        }
        Some("repl") => {
            let args = parse_repl_args(&cli_args[1..])?;
            return Ok(Args { input: String::new(), output: None, mode: RunMode::Repl(args) });
        }
        Some("session") => {
            let args = parse_session_args(&cli_args[1..])?;
            return Ok(Args { input: String::new(), output: None, mode: RunMode::Session(args) });
        }
        Some("web") => {
            let args = parse_web_args(&cli_args[1..])?;
            return Ok(Args { input: String::new(), output: None, mode: RunMode::Web(args) });
        }
        Some("serve") => {
            let args = parse_serve_args(&cli_args[1..])?;
            return Ok(Args { input: String::new(), output: None, mode: RunMode::Serve(args) });
        }
        _ => {}
    }

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

fn run_ast(loader: &Loader, input: &str, output: Option<&str>) -> Result<(), ()> {
    let program = loader.load_only_root(input).map_err(|e| report_load_file_error(&e))?;
    write_output(output, &program.to_string())
}

fn run_print(loader: &Loader, input: &str, output: Option<&str>) -> Result<(), ()> {
    let program = loader.load_only_root(input).map_err(|e| report_load_file_error(&e))?;
    write_output(output, &language::print_program(&program))
}

fn run_interpreter(loader: &Loader, input: &str, output_path: Option<&str>) -> Result<(), ()> {
    let file = InterpretedFile::load(loader, input).into_result()?;
    write_output(output_path, &file.to_string())?;
    if file.has_holes() {
        output::report_solved_holes(&file);
    }
    Ok(())
}

fn run_bench(loader: &Loader, input: &str, n: usize) -> Result<(), ()> {
    let file = InterpretedFile::load(loader, input).into_result()?;
    if file.has_holes() {
        eprintln!("error: benchmark file contains holes");
        return Err(());
    }
    let start = Instant::now();
    for _ in 0..n {
        let _ = InterpretedFile::load(loader, input);
    }
    let elapsed = start.elapsed();
    println!("{:.3}", elapsed.as_secs_f64() * 1000.0 / n as f64);
    Ok(())
}

#[allow(clippy::result_unit_err)]
fn run_web_cmd(args: WebArgs) -> Result<(), ()> {
    match alifib_web_server::run_web_server(&args.bind) {
        Ok(()) => Ok(()),
        Err(err) => {
            eprintln!("error: {}", err);
            Err(())
        }
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => { eprintln!("{}", msg); process::exit(1); }
    };
    let loader = Loader::default(vec![]);
    let result = match args.mode {
        RunMode::Ast       => run_ast(&loader, &args.input, args.output.as_deref()),
        RunMode::Print     => run_print(&loader, &args.input, args.output.as_deref()),
        RunMode::Interpret => run_interpreter(&loader, &args.input, args.output.as_deref()),
        RunMode::Bench(n)     => run_bench(&loader, &args.input, n),
        RunMode::Rewrite(cmd)  => run_rewrite(cmd),
        RunMode::Repl(args)    => run_repl_cmd(args),
        RunMode::Session(args) => run_session_cmd(args),
        RunMode::Web(args)     => run_web_cmd(args),
        RunMode::Serve(args)   => run_serve_cmd(args),
    };
    if result.is_err() {
        process::exit(1);
    }
}
