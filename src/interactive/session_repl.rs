//! Session REPL: load an .ali file, add `let` bindings, prove goals
//! interactively, and export the result back into the type block.
//!
//! # Usage
//!
//! ```text
//! alifib session <file> --type <t>
//! ```
//!
//! # Commands
//!
//! ```text
//! let <name> = <expr>          Add a let binding (full alifib syntax, validated by re-interpretation)
//! goal <name> : <src> -> <tgt> Start a goal sub-loop to prove a new generator
//! show                         Show additions made this session
//! export                       Print session additions (for pasting into the source file)
//! export <path>                Write the full modified source to <path>
//! help / ?                     Show this help
//! quit / exit / q              Exit the session
//! ```

use std::io::{BufRead, Write};
use std::sync::Arc;

use super::engine::RewriteEngine;
use super::repl::{run_goal_loop, GoalOutcome};
use super::workspace::Workspace;

// ── Public entry point ────────────────────────────────────────────────────────

/// Run the session REPL for `source_file` / `type_name`.
pub fn run_session(source_file: &str, type_name: &str) -> Result<(), ()> {
    let mut ws = match Workspace::load(source_file, type_name) {
        Ok(w) => w,
        Err(e) => { eprintln!("error: {}", e); return Err(()); }
    };

    println!("Session: {}  ·  type {}", source_file, type_name);
    println!("Type 'help' for commands.");

    let stdin = std::io::stdin();
    loop {
        print!("session> ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => { eprintln!("read error: {}", e); break; }
        }
        let line = line.trim();
        if line.is_empty() { continue; }

        if let SessionResult::Quit = dispatch_session_command(&mut ws, line) {
            break;
        }
    }

    println!();
    Ok(())
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

enum SessionResult { Continue, Quit }

fn dispatch_session_command(ws: &mut Workspace, line: &str) -> SessionResult {
    // `let` bindings: the whole line is passed to the workspace validator.
    if line.starts_with("let ") {
        match ws.eval_let(line) {
            Ok(()) => {
                if let Some(a) = ws.additions().last() {
                    println!("Added: {}", a.name());
                }
            }
            Err(e) => eprintln!("error: {}", e),
        }
        return SessionResult::Continue;
    }

    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").trim();
    let rest = parts.next().map(str::trim).unwrap_or("");

    match cmd {
        "goal" | "g" => {
            match parse_goal_spec(rest) {
                Err(e) => eprintln!("error: {}", e),
                Ok((name, src_name, tgt_name)) => {
                    run_goal(ws, &name, &src_name, &tgt_name);
                }
            }
        }
        "show" => show_workspace(ws),
        "export" | "e" => {
            if rest.is_empty() {
                print!("{}", ws.export_additions());
            } else {
                let full = ws.export_full_source();
                match std::fs::write(rest, &full) {
                    Ok(()) => println!("Written to '{}'.", rest),
                    Err(e) => eprintln!("error: cannot write '{}': {}", rest, e),
                }
            }
        }
        "help" | "?" => print_session_help(),
        "quit" | "exit" | "q" => return SessionResult::Quit,
        other => eprintln!("unknown command '{}' — type 'help' for a list", other),
    }
    SessionResult::Continue
}

// ── Goal handling ─────────────────────────────────────────────────────────────

fn run_goal(ws: &mut Workspace, name: &str, src_name: &str, tgt_name: &str) {
    let type_complex = match ws.type_complex() {
        Ok(c) => c,
        Err(e) => { eprintln!("error: {}", e); return; }
    };

    let mut engine = match RewriteEngine::from_store(
        Arc::clone(ws.store()),
        type_complex,
        src_name,
        Some(tgt_name),
        ws.source_file().to_owned(),
        ws.type_name().to_owned(),
    ) {
        Ok(e) => e,
        Err(e) => { eprintln!("error: {}", e); return; }
    };

    println!("Goal '{}': {} -> {}", name, src_name, tgt_name);

    match run_goal_loop(&mut engine) {
        GoalOutcome::Abandoned => {
            println!("Goal abandoned.");
        }
        GoalOutcome::Done => {
            let proof = match engine.running_diagram() {
                None => {
                    eprintln!("error: no proof steps applied — goal not proved");
                    return;
                }
                Some(d) => d.clone(),
            };
            let moves = engine.to_session_file().moves;
            match ws.add_goal_result(name, src_name, tgt_name, proof, moves) {
                Ok(()) => println!("Goal '{}' proved and registered as generator.", name),
                Err(e) => eprintln!("error registering proof: {}", e),
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse `<name> : <source> -> <target>` from the rest of a `goal` command.
fn parse_goal_spec(spec: &str) -> Result<(String, String, String), String> {
    let colon_pos = spec.find(':')
        .ok_or("goal syntax: <name> : <source> -> <target>")?;
    let name = spec[..colon_pos].trim().to_owned();
    if name.is_empty() {
        return Err("goal name cannot be empty".to_owned());
    }
    let after_colon = spec[colon_pos + 1..].trim();
    let arrow_pos = after_colon.find("->")
        .ok_or("goal syntax: <name> : <source> -> <target>")?;
    let src = after_colon[..arrow_pos].trim().to_owned();
    let tgt = after_colon[arrow_pos + 2..].trim().to_owned();
    if src.is_empty() || tgt.is_empty() {
        return Err("source and target diagram names cannot be empty".to_owned());
    }
    Ok((name, src, tgt))
}

fn show_workspace(ws: &Workspace) {
    let additions = ws.additions();
    if additions.is_empty() {
        println!("  (no additions yet)");
    } else {
        println!("additions in type '{}':", ws.type_name());
        for a in additions {
            println!("  {}", a.name());
        }
    }
    if !ws.proofs.is_empty() {
        println!("proved generators:");
        for p in &ws.proofs {
            println!("  {} : {} -> {}", p.name, p.source_name, p.target_name);
        }
    }
}

fn print_session_help() {
    println!(
        "\
Session commands:
  let <name> = <expr>          Add a let binding (alifib syntax, validated)
  goal <name> : <src> -> <tgt> Start an interactive proof goal        (alias: g)
  show                         Show additions made this session
  export                       Print session additions (paste into source)
  export <path>                Write full modified source to file     (alias: e)
  help / ?                     Show this help
  quit / exit / q              Exit the session"
    );
}
