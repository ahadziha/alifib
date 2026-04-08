//! In-process interactive REPL for rewrite sessions.
//!
//! The REPL has two phases:
//!
//! - **Setup phase** — file and type are loaded; the user must set both `source`
//!   and `target` with the `source` / `target` commands.  When both are set, the
//!   engine is created, `>> Ready.` is printed, and the REPL enters the rewriting
//!   phase.
//! - **Rewriting phase** — engine active; `apply`, `undo`, `undo all`, `restart`,
//!   `clear`, `show`, `history`, `proof`, `save`, `load`, etc. are available.
//!
//! All human-readable output flows through a single [`Display`] value.
//! Readline (with vi or emacs mode) is provided by `rustyline`.
//!
//! # Commands
//!
//! ```text
//! source <name>    Set the source diagram (setup phase)
//! target <name>    Set the target diagram (setup phase)
//! apply <n>        Apply rewrite at index <n>            (alias: a)
//! undo             Undo the last step                    (alias: u)
//! undo <n>         Undo back to step <n>
//! undo all         Reset to source (= restart)
//! restart          Reset to source diagram
//! clear            Destroy engine, return to setup phase
//! show             Redisplay current state
//! rules            List all rewrite rules in the type    (alias: r)
//! info <name>      Show source → target of a generator  (alias: i)
//! history          Show the move history                 (alias: h)
//! proof            Show the running proof diagram        (alias: p)
//! save <path>      Save session to a JSON file
//! load <path>      Load and replay a session file        (alias: l)
//! help / ?         Show command list
//! quit / exit / q  Exit the REPL
//! ```

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::EditMode;

use crate::core::diagram::{CellData, Sign};
use crate::core::complex::Complex;
use crate::interpreter::GlobalStore;
use crate::output::render_diagram;
use super::display::Display;
use super::engine::{RewriteEngine, load_type_context};
use super::render::{print_history, print_state};
use super::session::SessionFile;

// ── Public types ──────────────────────────────────────────────────────────────

/// Outcome of a goal sub-loop.
pub enum GoalOutcome {
    /// The user accepted the proof (typed `done` or `accept`).
    Done,
    /// The user abandoned the goal (typed `abandon`, `quit`, or EOF).
    Abandoned,
}

/// Result of dispatching a single rewrite command.
#[derive(PartialEq, Eq)]
pub enum DispatchResult { Continue, Quit }

// ── Public entry points ───────────────────────────────────────────────────────

/// Run the inner goal loop on an already-initialised engine.
///
/// Accepts all standard rewrite commands plus `done`/`accept` to finish the
/// proof. Returns [`GoalOutcome::Done`] when the user accepts, or
/// [`GoalOutcome::Abandoned`] on `abandon` / `quit` / EOF.
pub fn run_goal_loop(
    engine: &mut RewriteEngine,
    display: &Display,
    rl: &mut rustyline::DefaultEditor,
) -> GoalOutcome {
    show_state(engine, display);
    loop {
        match rl.readline("goal> ") {
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => {
                return GoalOutcome::Abandoned;
            }
            Err(e) => {
                display.error(&format!("read error: {e}"));
                return GoalOutcome::Abandoned;
            }
            Ok(line) => {
                let line = line.trim().to_owned();
                if line.is_empty() { continue; }
                rl.add_history_entry(&line).ok();

                match line.as_str() {
                    "done" | "accept" | "d" | "a" => return GoalOutcome::Done,
                    "abandon" => return GoalOutcome::Abandoned,
                    _ => {}
                }
                if dispatch_rewrite_command(engine, &line, display) == DispatchResult::Quit {
                    return GoalOutcome::Abandoned;
                }
            }
        }
    }
}

/// Run the interactive REPL starting from a loaded file and type.
///
/// `source_diagram` and `target_diagram` may be given as CLI arguments; if
/// omitted the user sets them interactively via `source` / `target` commands.
/// `emacs_mode` selects Emacs keybindings; the default is vi mode.
pub fn run_repl(
    source_file: &str,
    type_name: &str,
    source_diagram: Option<&str>,
    target_diagram: Option<&str>,
    emacs_mode: bool,
) -> Result<(), ()> {
    let display = Display::new();

    let (store, type_complex, _canonical) =
        match load_type_context(source_file, type_name) {
            Ok(r) => r,
            Err(e) => { display.error(&e); return Err(()); }
        };

    display.meta(&format!("Loaded {}", source_file));
    display.meta(&format!("Type: {}", type_name));

    let mut rl = make_editor(emacs_mode);

    let mut pending_source: Option<String> = source_diagram.map(str::to_owned);
    let mut pending_target: Option<String> = target_diagram.map(str::to_owned);
    let mut engine: Option<RewriteEngine> = None;

    // If both were supplied on the CLI, create the engine immediately.
    if let (Some(src), Some(tgt)) = (&pending_source, &pending_target) {
        match try_create_engine(&store, &type_complex, src, tgt, source_file, type_name, &display) {
            Some(e) => {
                engine = Some(e);
                display.meta("Ready.");
                show_state(engine.as_ref().unwrap(), &display);
            }
            None => { return Err(()); }
        }
    }

    loop {
        match rl.readline("> ") {
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break,
            Err(e) => { display.error(&format!("read error: {e}")); break; }
            Ok(line) => {
                let line = line.trim().to_owned();
                if line.is_empty() { continue; }
                rl.add_history_entry(&line).ok();

                match parse_command(&line) {
                    // ── Always-available commands ─────────────────────
                    Cmd::Source(name) => {
                        pending_source = Some(name);
                        maybe_start_engine(
                            &pending_source, &pending_target,
                            &store, &type_complex, source_file, type_name,
                            &display, &mut engine,
                        );
                    }
                    Cmd::Target(name) => {
                        pending_target = Some(name);
                        maybe_start_engine(
                            &pending_source, &pending_target,
                            &store, &type_complex, source_file, type_name,
                            &display, &mut engine,
                        );
                    }
                    Cmd::Clear => {
                        engine = None;
                        pending_source = None;
                        pending_target = None;
                        display.meta("Cleared.");
                    }
                    Cmd::Rules => {
                        let n = engine.as_ref().map(|e| e.current_diagram().top_dim()).unwrap_or(0);
                        dispatch_rules(&type_complex, &store, n, &display);
                    }
                    Cmd::Info(name) => dispatch_info(&type_complex, &store, &name, &display),
                    Cmd::Help => print_help(&display),
                    Cmd::Quit => break,

                    // ── Rewriting-phase commands (require engine) ─────
                    cmd => match engine.as_mut() {
                        None => display.error("set source and target first"),
                        Some(e) => dispatch_engine_cmd(e, cmd, &display),
                    },
                }
            }
        }
    }

    display.blank();
    Ok(())
}

/// Dispatch a single rewrite command to an existing engine.
///
/// Used by [`run_goal_loop`].  Returns [`DispatchResult::Quit`] if the command
/// was `quit`/`exit`/`q`, otherwise [`DispatchResult::Continue`].
pub fn dispatch_rewrite_command(
    engine: &mut RewriteEngine,
    line: &str,
    display: &Display,
) -> DispatchResult {
    match parse_command(line) {
        Cmd::Quit => return DispatchResult::Quit,
        Cmd::Clear | Cmd::Source(_) | Cmd::Target(_) => {
            display.error("command not available here");
        }
        cmd => dispatch_engine_cmd(engine, cmd, display),
    }
    DispatchResult::Continue
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Create a rustyline editor in vi or emacs mode.
fn make_editor(emacs_mode: bool) -> rustyline::DefaultEditor {
    let mut rl = rustyline::DefaultEditor::new().expect("readline init failed");
    rl.set_edit_mode(if emacs_mode { EditMode::Emacs } else { EditMode::Vi });
    rl
}

/// Attempt to create a `RewriteEngine`.  Prints an error and returns `None` on
/// failure.
fn try_create_engine(
    store: &std::sync::Arc<GlobalStore>,
    type_complex: &std::sync::Arc<Complex>,
    src: &str,
    tgt: &str,
    source_file: &str,
    type_name: &str,
    display: &Display,
) -> Option<RewriteEngine> {
    match RewriteEngine::from_store(
        std::sync::Arc::clone(store),
        std::sync::Arc::clone(type_complex),
        src,
        Some(tgt),
        source_file.to_owned(),
        type_name.to_owned(),
    ) {
        Ok(e) => Some(e),
        Err(e) => { display.error(&e); None }
    }
}

/// Create the engine when both source and target are set, updating `engine` in place.
#[allow(clippy::too_many_arguments)]
fn maybe_start_engine(
    pending_source: &Option<String>,
    pending_target: &Option<String>,
    store: &std::sync::Arc<GlobalStore>,
    type_complex: &std::sync::Arc<Complex>,
    source_file: &str,
    type_name: &str,
    display: &Display,
    engine: &mut Option<RewriteEngine>,
) {
    if let (Some(src), Some(tgt)) = (pending_source, pending_target) {
        if let Some(e) = try_create_engine(store, type_complex, src, tgt, source_file, type_name, display) {
            *engine = Some(e);
            display.meta("Ready.");
            show_state(engine.as_ref().unwrap(), display);
        }
    }
}

/// Call `print_state` with fields drawn from `engine`, computing proof strings
/// as local temporaries so that references remain valid.
fn show_state(engine: &RewriteEngine, display: &Display) {
    let src_label = render_diagram(engine.source_diagram(), engine.type_complex());
    let tgt_label = engine.target_diagram()
        .map(|t| render_diagram(t, engine.type_complex()));
    let proof_label = engine.proof_label();

    let proof = match (&tgt_label, &proof_label) {
        (Some(tl), Some(pl)) => Some((src_label.as_str(), tl.as_str(), pl.as_str())),
        _ => None,
    };

    print_state(
        display,
        engine.current_diagram(),
        engine.target_diagram(),
        engine.available_rewrites(),
        engine.type_complex(),
        proof,
    );
}

/// Dispatch all commands that require an active engine.
fn dispatch_engine_cmd(engine: &mut RewriteEngine, cmd: Cmd, display: &Display) {
    match cmd {
        Cmd::Apply(n) => {
            match engine.step(n) {
                Ok(rule) => {
                    display.meta(&format!("Applied {}.", rule));
                    show_state(engine, display);
                }
                Err(e) => display.error(&e),
            }
        }
        Cmd::Undo(None) => {
            match engine.undo() {
                Ok(()) => show_state(engine, display),
                Err(e) => display.error(&e),
            }
        }
        Cmd::Undo(Some(target)) => {
            match engine.undo_to(target) {
                Ok(()) => show_state(engine, display),
                Err(e) => display.error(&e),
            }
        }
        Cmd::UndoAll | Cmd::Restart => {
            match engine.undo_all() {
                Ok(()) => {
                    display.meta("Reset to source.");
                    show_state(engine, display);
                }
                Err(e) => display.error(&e),
            }
        }
        Cmd::Show => show_state(engine, display),
        Cmd::Rules => {
            let n = engine.current_diagram().top_dim();
            dispatch_rules(engine.type_complex(), engine.store(), n, display);
        }
        Cmd::Info(name) => dispatch_info(engine.type_complex(), engine.store(), &name, display),
        Cmd::History => {
            let sf = engine.to_session_file();
            let entries: Vec<(usize, &str)> = sf.moves.iter()
                .map(|m| (m.choice, m.rule_name.as_str()))
                .collect();
            print_history(display, engine.source_diagram(), &entries, engine.type_complex());
        }
        Cmd::Proof => {
            match engine.running_diagram() {
                None => display.meta("(no proof built yet)"),
                Some(d) => {
                    let n = engine.source_diagram().top_dim();
                    match (
                        crate::core::diagram::Diagram::boundary(Sign::Source, n, d),
                        crate::core::diagram::Diagram::boundary(Sign::Target, n, d),
                    ) {
                        (Ok(src), Ok(tgt)) => display.meta(&format!(
                            "{} : {} -> {}",
                            render_diagram(d, engine.type_complex()),
                            render_diagram(&src, engine.type_complex()),
                            render_diagram(&tgt, engine.type_complex()),
                        )),
                        _ => display.error("boundary extraction failed"),
                    }
                }
            }
        }
        Cmd::Save(path) => {
            match engine.to_session_file().write(&path) {
                Ok(()) => display.meta(&format!("Saved session to '{}'.", path)),
                Err(e) => display.error(&e),
            }
        }
        Cmd::Load(path) => {
            match SessionFile::read(&path) {
                Err(e) => display.error(&e),
                Ok(sf) => match RewriteEngine::from_session(sf) {
                    Err(e) => display.error(&format!("loading session: {}", e)),
                    Ok(new_engine) => {
                        *engine = new_engine;
                        display.meta(&format!("Loaded session from '{}' .", path));
                        show_state(engine, display);
                    }
                }
            }
        }
        Cmd::Help => print_help(display),
        Cmd::Quit => {}  // handled by caller
        // These are handled before dispatch_engine_cmd is reached
        Cmd::Clear | Cmd::Source(_) | Cmd::Target(_) => unreachable!(),
        Cmd::Unknown(s) => display.error(&format!("unknown command '{}' — type 'help' for a list", s)),
    }
}

/// Display the available rewrite rules at dimension `n + 1`.
fn dispatch_rules(complex: &Complex, store: &GlobalStore, n: usize, display: &Display) {
    display.meta(&format!("rewrite rules (dim {}):", n + 1));
    let mut any = false;
    for (name, tag, dim) in complex.generators_iter() {
        if dim != n + 1 { continue; }
        any = true;
        match store.cell_data_for_tag(complex, tag) {
            Some(CellData::Boundary { boundary_in, boundary_out }) => {
                display.meta(&format!(
                    "  {} : {}  ->  {}",
                    name,
                    render_diagram(&boundary_in, complex),
                    render_diagram(&boundary_out, complex),
                ));
            }
            _ => display.meta(&format!("  {} (no boundaries)", name)),
        }
    }
    if !any {
        display.meta(&format!("  (no rewrite rules at dim {})", n + 1));
    }
}

/// Display the source → target of a named generator.
fn dispatch_info(complex: &Complex, store: &GlobalStore, name: &str, display: &Display) {
    match complex.find_generator(name) {
        Some((tag, dim)) => {
            match store.cell_data_for_tag(complex, tag) {
                Some(CellData::Boundary { boundary_in, boundary_out }) => {
                    display.meta(&format!(
                        "{} (dim {}): {}  ->  {}",
                        name, dim,
                        render_diagram(&boundary_in, complex),
                        render_diagram(&boundary_out, complex),
                    ));
                }
                Some(CellData::Zero) => display.meta(&format!("{} (dim 0): 0-cell", name)),
                None => display.error(&format!("no cell data for '{}'", name)),
            }
        }
        None => display.error(&format!("generator '{}' not found", name)),
    }
}

fn print_help(display: &Display) {
    display.meta(
        "Commands:\n\
         \x20 source <name>    Set the source diagram (setup phase)\n\
         \x20 target <name>    Set the target diagram (setup phase)\n\
         \x20 apply <n>        Apply rewrite at index <n>            (alias: a)\n\
         \x20 undo             Undo the last step                    (alias: u)\n\
         \x20 undo <n>         Undo back to step <n>\n\
         \x20 undo all         Reset to source (= restart)\n\
         \x20 restart          Reset to source diagram\n\
         \x20 clear            Destroy engine, return to setup phase\n\
         \x20 show             Redisplay current state\n\
         \x20 rules            List all rewrite rules in the type    (alias: r)\n\
         \x20 info <name>      Show source -> target of a generator  (alias: i)\n\
         \x20 history          Show the move history                 (alias: h)\n\
         \x20 proof            Show the running proof diagram        (alias: p)\n\
         \x20 save <path>      Save session to a JSON file\n\
         \x20 load <path>      Load and replay a session file        (alias: l)\n\
         \x20 help / ?         Show this help\n\
         \x20 quit / exit / q  Exit the REPL"
    );
}

// ── Command parsing ───────────────────────────────────────────────────────────

enum Cmd {
    Source(String),
    Target(String),
    Apply(usize),
    Undo(Option<usize>),
    UndoAll,
    Restart,
    Clear,
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

    match word {
        "source" => {
            if rest.is_empty() { Cmd::Unknown("source <name>".to_owned()) }
            else { Cmd::Source(rest.to_owned()) }
        }
        "target" => {
            if rest.is_empty() { Cmd::Unknown("target <name>".to_owned()) }
            else { Cmd::Target(rest.to_owned()) }
        }
        "apply" | "a" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Apply(n),
                Err(_) => Cmd::Unknown(format!("apply {}", rest)),
            }
        }
        "undo" | "u" => {
            if rest.is_empty() {
                Cmd::Undo(None)
            } else if rest == "all" {
                Cmd::UndoAll
            } else {
                match rest.parse::<usize>() {
                    Ok(n) => Cmd::Undo(Some(n)),
                    Err(_) => Cmd::Unknown(format!("undo {}", rest)),
                }
            }
        }
        "restart" => Cmd::Restart,
        "clear"   => Cmd::Clear,
        "show"    => Cmd::Show,
        "rules" | "r" => Cmd::Rules,
        "info" | "i" => {
            if rest.is_empty() { Cmd::Unknown("info <name>".to_owned()) }
            else { Cmd::Info(rest.to_owned()) }
        }
        "history" | "h" => Cmd::History,
        "proof" | "p"   => Cmd::Proof,
        "save" => {
            if rest.is_empty() { Cmd::Unknown("save <path>".to_owned()) }
            else { Cmd::Save(rest.to_owned()) }
        }
        "load" | "l" => {
            if rest.is_empty() { Cmd::Unknown("load <path>".to_owned()) }
            else { Cmd::Load(rest.to_owned()) }
        }
        "help" | "?" => Cmd::Help,
        "quit" | "exit" | "q" => Cmd::Quit,
        other => Cmd::Unknown(other.to_owned()),
    }
}
