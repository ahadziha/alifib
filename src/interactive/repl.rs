//! In-process interactive REPL for rewrite sessions.
//!
//! Wraps a [`RewriteEngine`] with a readline-style prompt loop. All state
//! lives in memory — no re-interpretation on each command.
//!
//! # Usage
//!
//! ```text
//! alifib repl <file> --type <t> --source <s> [--target <t>]
//! ```
//!
//! # Commands
//!
//! ```text
//! <number>        Apply rewrite at index (shorthand for `step`)
//! step <n>        Apply rewrite at index <n>
//! undo            Undo the last step
//! undo <n>        Undo to step <n>
//! show            Redisplay current state and available rewrites
//! rules           List all rewrite rules (n+1 generators) in the type
//! info <name>     Show source → target of a named generator
//! history         Show the move history
//! proof           Show the running proof diagram (if any)
//! save <path>     Save session to a JSON file
//! load <path>     Load and replay a session file (replaces current session)
//! help            Show command list
//! quit / exit     Exit the REPL
//! ```

use std::io::{BufRead, Write};

use crate::core::diagram::{CellData, Sign};
use crate::output::render_diagram;
use super::engine::RewriteEngine;
use super::render::{print_history, print_state};
use super::session::SessionFile;

/// Run the interactive REPL for a new session.
pub fn run_repl(
    source_file: &str,
    type_name: &str,
    source_diagram: &str,
    target_diagram: Option<&str>,
) -> Result<(), ()> {
    let mut engine = match RewriteEngine::init(source_file, type_name, source_diagram, target_diagram) {
        Ok(e) => e,
        Err(e) => { eprintln!("error: {}", e); return Err(()); }
    };

    println!(
        "Loaded {}  ·  type {}  ·  source: {}",
        source_file,
        type_name,
        render_diagram(engine.source_diagram(), engine.type_complex()),
    );
    if let Some(t) = engine.target_diagram() {
        println!("target: {}", render_diagram(t, engine.type_complex()));
    }

    print_state(
        engine.step_count(),
        engine.current_diagram(),
        engine.target_diagram(),
        engine.available_rewrites(),
        engine.type_complex(),
    );

    let stdin = std::io::stdin();
    loop {
        print!("rewrite[{}]> ", engine.step_count());
        std::io::stdout().flush().ok();

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => { eprintln!("read error: {}", e); break; }
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match parse_command(line) {
            Cmd::Step(n) => {
                match engine.step(n) {
                    Ok(rule) => {
                        println!("Applied {} (choice {}).", rule, n);
                        print_state(
                            engine.step_count(),
                            engine.current_diagram(),
                            engine.target_diagram(),
                            engine.available_rewrites(),
                            engine.type_complex(),
                        );
                    }
                    Err(e) => eprintln!("error: {}", e),
                }
            }
            Cmd::Undo(None) => {
                match engine.undo() {
                    Ok(()) => {
                        println!("Undone to step {}.", engine.step_count());
                        print_state(
                            engine.step_count(),
                            engine.current_diagram(),
                            engine.target_diagram(),
                            engine.available_rewrites(),
                            engine.type_complex(),
                        );
                    }
                    Err(e) => eprintln!("error: {}", e),
                }
            }
            Cmd::Undo(Some(target)) => {
                match engine.undo_to(target) {
                    Ok(()) => {
                        println!("Undone to step {}.", engine.step_count());
                        print_state(
                            engine.step_count(),
                            engine.current_diagram(),
                            engine.target_diagram(),
                            engine.available_rewrites(),
                            engine.type_complex(),
                        );
                    }
                    Err(e) => eprintln!("error: {}", e),
                }
            }
            Cmd::Show => {
                print_state(
                    engine.step_count(),
                    engine.current_diagram(),
                    engine.target_diagram(),
                    engine.available_rewrites(),
                    engine.type_complex(),
                );
            }
            Cmd::Rules => {
                let complex = engine.type_complex();
                let store = engine.store();
                let n = engine.current_diagram().top_dim();
                println!("rewrite rules (dim {}):", n + 1);
                let mut any = false;
                for (name, tag, dim) in complex.generators_iter() {
                    if dim != n + 1 {
                        continue;
                    }
                    any = true;
                    match store.cell_data_for_tag(complex, tag) {
                        Some(CellData::Boundary { boundary_in, boundary_out }) => {
                            println!(
                                "  {} : {}  ->  {}",
                                name,
                                render_diagram(&boundary_in, complex),
                                render_diagram(&boundary_out, complex),
                            );
                        }
                        _ => println!("  {} (no boundaries)", name),
                    }
                }
                if !any {
                    println!("  (no rewrite rules at dim {})", n + 1);
                }
            }
            Cmd::Info(name) => {
                let complex = engine.type_complex();
                let store = engine.store();
                match complex.find_generator(&name) {
                    Some((tag, dim)) => {
                        match store.cell_data_for_tag(complex, tag) {
                            Some(CellData::Boundary { boundary_in, boundary_out }) => {
                                println!(
                                    "{} (dim {}): {}  ->  {}",
                                    name, dim,
                                    render_diagram(&boundary_in, complex),
                                    render_diagram(&boundary_out, complex),
                                );
                            }
                            Some(CellData::Zero) => {
                                println!("{} (dim 0): 0-cell", name);
                            }
                            None => eprintln!("error: no cell data for '{}'", name),
                        }
                    }
                    None => eprintln!("error: generator '{}' not found", name),
                }
            }
            Cmd::History => {
                let sf = engine.to_session_file();
                let entries: Vec<(usize, &str)> = sf.moves.iter()
                    .map(|m| (m.choice, m.rule_name.as_str()))
                    .collect();
                print_history(engine.source_diagram(), &entries, engine.type_complex());
            }
            Cmd::Proof => {
                match engine.running_diagram() {
                    None => println!("  (no proof built yet)"),
                    Some(d) => {
                        let n = engine.source_diagram().top_dim();
                        println!("proof (dim {}):", d.top_dim());
                        match (
                            crate::core::diagram::Diagram::boundary(Sign::Source, n, d),
                            crate::core::diagram::Diagram::boundary(Sign::Target, n, d),
                        ) {
                            (Ok(src), Ok(tgt)) => println!(
                                "  {} steps: {}  =>  {}",
                                engine.step_count(),
                                render_diagram(&src, engine.type_complex()),
                                render_diagram(&tgt, engine.type_complex()),
                            ),
                            _ => println!("  (boundary extraction failed)"),
                        }
                    }
                }
            }
            Cmd::Save(path) => {
                match engine.to_session_file().write(&path) {
                    Ok(()) => println!("Saved session to '{}'.", path),
                    Err(e) => eprintln!("error: {}", e),
                }
            }
            Cmd::Load(path) => {
                match SessionFile::read(&path) {
                    Err(e) => eprintln!("error: {}", e),
                    Ok(sf) => match RewriteEngine::from_session(sf) {
                        Err(e) => eprintln!("error loading session: {}", e),
                        Ok(new_engine) => {
                            engine = new_engine;
                            println!("Loaded session from '{}'.", path);
                            print_state(
                                engine.step_count(),
                                engine.current_diagram(),
                                engine.target_diagram(),
                                engine.available_rewrites(),
                                engine.type_complex(),
                            );
                        }
                    }
                }
            }
            Cmd::Help => print_help(),
            Cmd::Quit => break,
            Cmd::Unknown(s) => eprintln!("unknown command '{}' — type 'help' for a list", s),
        }
    }

    println!();
    Ok(())
}

// ── Command parsing ───────────────────────────────────────────────────────────

enum Cmd {
    Step(usize),
    Undo(Option<usize>),
    Show,
    Rules,
    Info(String),
    History,
    Proof,
    Save(String),
    Load(String),
    Help,
    Quit,
    Unknown(String),
}

fn parse_command(line: &str) -> Cmd {
    let mut parts = line.splitn(2, char::is_whitespace);
    let word = parts.next().unwrap_or("").trim();
    let rest = parts.next().map(str::trim).unwrap_or("");

    // Bare number → step
    if let Ok(n) = word.parse::<usize>() {
        if rest.is_empty() {
            return Cmd::Step(n);
        }
    }

    match word {
        "step" | "s" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Step(n),
                Err(_) => Cmd::Unknown(format!("step {}", rest)),
            }
        }
        "undo" | "u" => {
            if rest.is_empty() {
                Cmd::Undo(None)
            } else {
                match rest.parse::<usize>() {
                    Ok(n) => Cmd::Undo(Some(n)),
                    Err(_) => Cmd::Unknown(format!("undo {}", rest)),
                }
            }
        }
        "show" => Cmd::Show,
        "rules" | "r" => Cmd::Rules,
        "info" | "i" => {
            if rest.is_empty() {
                Cmd::Unknown("info <name>".to_owned())
            } else {
                Cmd::Info(rest.to_owned())
            }
        }
        "history" | "h" => Cmd::History,
        "proof" | "p" => Cmd::Proof,
        "save" => {
            if rest.is_empty() {
                Cmd::Unknown("save <path>".to_owned())
            } else {
                Cmd::Save(rest.to_owned())
            }
        }
        "load" | "l" => {
            if rest.is_empty() {
                Cmd::Unknown("load <path>".to_owned())
            } else {
                Cmd::Load(rest.to_owned())
            }
        }
        "help" | "?" => Cmd::Help,
        "quit" | "exit" | "q" => Cmd::Quit,
        other => Cmd::Unknown(other.to_owned()),
    }
}

fn print_help() {
    println!(
        "\
Commands:
  <n>              Apply rewrite at index <n>
  step <n>         Apply rewrite at index <n>          (alias: s)
  undo             Undo the last step                  (alias: u)
  undo <n>         Undo back to step <n>
  show             Redisplay current state
  rules            List all rewrite rules in the type  (alias: r)
  info <name>      Show source → target of a generator (alias: i)
  history          Show the move history               (alias: h)
  proof            Show the running proof diagram      (alias: p)
  save <path>      Save session to a JSON file
  load <path>      Load and replay a session file      (alias: l)
  help / ?         Show this help
  quit / exit / q  Exit the REPL"
    );
}
