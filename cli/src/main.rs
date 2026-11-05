use std::fs;
use std::net::TcpListener;
use std::num::NonZeroUsize;
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use alifib::aux::loader::Loader;
use alifib::interactive::kernel::CanonicalDir;
use alifib::interpreter::{InterpretedFile, LoadFailure};
use alifib::language;
use alifib_cli::display::Display;
use alifib_cli::examples::ExampleSet;
use alifib_cli::server::{self, Limits, Ui};
use alifib_cli::{client, repl};

const USAGE: &str = "\
Usage: alifib <input-file> [-o|--output <output-file>] [--ast] [--print] [--bench N]
       alifib repl <file> [<line>] [--emacs]
       alifib web [<dir>] [--bind <addr>]
       alifib serve [<dir>] [--bind <addr>]
       alifib do [--at <addr>] <line>";

/// Where `web` and `serve` bind, and where `do` calls, by default.
const DEFAULT_ADDRESS: &str = "127.0.0.1:8000";

/// One invocation, with exactly the arguments it uses.
enum Command {
    File {
        input: String,
        output: Option<String>,
        action: FileAction,
    },
    Repl {
        file: String,
        line: Option<String>,
        emacs: bool,
    },
    Web(ServerArgs),
    Serve(ServerArgs),
    Do {
        address: String,
        line: String,
    },
}

enum FileAction {
    Interpret,
    Ast,
    Print,
    Bench(NonZeroUsize),
}

struct ServerArgs {
    bind: String,
    dir: String,
}

/// Why parsing stopped short of a command.
enum Stop {
    Help,
    Error(String),
}

fn error(message: impl Into<String>) -> Stop {
    Stop::Error(message.into())
}

fn unknown(command: &str, option: &str) -> Stop {
    error(format!("{command}: unknown option '{option}'"))
}

/// The value following `flag`.
fn value<'a>(args: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, Stop> {
    args.next()
        .cloned()
        .ok_or_else(|| error(format!("{flag} requires an argument")))
}

/// Refuse positional arguments beyond those `command` took.
fn exhausted(mut positionals: impl Iterator<Item = String>, command: &str) -> Result<(), Stop> {
    match positionals.next() {
        Some(_) => Err(error(format!("{command}: too many arguments"))),
        None => Ok(()),
    }
}

fn parse(args: &[String]) -> Result<Command, Stop> {
    match args.first().map(String::as_str) {
        Some("repl") => parse_repl(&args[1..]),
        Some("web") => parse_server(&args[1..], "web").map(Command::Web),
        Some("serve") => parse_server(&args[1..], "serve").map(Command::Serve),
        Some("do") => parse_do(&args[1..]),
        _ => parse_file(args),
    }
}

fn parse_file(args: &[String]) -> Result<Command, Stop> {
    let mut output = None;
    let mut action = FileAction::Interpret;
    let mut positionals = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => output = Some(value(&mut args, arg)?),
            "-h" | "--help" => return Err(Stop::Help),
            "--ast" => action = FileAction::Ast,
            "--print" => action = FileAction::Print,
            "--bench" => {
                let runs = value(&mut args, arg)?;
                let runs = runs
                    .parse()
                    .map_err(|_| error(format!("--bench: '{runs}' is not a positive number")))?;
                action = FileAction::Bench(runs);
            }
            option if option.starts_with('-') => return Err(unknown("alifib", option)),
            _ => positionals.push(arg.clone()),
        }
    }
    let mut positionals = positionals.into_iter();
    let input = positionals
        .next()
        .ok_or_else(|| error("alifib: <input-file> argument is required"))?;
    exhausted(positionals, "alifib")?;
    Ok(Command::File {
        input,
        output,
        action,
    })
}

fn parse_repl(args: &[String]) -> Result<Command, Stop> {
    let mut emacs = false;
    let mut positionals = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--emacs" => emacs = true,
            "-h" | "--help" => return Err(Stop::Help),
            option if option.starts_with('-') => return Err(unknown("repl", option)),
            _ => positionals.push(arg.clone()),
        }
    }
    let mut positionals = positionals.into_iter();
    let file = positionals
        .next()
        .ok_or_else(|| error("repl: <file> argument is required"))?;
    let line = positionals.next();
    exhausted(positionals, "repl")?;
    Ok(Command::Repl { file, line, emacs })
}

fn parse_server(args: &[String], command: &str) -> Result<ServerArgs, Stop> {
    let mut bind = DEFAULT_ADDRESS.to_owned();
    let mut positionals = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => bind = value(&mut args, arg)?,
            "-h" | "--help" => return Err(Stop::Help),
            option if option.starts_with('-') => return Err(unknown(command, option)),
            _ => positionals.push(arg.clone()),
        }
    }
    let mut positionals = positionals.into_iter();
    let dir = positionals.next().unwrap_or_else(|| ".".to_owned());
    exhausted(positionals, command)?;
    Ok(ServerArgs { bind, dir })
}

fn parse_do(args: &[String]) -> Result<Command, Stop> {
    let mut address = DEFAULT_ADDRESS.to_owned();
    let mut positionals = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--at" => address = value(&mut args, arg)?,
            "-h" | "--help" => return Err(Stop::Help),
            option if option.starts_with('-') => return Err(unknown("do", option)),
            _ => positionals.push(arg.clone()),
        }
    }
    let mut positionals = positionals.into_iter();
    let line = positionals
        .next()
        .ok_or_else(|| error("do: <line> argument is required"))?;
    exhausted(positionals, "do")?;
    Ok(Command::Do { address, line })
}

fn write_output(path: Option<&str>, text: &str) -> ExitCode {
    let Some(path) = path else {
        println!("{text}");
        return ExitCode::SUCCESS;
    };
    match fs::write(path, text) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: could not write `{path}`: {error}");
            ExitCode::FAILURE
        }
    }
}

fn report(failure: &LoadFailure) -> ExitCode {
    Display::stderr().failure(Some(&failure.message), &failure.diagnostics);
    ExitCode::FAILURE
}

fn run_file(input: &str, output: Option<&str>, action: FileAction) -> ExitCode {
    let loader = Loader::default(vec![]);
    let text = match action {
        FileAction::Interpret => InterpretedFile::load(&loader, input).map(|file| file.to_string()),
        FileAction::Ast => loader
            .load_only_root(input)
            .map(|program| program.to_string())
            .map_err(LoadFailure::from),
        FileAction::Print => loader
            .load_only_root(input)
            .map(|program| language::print_program(&program))
            .map_err(LoadFailure::from),
        FileAction::Bench(runs) => return run_bench(&loader, input, runs),
    };
    match text {
        Ok(text) => write_output(output, &text),
        Err(failure) => report(&failure),
    }
}

fn run_bench(loader: &Loader, input: &str, runs: NonZeroUsize) -> ExitCode {
    if let Err(failure) = InterpretedFile::load(loader, input) {
        return report(&failure);
    }
    let start = Instant::now();
    for _ in 0..runs.get() {
        let _ = InterpretedFile::load(loader, input);
    }
    println!(
        "{:.3}",
        start.elapsed().as_secs_f64() * 1000.0 / runs.get() as f64
    );
    ExitCode::SUCCESS
}

/// Resolve the directory and bind, then serve the interface over that directory.
fn run_server(args: &ServerArgs, ui: impl FnOnce(CanonicalDir) -> Ui) -> ExitCode {
    let dir = match CanonicalDir::new(Path::new(&args.dir)) {
        Ok(dir) => dir,
        Err(error) => {
            eprintln!("could not resolve directory {}: {error}", args.dir);
            return ExitCode::FAILURE;
        }
    };
    let listener = match TcpListener::bind(&args.bind) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("could not bind {}: {error}", args.bind);
            return ExitCode::FAILURE;
        }
    };
    let Err(error) = server::run(listener, dir.clone(), ui(dir), Limits::DEFAULT);
    eprintln!("error: {error}");
    ExitCode::FAILURE
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Command::File {
            input,
            output,
            action,
        }) => run_file(&input, output.as_deref(), action),
        Ok(Command::Repl { file, line, emacs }) => repl::run(&file, line.as_deref(), emacs),
        Ok(Command::Web(args)) => run_server(&args, |dir| Ui::Browser(ExampleSet::new(dir))),
        Ok(Command::Serve(args)) => run_server(&args, |_| Ui::Headless),
        Ok(Command::Do { address, line }) => client::run(&address, &line),
        Err(Stop::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(Stop::Error(message)) => {
            eprintln!("{message}\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}
