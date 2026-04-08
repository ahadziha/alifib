//! CLI argument parsing and command dispatch for `alifib rewrite` and `alifib repl`.
//!
//! Commands:
//!   alifib rewrite init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
//!   alifib rewrite step   --session <p> --choice <n> [--format text|json]
//!   alifib rewrite undo   --session <p> [--format text|json]
//!   alifib rewrite show   --session <p> [--format text|json]
//!   alifib rewrite done   --session <p> [--format text|json]
//!   alifib repl <file> --type <t> --source <s> [--target <t>]

use crate::output::render_diagram;
use super::engine::replay_session;
use super::output::{AvailableRewrite, OutputFormat, RewriteResponse, Status};
use super::session::{Move, SessionFile};
use super::repl::run_repl;

/// Arguments for the `alifib repl` subcommand.
pub struct ReplArgs {
    pub file: String,
    pub type_name: String,
    pub source: String,
    pub target: Option<String>,
}

const REPL_USAGE: &str = "\
Usage: alifib repl <file> --type <t> --source <s> [--target <t>]
";

/// Parse the arguments following `alifib repl`.
pub fn parse_repl_args(args: &[String]) -> Result<ReplArgs, String> {
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
        source:    source.ok_or("repl: --source is required")?,
        target,
    })
}

/// Run the REPL with the given arguments.
pub fn run_repl_cmd(args: ReplArgs) -> Result<(), ()> {
    run_repl(&args.file, &args.type_name, &args.source, args.target.as_deref())
}

const REWRITE_USAGE: &str = "\
Usage: alifib rewrite <subcommand> [options]

Subcommands:
  init   --file <f> --type <t> --source <s> [--target <t>] --session <p> [--format text|json]
  step   --session <p> --choice <n> [--format text|json]
  undo   --session <p> [--format text|json]
  show   --session <p> [--format text|json]
  done   --session <p> [--format text|json]
";

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
    Done {
        session: String,
        format: OutputFormat,
    },
}

/// Parse the arguments following `alifib rewrite` into a [`RewriteCommand`].
pub fn parse_rewrite_args(args: &[String]) -> Result<RewriteCommand, String> {
    let sub = args.first().ok_or_else(|| REWRITE_USAGE.to_string())?;
    let rest = &args[1..];

    match sub.as_str() {
        "init" => parse_init(rest),
        "step" => parse_step(rest),
        "undo" => parse_undo(rest),
        "show" => parse_show(rest),
        "done" => parse_done(rest),
        "-h" | "--help" => Err(REWRITE_USAGE.to_string()),
        other => Err(format!("unknown rewrite subcommand '{}'\n{}", other, REWRITE_USAGE)),
    }
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

fn parse_done(args: &[String]) -> Result<RewriteCommand, String> {
    let (session, format) = parse_session_and_format(args, "done")?;
    Ok(RewriteCommand::Done { session, format })
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

/// Execute a parsed [`RewriteCommand`], printing output and returning
/// `Ok(())` on success or `Err(())` on failure (error already printed).
pub fn run_rewrite(cmd: RewriteCommand) -> Result<(), ()> {
    match cmd {
        RewriteCommand::Init { file, type_name, source, target, session: session_path, format } => {
            let session = SessionFile {
                source_file: file,
                type_name,
                source_diagram: source,
                target_diagram: target,
                moves: vec![],
            };
            run_and_print(session, &session_path, true, format)
        }
        RewriteCommand::Step { session: session_path, choice, format } => {
            let session = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { print_error(&e, format); return Err(()); }
            };
            // Replay to get current state so we can record the rule name for this choice.
            let state = match replay_session(session.clone()) {
                Ok(s) => s,
                Err(e) => { print_error(&e, format); return Err(()); }
            };
            let candidate = match state.available_rewrites.get(choice) {
                Some(c) => c,
                None => {
                    let msg = format!(
                        "choice {} out of range ({} rewrite(s) available)",
                        choice,
                        state.available_rewrites.len()
                    );
                    print_error(&msg, format);
                    return Err(());
                }
            };
            let mov = Move { choice, rule_name: candidate.rule_name.clone() };
            let mut updated = session;
            updated.moves.push(mov);
            run_and_print(updated, &session_path, true, format)
        }
        RewriteCommand::Undo { session: session_path, format } => {
            let mut session = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { print_error(&e, format); return Err(()); }
            };
            if session.moves.is_empty() {
                print_error("nothing to undo", format);
                return Err(());
            }
            session.moves.pop();
            run_and_print(session, &session_path, true, format)
        }
        RewriteCommand::Show { session: session_path, format } => {
            let session = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { print_error(&e, format); return Err(()); }
            };
            run_and_print(session, &session_path, false, format)
        }
        RewriteCommand::Done { session: session_path, format } => {
            let session = match SessionFile::read(&session_path) {
                Ok(s) => s,
                Err(e) => { print_error(&e, format); return Err(()); }
            };
            run_and_print(session, &session_path, false, format)
        }
    }
}

/// Replay `session`, write it back to disk if `write_back` is true, then
/// print the [`RewriteResponse`]. Returns `Err(())` if anything fails.
fn run_and_print(
    session: SessionFile,
    session_path: &str,
    write_back: bool,
    format: OutputFormat,
) -> Result<(), ()> {
    let step_count = session.moves.len();
    let state = match replay_session(session) {
        Ok(s) => s,
        Err(e) => { print_error(&e, format); return Err(()); }
    };

    if write_back {
        if let Err(e) = state.session.write(session_path) {
            print_error(&e, format);
            return Err(());
        }
    }

    let current_str = render_diagram(&state.current_diagram, &state.type_complex);
    let target_str = state.target_diagram.as_ref()
        .map(|t| render_diagram(t, &state.type_complex));
    let target_reached = state.target_reached();

    let available: Vec<AvailableRewrite> = state.available_rewrites
        .iter()
        .enumerate()
        .map(|(i, c)| AvailableRewrite {
            index: i,
            rule_name: c.rule_name.clone(),
            rule_source: render_diagram(&c.source_boundary, &state.type_complex),
            rule_target: render_diagram(&c.target_boundary, &state.type_complex),
        })
        .collect();

    let response = RewriteResponse {
        status: Status::Ok,
        step_count,
        current_diagram: current_str,
        target_diagram: target_str,
        target_reached,
        available_rewrites: available,
        error: None,
    };

    response.print(format);
    Ok(())
}

fn print_error(msg: &str, format: OutputFormat) {
    RewriteResponse::error(msg).print(format);
}
