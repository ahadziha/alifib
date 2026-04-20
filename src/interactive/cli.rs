//! CLI argument parsing and command dispatch for `alifib rewrite` and `alifib repl`.
//!
//! # Commands
//!
//! ```text
//! alifib rewrite init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
//! alifib rewrite step   --session <p> --choice <n> [--format text|json]
//! alifib rewrite undo   --session <p> [--format text|json]
//! alifib rewrite show   --session <p> [--format text|json]
//! alifib repl <file>    [--type <t>] [--source <s>] [--target <t>] [--emacs]
//! ```

use serde::Serialize;

use crate::output::render_diagram;
use super::engine::RewriteEngine;
use super::session::SessionFile;
#[cfg(feature = "cli")]
use super::repl::run_repl;
#[cfg(feature = "cli")]
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

/// Output format for `alifib rewrite` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text (the default).
    Text,
    /// Pretty-printed JSON, suitable for machine consumption.
    Json,
}

impl OutputFormat {
    /// Parse `"text"` or `"json"`, returning an error for anything else.
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
                .map(|(i, m)| {
                    let n_plus_1 = m.step.top_dim();
                    let n = n_plus_1.saturating_sub(1);
                    let rule_tag = m.step.labels_at(n_plus_1).and_then(|ls| ls.first());
                    let classifier = rule_tag
                        .and_then(|tag| scope.find_generator_by_tag(tag))
                        .and_then(|name| scope.classifier(name));
                    let (rule_source, rule_target) = classifier
                        .and_then(|cl| {
                            let s = crate::core::diagram::Diagram::boundary(
                                crate::core::diagram::Sign::Source, n, cl).ok()?;
                            let t = crate::core::diagram::Diagram::boundary(
                                crate::core::diagram::Sign::Target, n, cl).ok()?;
                            Some((render_diagram(&s, scope), render_diagram(&t, scope)))
                        })
                        .unwrap_or_else(|| ("?".into(), "?".into()));
                    AvailableRewrite {
                        index: i,
                        rule_name: m.rule_name.clone(),
                        rule_source,
                        rule_target,
                    }
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
    /// Initialise a fresh session from a source diagram and write the move log.
    Init {
        /// Path to the `.ali` source file.
        file: String,
        /// Name of the type whose generators are the rewrite rules.
        type_name: String,
        /// Name of the source diagram within the type.
        source: String,
        /// Optional name of the target (goal) diagram.
        target: Option<String>,
        /// Path where the session JSON file will be written.
        session: String,
        format: OutputFormat,
    },
    /// Apply one rewrite step and update the session file.
    Step {
        /// Path to the existing session JSON file.
        session: String,
        /// Index into the sorted candidate list at the current state.
        choice: usize,
        format: OutputFormat,
    },
    /// Undo the last step and update the session file.
    Undo {
        /// Path to the existing session JSON file.
        session: String,
        format: OutputFormat,
    },
    /// Display the current session state without mutating the file.
    Show {
        /// Path to the existing session JSON file.
        session: String,
        format: OutputFormat,
    },
}

/// Arguments for the `alifib repl` subcommand.
pub struct ReplArgs {
    /// Path to the `.ali` source file.
    pub file: String,
    /// Pre-selected type name (set via `@ <TypeName>` interactively if absent).
    pub type_name: Option<String>,
    /// Pre-selected source diagram name.
    pub source: Option<String>,
    /// Pre-selected target diagram name.
    pub target: Option<String>,
    /// Use Emacs keybindings instead of the default vi mode.
    pub emacs: bool,
}

/// Arguments for the `alifib session` subcommand.
pub struct SessionArgs {
    /// Path to the `.ali` source file.
    pub file: String,
    /// Name of the type to work in (required).
    pub type_name: String,
    /// Use Emacs keybindings instead of the default vi mode.
    pub emacs: bool,
}

/// Arguments for the `alifib serve` subcommand.
///
/// All fields are optional: `alifib serve` with no arguments starts blank
/// and waits for an `Init` request; with arguments it pre-loads the session
/// and emits an initial state response before entering the request loop.
pub struct ServeArgs {
    /// Path to the `.ali` source file (required if any of the others are given).
    pub file: Option<String>,
    /// Name of the type to load (required if `file` is given).
    pub type_name: Option<String>,
    /// Name of the source diagram (required if `file` is given).
    pub source: Option<String>,
    /// Optional name of the target (goal) diagram.
    pub target: Option<String>,
}

/// Arguments for the `alifib web` subcommand.
pub struct WebArgs {
    /// Address to bind the localhost HTTP server to.
    pub bind: String,
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
Usage: alifib repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]
";

const WEB_USAGE: &str = "\
Usage: alifib web [--bind <addr>]
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
        type_name,
        source,
        target,
        emacs,
    })
}

/// Parse the arguments following `alifib web`.
pub fn parse_web_args(args: &[String]) -> Result<WebArgs, String> {
    let mut bind = "127.0.0.1:8000".to_string();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--bind" => { bind = next_arg(&mut it, "--bind")?; }
            "-h" | "--help" => return Err(WEB_USAGE.to_string()),
            s if s.starts_with('-') => {
                return Err(format!("unknown option '{}' for web\n{}", s, WEB_USAGE));
            }
            s => {
                return Err(format!("unexpected argument '{}' for web\n{}", s, WEB_USAGE));
            }
        }
    }

    Ok(WebArgs { bind })
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
        .cloned()
}

// ── Dispatchers ───────────────────────────────────────────────────────────────

/// Execute a parsed [`RewriteCommand`].
#[allow(clippy::result_unit_err)]
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
#[allow(clippy::result_unit_err)]
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

/// Run the localhost web server with the given arguments.
#[allow(clippy::result_unit_err)]
pub fn run_web_cmd(args: WebArgs) -> Result<(), ()> {
    match super::web_server::run_web_server(&args.bind) {
        Ok(()) => Ok(()),
        Err(err) => {
            eprintln!("error: {}", err);
            Err(())
        }
    }
}

/// Run the REPL with the given arguments.
#[cfg(feature = "cli")]
#[allow(clippy::result_unit_err)]
pub fn run_repl_cmd(args: ReplArgs) -> Result<(), ()> {
    run_repl(&args.file, args.type_name.as_deref(), args.source.as_deref(), args.target.as_deref(), args.emacs)
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
#[cfg(feature = "cli")]
#[allow(clippy::result_unit_err)]
pub fn run_session_cmd(args: SessionArgs) -> Result<(), ()> {
    run_session(&args.file, &args.type_name, args.emacs)
}
