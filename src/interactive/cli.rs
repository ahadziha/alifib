//! CLI argument parsing and command dispatch for `alifib rewrite` and `alifib repl`.
//!
//! Commands:
//!   alifib rewrite init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
//!   alifib rewrite step   --session <p> --choice <n> [--format text|json]
//!   alifib rewrite undo   --session <p> [--format text|json]
//!   alifib rewrite show   --session <p> [--format text|json]
//!   alifib repl <file> --type <t> --source <s> [--target <t>]

use serde::Serialize;

use crate::output::render_diagram;
use super::engine::RewriteEngine;
use super::session::SessionFile;
use super::repl::run_repl;
use super::session_repl::run_session;

const REWRITE_USAGE: &str = "\
Usage: alifib rewrite <subcommand> [options]

Subcommands:
  init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
  step   --session <p> --choice <n> [--format text|json]
  undo   --session <p> [--format text|json]
  show   --session <p> [--format text|json]
";

// ── Output types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat { Text, Json }

impl OutputFormat {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            other  => Err(format!("unknown format '{}': expected 'text' or 'json'", other)),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status { Ok, Error }

#[derive(Debug, Serialize)]
struct AvailableRewrite {
    index: usize,
    rule_name: String,
    rule_source: String,
    rule_target: String,
}

#[derive(Debug, Serialize)]
struct RewriteResponse {
    status: Status,
    step_count: usize,
    current_diagram: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_diagram: Option<String>,
    target_reached: bool,
    available_rewrites: Vec<AvailableRewrite>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl RewriteResponse {
    fn from_engine(engine: &RewriteEngine) -> Self {
        let scope = engine.type_complex();
        Self {
            status: Status::Ok,
            step_count: engine.step_count(),
            current_diagram: render_diagram(engine.current_diagram(), scope),
            target_diagram: engine.target_diagram().map(|t| render_diagram(t, scope)),
            target_reached: engine.target_reached(),
            available_rewrites: engine
                .available_rewrites()
                .iter()
                .enumerate()
                .map(|(i, c)| AvailableRewrite {
                    index: i,
                    rule_name: c.rule_name.clone(),
                    rule_source: render_diagram(&c.source_boundary, scope),
                    rule_target: render_diagram(&c.target_boundary, scope),
                })
                .collect(),
            error: None,
        }
    }

    fn error(msg: impl Into<String>) -> Self {
        Self {
            status: Status::Error,
            step_count: 0,
            current_diagram: String::new(),
            target_diagram: None,
            target_reached: false,
            available_rewrites: vec![],
            error: Some(msg.into()),
        }
    }

    fn print(&self, format: OutputFormat) {
        match format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(self).unwrap()),
            OutputFormat::Text => self.print_text(),
        }
    }

    fn print_text(&self) {
        if let Status::Error = self.status {
            eprintln!("error: {}", self.error.as_deref().unwrap_or("unknown"));
            return;
        }
        println!("step: {}", self.step_count);
        println!("current: {}", self.current_diagram);
        if let Some(t) = &self.target_diagram {
            println!("target:  {}", t);
        }
        if self.target_reached {
            println!("target reached.");
        }
        if self.available_rewrites.is_empty() {
            println!("no rewrites available.");
        } else {
            println!("\navailable rewrites:");
            for r in &self.available_rewrites {
                println!("  [{}] {}  :  {}  ->  {}", r.index, r.rule_name, r.rule_source, r.rule_target);
            }
        }
    }
}

// ── Rewrite subcommand types ──────────────────────────────────────────────────

/// A parsed `alifib rewrite` subcommand and its arguments.
pub enum RewriteCommand {
    Init {
        file: String,
        type_name: String,
        source: String,
        target: Option<String>,
        session: String,
        format: OutputFormat,
    },
    Step {
        session: String,
        choice: usize,
        format: OutputFormat,
    },
    Undo {
        session: String,
        format: OutputFormat,
    },
    Show {
        session: String,
        format: OutputFormat,
    },
}

/// Arguments for the `alifib repl` subcommand.
pub struct ReplArgs {
    pub file: String,
    pub type_name: String,
    pub source: Option<String>,
    pub target: Option<String>,
    pub emacs: bool,
}

/// Arguments for the `alifib session` subcommand.
pub struct SessionArgs {
    pub file: String,
    pub type_name: String,
    pub emacs: bool,
}

/// Arguments for the `alifib serve` subcommand.
///
/// All fields are optional: `alifib serve` with no arguments starts blank
/// and waits for an `Init` request; with arguments it pre-loads the session
/// and emits an initial state response before entering the request loop.
pub struct ServeArgs {
    pub file: Option<String>,
    pub type_name: Option<String>,
    pub source: Option<String>,
    pub target: Option<String>,
}

// ── Parsers ───────────────────────────────────────────────────────────────────

/// Parse the arguments following `alifib rewrite` into a [`RewriteCommand`].
pub fn parse_rewrite_args(args: &[String]) -> Result<RewriteCommand, String> {
    let sub = args.first().ok_or_else(|| REWRITE_USAGE.to_string())?;
    let rest = &args[1..];
    match sub.as_str() {
        "init" => parse_init(rest),
        "step" => parse_step(rest),
        "undo" => parse_undo(rest),
        "show" => parse_show(rest),
        "-h" | "--help" => Err(REWRITE_USAGE.to_string()),
        other => Err(format!("unknown rewrite subcommand '{}'\n{}", other, REWRITE_USAGE)),
    }
}

const REPL_USAGE: &str = "\
Usage: alifib repl <file> --type <t> [--source <s>] [--target <t>] [--emacs]
";

/// Parse the arguments following `alifib repl`.
pub fn parse_repl_args(args: &[String]) -> Result<ReplArgs, String> {
    let mut file = None;
    let mut type_name = None;
    let mut source = None;
    let mut target = None;
    let mut emacs = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--type"   => { type_name = Some(next_arg(&mut it, "--type")?); }
            "--source" => { source    = Some(next_arg(&mut it, "--source")?); }
            "--target" => { target    = Some(next_arg(&mut it, "--target")?); }
            "--emacs"  => { emacs = true; }
            "-h" | "--help" => return Err(REPL_USAGE.to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for repl\n{}", s, REPL_USAGE));
            }
            s => {
                if file.is_some() {
                    return Err("repl: multiple input files specified".to_string());
                }
                file = Some(s.to_string());
            }
        }
    }

    Ok(ReplArgs {
        file:      file.ok_or("repl: <file> argument is required")?,
        type_name: type_name.ok_or("repl: --type is required")?,
        source,
        target,
        emacs,
    })
}

fn parse_init(args: &[String]) -> Result<RewriteCommand, String> {
    let mut file = None;
    let mut type_name = None;
    let mut source = None;
    let mut target = None;
    let mut session = None;
    let mut format = OutputFormat::Text;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--file"    => { file      = Some(next_arg(&mut it, "--file")?);    }
            "--type"    => { type_name = Some(next_arg(&mut it, "--type")?);    }
            "--source"  => { source    = Some(next_arg(&mut it, "--source")?);  }
            "--target"  => { target    = Some(next_arg(&mut it, "--target")?);  }
            "--session" => { session   = Some(next_arg(&mut it, "--session")?); }
            "--format"  => {
                let s = next_arg(&mut it, "--format")?;
                format = OutputFormat::parse(&s)?;
            }
            other => return Err(format!("unknown option '{}' for rewrite init", other)),
        }
    }

    Ok(RewriteCommand::Init {
        file:      file.ok_or("rewrite init: --file is required")?,
        type_name: type_name.ok_or("rewrite init: --type is required")?,
        source:    source.ok_or("rewrite init: --source is required")?,
        target,
        session:   session.ok_or("rewrite init: --session is required")?,
        format,
    })
}

fn parse_step(args: &[String]) -> Result<RewriteCommand, String> {
    let mut session = None;
    let mut choice = None;
    let mut format = OutputFormat::Text;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--session" => { session = Some(next_arg(&mut it, "--session")?); }
            "--choice"  => {
                let s = next_arg(&mut it, "--choice")?;
                let n: usize = s.parse().map_err(|_| format!("--choice: invalid number '{}'", s))?;
                choice = Some(n);
            }
            "--format" => {
                let s = next_arg(&mut it, "--format")?;
                format = OutputFormat::parse(&s)?;
            }
            other => return Err(format!("unknown option '{}' for rewrite step", other)),
        }
    }

    Ok(RewriteCommand::Step {
        session: session.ok_or("rewrite step: --session is required")?,
        choice:  choice.ok_or("rewrite step: --choice is required")?,
        format,
    })
}

fn parse_undo(args: &[String]) -> Result<RewriteCommand, String> {
    let (session, format) = parse_session_and_format(args, "undo")?;
    Ok(RewriteCommand::Undo { session, format })
}

fn parse_show(args: &[String]) -> Result<RewriteCommand, String> {
    let (session, format) = parse_session_and_format(args, "show")?;
    Ok(RewriteCommand::Show { session, format })
}

fn parse_session_and_format(args: &[String], sub: &str) -> Result<(String, OutputFormat), String> {
    let mut session = None;
    let mut format = OutputFormat::Text;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--session" => { session = Some(next_arg(&mut it, "--session")?); }
            "--format" => {
                let s = next_arg(&mut it, "--format")?;
                format = OutputFormat::parse(&s)?;
            }
            other => return Err(format!("unknown option '{}' for rewrite {}", other, sub)),
        }
    }

    Ok((session.ok_or(format!("rewrite {}: --session is required", sub))?, format))
}

fn next_arg<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    it.next()
        .ok_or_else(|| format!("{} requires an argument", flag))
        .map(|s| s.clone())
}

// ── Dispatchers ───────────────────────────────────────────────────────────────

/// Execute a parsed [`RewriteCommand`].
pub fn run_rewrite(cmd: RewriteCommand) -> Result<(), ()> {
    match cmd {
        RewriteCommand::Init { file, type_name, source, target, session: session_path, format } => {
            let engine = match RewriteEngine::init(&file, &type_name, &source, target.as_deref()) {
                Ok(e) => e,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            let sf = engine.to_session_file();
            if let Err(e) = sf.write(&session_path) {
                RewriteResponse::error(e).print(format);
                return Err(());
            }
            RewriteResponse::from_engine(&engine).print(format);
            Ok(())
        }
        RewriteCommand::Step { session: session_path, choice, format } => {
            let sf = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            let mut engine = match RewriteEngine::from_session(sf) {
                Ok(e) => e,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            if let Err(e) = engine.step(choice) {
                RewriteResponse::error(e).print(format);
                return Err(());
            }
            if let Err(e) = engine.to_session_file().write(&session_path) {
                RewriteResponse::error(e).print(format);
                return Err(());
            }
            RewriteResponse::from_engine(&engine).print(format);
            Ok(())
        }
        RewriteCommand::Undo { session: session_path, format } => {
            let sf = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            let mut engine = match RewriteEngine::from_session(sf) {
                Ok(e) => e,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            if let Err(e) = engine.undo() {
                RewriteResponse::error(e).print(format);
                return Err(());
            }
            if let Err(e) = engine.to_session_file().write(&session_path) {
                RewriteResponse::error(e).print(format);
                return Err(());
            }
            RewriteResponse::from_engine(&engine).print(format);
            Ok(())
        }
        RewriteCommand::Show { session: session_path, format } => {
            let sf = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            let engine = match RewriteEngine::from_session(sf) {
                Ok(e) => e,
                Err(e) => { RewriteResponse::error(e).print(format); return Err(()); }
            };
            RewriteResponse::from_engine(&engine).print(format);
            Ok(())
        }
    }
}

/// Parse the arguments following `alifib serve`.
pub fn parse_serve_args(args: &[String]) -> Result<ServeArgs, String> {
    let mut file = None;
    let mut type_name = None;
    let mut source = None;
    let mut target = None;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--type"   => { type_name = Some(next_arg(&mut it, "--type")?); }
            "--source" => { source    = Some(next_arg(&mut it, "--source")?); }
            "--target" => { target    = Some(next_arg(&mut it, "--target")?); }
            "-h" | "--help" => return Err("Usage: alifib serve [<file> --type <t> --source <s> [--target <t>]]\n".to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for serve", s));
            }
            s => {
                if file.is_some() {
                    return Err("serve: multiple input files specified".to_string());
                }
                file = Some(s.to_string());
            }
        }
    }

    Ok(ServeArgs { file, type_name, source, target })
}

/// Run the daemon, optionally pre-loading a session from the given arguments.
pub fn run_serve_cmd(args: ServeArgs) -> Result<(), ()> {
    use super::daemon::run_daemon;
    let initial = match (args.file, args.type_name, args.source) {
        (Some(file), Some(type_name), Some(source)) => {
            match RewriteEngine::init(&file, &type_name, &source, args.target.as_deref()) {
                Ok(e) => Some(e),
                Err(e) => { eprintln!("error: {}", e); return Err(()); }
            }
        }
        (None, None, None) => None,
        _ => {
            eprintln!("error: serve: if any of <file>, --type, --source are given, all three are required");
            return Err(());
        }
    };
    run_daemon(initial)
}

/// Run the REPL with the given arguments.
pub fn run_repl_cmd(args: ReplArgs) -> Result<(), ()> {
    run_repl(&args.file, &args.type_name, args.source.as_deref(), args.target.as_deref(), args.emacs)
}

const SESSION_USAGE: &str = "\
Usage: alifib session <file> --type <t> [--emacs]
";

/// Parse the arguments following `alifib session`.
pub fn parse_session_args(args: &[String]) -> Result<SessionArgs, String> {
    let mut file = None;
    let mut type_name = None;
    let mut emacs = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--type"  => { type_name = Some(next_arg(&mut it, "--type")?); }
            "--emacs" => { emacs = true; }
            "-h" | "--help" => return Err(SESSION_USAGE.to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for session\n{}", s, SESSION_USAGE));
            }
            s => {
                if file.is_some() {
                    return Err("session: multiple input files specified".to_string());
                }
                file = Some(s.to_string());
            }
        }
    }

    Ok(SessionArgs {
        file:      file.ok_or("session: <file> argument is required")?,
        type_name: type_name.ok_or("session: --type is required")?,
        emacs,
    })
}

/// Run the session REPL with the given arguments.
pub fn run_session_cmd(args: SessionArgs) -> Result<(), ()> {
    run_session(&args.file, &args.type_name, args.emacs)
}
