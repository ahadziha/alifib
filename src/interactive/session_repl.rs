//! Session REPL: load an .ali file, add `let` bindings, prove goals
//! interactively, and export the result back into the type block.
//!
//! # Usage
//!
//! ```text
//! alifib session <file> --type <t> [--emacs]
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

use std::sync::Arc;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::EditMode;

use super::display::Display;
use super::engine::RewriteEngine;
use super::repl::{run_goal_loop, GoalOutcome};
use super::workspace::Workspace;

// ── Public entry point ────────────────────────────────────────────────────────

/// Run the session REPL for `source_file` / `type_name`.
///
/// `emacs_mode` selects Emacs keybindings; the default is vi mode.
#[allow(clippy::result_unit_err)]
pub fn run_session(source_file: &str, type_name: &str, emacs_mode: bool) -> Result<(), ()> {
    let display = Display::new();

    let mut ws = match Workspace::load(source_file, type_name) {
        Ok(w) => w,
        Err(e) => { display.error(&e); return Err(()); }
    };

    display.meta(&format!("Session: {}  ·  type {}", source_file, type_name));
    display.meta("Type 'help' for commands.");

    let mut rl = {
        let mut editor = rustyline::DefaultEditor::new().expect("readline init failed");
        editor.set_edit_mode(if emacs_mode { EditMode::Emacs } else { EditMode::Vi });
        editor
    };

    loop {
        match rl.readline("session> ") {
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break,
            Err(e) => { display.error(&format!("read error: {e}")); break; }
            Ok(line) => {
                let line = line.trim().to_owned();
                if line.is_empty() { continue; }
                rl.add_history_entry(&line).ok();

                if let SessionResult::Quit = dispatch_session_command(&mut ws, &line, &display, &mut rl) {
                    break;
                }
            }
        }
    }

    display.blank();
    Ok(())
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

enum SessionResult { Continue, Quit }

fn dispatch_session_command(
    ws: &mut Workspace,
    line: &str,
    display: &Display,
    rl: &mut rustyline::DefaultEditor,
) -> SessionResult {
    // `let` bindings: the whole line is passed to the workspace validator.
    if line.starts_with("let ") {
        match ws.eval_let(line) {
            Ok(()) => {
                if let Some(a) = ws.additions().last() {
                    display.meta(&format!("Added: {}", a.name()));
                }
            }
            Err(e) => display.error(&e),
        }
        return SessionResult::Continue;
    }

    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").trim();
    let rest = parts.next().map(str::trim).unwrap_or("");

    match cmd {
        "goal" | "g" => {
            match parse_goal_spec(rest) {
                Err(e) => display.error(&e),
                Ok((name, src_name, tgt_name)) => {
                    run_goal(ws, &name, &src_name, &tgt_name, display, rl);
                }
            }
        }
        "show" => show_workspace(ws, display),
        "export" | "e" => {
            if rest.is_empty() {
                display.cell(&ws.export_additions());
            } else {
                let full = ws.export_full_source();
                match std::fs::write(rest, &full) {
                    Ok(()) => display.meta(&format!("Written to '{}'.", rest)),
                    Err(e) => display.error(&format!("cannot write '{}': {}", rest, e)),
                }
            }
        }
        "help" | "?" => print_session_help(display),
        "quit" | "exit" | "q" => return SessionResult::Quit,
        other => display.error(&format!("unknown command '{}' — type 'help' for a list", other)),
    }
    SessionResult::Continue
}

// ── Goal handling ─────────────────────────────────────────────────────────────

fn run_goal(
    ws: &mut Workspace,
    name: &str,
    src_name: &str,
    tgt_name: &str,
    display: &Display,
    rl: &mut rustyline::DefaultEditor,
) {
    let type_complex = match ws.type_complex() {
        Ok(c) => c,
        Err(e) => { display.error(&e); return; }
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
        Err(e) => { display.error(&e); return; }
    };

    display.meta(&format!("Goal '{}': {} -> {}", name, src_name, tgt_name));

    match run_goal_loop(&mut engine, display, rl) {
        GoalOutcome::Abandoned => {
            display.meta("Goal abandoned.");
        }
        GoalOutcome::Done => {
            let proof = match engine.running_diagram() {
                None => {
                    display.error("no proof steps applied — goal not proved");
                    return;
                }
                Some(d) => d.clone(),
            };
            let moves = engine.to_session_file().moves;
            match ws.add_goal_result(name, src_name, tgt_name, proof, moves) {
                Ok(()) => display.meta(&format!("Goal '{}' proved and registered as generator.", name)),
                Err(e) => display.error(&format!("registering proof: {}", e)),
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

fn show_workspace(ws: &Workspace, display: &Display) {
    let additions = ws.additions();
    if additions.is_empty() {
        display.meta("(no additions yet)");
    } else {
        display.meta(&format!("additions in type '{}':", ws.type_name()));
        for a in additions {
            display.meta(&format!("  {}", a.name()));
        }
    }
    if !ws.proofs.is_empty() {
        display.meta("proved generators:");
        for p in &ws.proofs {
            display.meta(&format!("  {} : {} -> {}", p.name, p.source_name, p.target_name));
        }
    }
}

fn print_session_help(display: &Display) {
    display.meta(
        "Session commands:\n\
         \x20 let <name> = <expr>          Add a let binding (alifib syntax, validated)\n\
         \x20 goal <name> : <src> -> <tgt> Start an interactive proof goal        (alias: g)\n\
         \x20 show                         Show additions made this session\n\
         \x20 export                       Print session additions (paste into source)\n\
         \x20 export <path>                Write full modified source to file     (alias: e)\n\
         \x20 help / ?                     Show this help\n\
         \x20 quit / exit / q              Exit the session"
    );
}
