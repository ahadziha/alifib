//! In-process interactive REPL for rewrite sessions.
//!
//! The REPL has two phases:
//!
//! - **Setup phase** — file is loaded.  The user must select a type (`@ Idem`),
//!   then set `source` and `target` diagram names.  When all three are set, the
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
//! @ <type>         Select a type from the loaded file (reuses language parser)
//! types            List all types declared in the file
//! source <name>    Set the source diagram
//! target <name>    Set the target diagram
//! apply <n>        Apply rewrite at index <n>            (alias: a)
//! undo             Undo the last step                    (alias: u)
//! undo <n>         Undo back to step <n>
//! undo all         Reset to source (= restart)
//! restart          Reset to source diagram
//! clear            Destroy engine and type, return to setup phase
//! show             Redisplay current state
//! rules            List generators in the selected type  (alias: r)
//! info <name>      Show source → target of a generator  (alias: i)
//! history          Show the move history                 (alias: h)
//! proof            Show the running proof diagram        (alias: p)
//! save <path>      Save session to a JSON file
//! load <path>      Load and replay a session file        (alias: l)
//! help / ?         Show command list
//! quit / exit / q  Exit the REPL
//! ```

use std::sync::Arc;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::EditMode;

use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Sign};
use crate::interpreter::GlobalStore;
use crate::language;
use crate::language::ast::Complex as AstComplex;
use crate::output::render_diagram;
use super::display::Display;
use super::engine::{RewriteEngine, load_file_context, resolve_type};
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

/// Run the interactive REPL starting from a loaded file.
///
/// `type_name`, `source_diagram`, and `target_diagram` may be given as CLI
/// arguments; any that are omitted must be set interactively.
/// `emacs_mode` selects Emacs keybindings; the default is vi mode.
pub fn run_repl(
    source_file: &str,
    type_name: Option<&str>,
    source_diagram: Option<&str>,
    target_diagram: Option<&str>,
    emacs_mode: bool,
) -> Result<(), ()> {
    let display = Display::new();

    let (store, canonical_path, file_output) = match load_file_context(source_file) {
        Ok(r) => r,
        Err(e) => { display.error(&e); return Err(()); }
    };

    display.meta(&format!("Loaded {}", source_file));

    let mut rl = make_editor(emacs_mode);

    // Mutable setup-phase state
    let mut type_complex: Option<Arc<Complex>> = None;
    let mut type_name_str: Option<String> = None;
    let mut pending_source: Option<String> = source_diagram.map(str::to_owned);
    let mut pending_target: Option<String> = target_diagram.map(str::to_owned);
    let mut engine: Option<RewriteEngine> = None;

    // Pre-select type from CLI arg.
    if let Some(tn) = type_name {
        match resolve_type(&store, &canonical_path, tn) {
            Ok(tc) => {
                type_complex = Some(tc);
                type_name_str = Some(tn.to_owned());
                display.meta(&format!("Type: {}", tn));
            }
            Err(e) => { display.error(&e); return Err(()); }
        }
    }

    // If all three were given on the CLI, start immediately.
    if type_complex.is_some() {
        maybe_start_engine(
            &type_complex, &type_name_str, &pending_source, &pending_target,
            &store, source_file, &display, &mut engine,
        );
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
                    Cmd::AtExpr(expr) => {
                        handle_at_command(
                            &expr, &store, &canonical_path, source_file,
                            &display,
                            &mut type_complex, &mut type_name_str,
                            &mut pending_source, &mut pending_target,
                            &mut engine,
                        );
                    }
                    Cmd::Types => dispatch_types(&store, &canonical_path, &display),
                    Cmd::Print(None) => {
                        let trimmed = file_output.trim_end();
                        if !trimmed.is_empty() { display.cell(trimmed); }
                    }
                    Cmd::Print(Some(name)) => {
                        match (&engine, &type_complex) {
                            (Some(e), _) => dispatch_print_cell(e.type_complex(), &name, &display),
                            (None, Some(tc)) => dispatch_print_cell(tc, &name, &display),
                            (None, None) => display.error("set type first (@ <TypeName>)"),
                        }
                    }
                    Cmd::Clear => {
                        engine = None;
                        type_complex = None;
                        type_name_str = None;
                        pending_source = None;
                        pending_target = None;
                        display.meta("Cleared.");
                    }
                    Cmd::Help => print_help(&display),
                    Cmd::Quit => break,

                    // ── Commands that need type to be set ─────────────
                    Cmd::Source(name) => {
                        pending_source = Some(name);
                        maybe_start_engine(
                            &type_complex, &type_name_str, &pending_source, &pending_target,
                            &store, source_file, &display, &mut engine,
                        );
                    }
                    Cmd::Target(name) => {
                        pending_target = Some(name);
                        maybe_start_engine(
                            &type_complex, &type_name_str, &pending_source, &pending_target,
                            &store, source_file, &display, &mut engine,
                        );
                    }
                    Cmd::Rules => {
                        match (&engine, &type_complex) {
                            (Some(e), _) => dispatch_rules(e.type_complex(), e.store(), Some(e.current_diagram().top_dim() + 1), &display),
                            (None, Some(tc)) => dispatch_rules(tc, &store, None, &display),
                            (None, None) => display.error("set type first (@ <TypeName>)"),
                        }
                    }
                    Cmd::Info(name) => {
                        match (&engine, &type_complex) {
                            (Some(e), _) => dispatch_info(e.type_complex(), e.store(), &name, &display),
                            (None, Some(tc)) => dispatch_info(tc, &store, &name, &display),
                            (None, None) => display.error("set type first (@ <TypeName>)"),
                        }
                    }

                    // ── Rewriting-phase commands (require engine) ─────
                    cmd => match engine.as_mut() {
                        None => display.error("set type, source, and target first"),
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
        Cmd::Clear | Cmd::Source(_) | Cmd::Target(_) | Cmd::AtExpr(_) | Cmd::Types | Cmd::Print(None) => {
            display.error("command not available here");
        }
        Cmd::Print(Some(name)) => dispatch_print_cell(engine.type_complex(), &name, display),
        cmd => dispatch_engine_cmd(engine, cmd, display),
    }
    DispatchResult::Continue
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn make_editor(emacs_mode: bool) -> rustyline::DefaultEditor {
    let mut rl = rustyline::DefaultEditor::new().expect("readline init failed");
    rl.set_edit_mode(if emacs_mode { EditMode::Emacs } else { EditMode::Vi });
    rl
}

/// Process a `@ <expr>` command: parse the complex expression, resolve to a
/// type, update state, and attempt to start the engine.
#[allow(clippy::too_many_arguments)]
fn handle_at_command(
    expr: &str,
    store: &Arc<GlobalStore>,
    canonical_path: &str,
    source_file: &str,
    display: &Display,
    type_complex: &mut Option<Arc<Complex>>,
    type_name_str: &mut Option<String>,
    pending_source: &mut Option<String>,
    pending_target: &mut Option<String>,
    engine: &mut Option<RewriteEngine>,
) {
    match language::parse_complex(expr) {
        Err(e) => {
            display.error(&format!("parse error: {}", e));
        }
        Ok(AstComplex::Block { .. }) => {
            display.error("block syntax not supported at the REPL prompt — use a bare name, e.g. @ Idem");
        }
        Ok(AstComplex::Address(addr)) => {
            let name = addr.iter().map(|s| s.inner.as_str()).collect::<Vec<_>>().join(".");
            match resolve_type(store, canonical_path, &name) {
                Err(e) => display.error(&e),
                Ok(tc) => {
                    *type_complex = Some(tc);
                    *type_name_str = Some(name.clone());
                    // Reset source/target — they may not exist in the new type.
                    *pending_source = None;
                    *pending_target = None;
                    *engine = None;
                    display.meta(&format!("Type: {}", name));
                    // Immediately try to start if source/target happen to be set
                    maybe_start_engine(
                        type_complex, type_name_str, pending_source, pending_target,
                        store, source_file, display, engine,
                    );
                }
            }
        }
    }
}

/// List all types declared in the file.
fn dispatch_types(store: &GlobalStore, canonical_path: &str, display: &Display) {
    let Some(mc) = store.find_module(canonical_path) else {
        display.error("module not found");
        return;
    };
    let mut any = false;
    for (name, tag, _dim) in mc.generators_iter() {
        if let crate::aux::Tag::Global(gid) = tag {
            if store.find_type(*gid).is_some() {
                display.meta(&format!("  {}", name));
                any = true;
            }
        }
    }
    if !any {
        display.meta("  (no types found)");
    }
}

/// Create the engine when type, source, and target are all set.
#[allow(clippy::too_many_arguments)]
fn maybe_start_engine(
    type_complex: &Option<Arc<Complex>>,
    type_name_str: &Option<String>,
    pending_source: &Option<String>,
    pending_target: &Option<String>,
    store: &Arc<GlobalStore>,
    source_file: &str,
    display: &Display,
    engine: &mut Option<RewriteEngine>,
) {
    if let (Some(tc), Some(tn), Some(src), Some(tgt)) =
        (type_complex, type_name_str, pending_source, pending_target)
    {
        match RewriteEngine::from_store(
            Arc::clone(store),
            Arc::clone(tc),
            src,
            Some(tgt),
            source_file.to_owned(),
            tn.clone(),
        ) {
            Ok(e) => {
                *engine = Some(e);
                display.meta("Ready.");
                show_state(engine.as_ref().unwrap(), display);
            }
            Err(e) => display.error(&e),
        }
    }
}

/// Call `print_state` with fields drawn from `engine`.
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

/// Display generators in `complex`, optionally filtered to those at `filter_dim`.
///
/// Without a filter (setup phase, no source diagram yet), all generators are
/// shown with their dimensions.  With a filter (rewriting phase), only the
/// relevant rewrite rules are shown.
fn dispatch_rules(complex: &Complex, store: &GlobalStore, filter_dim: Option<usize>, display: &Display) {
    if let Some(d) = filter_dim {
        display.meta(&format!("rewrite rules (dim {}):", d));
    } else {
        display.meta("generators:");
    }
    let mut any = false;
    for (name, tag, dim) in complex.generators_iter() {
        if let Some(d) = filter_dim {
            if dim != d { continue; }
        }
        any = true;
        match store.cell_data_for_tag(complex, tag) {
            Some(CellData::Boundary { boundary_in, boundary_out }) => {
                if filter_dim.is_some() {
                    display.meta(&format!(
                        "  {} : {}  ->  {}",
                        name,
                        render_diagram(&boundary_in, complex),
                        render_diagram(&boundary_out, complex),
                    ));
                } else {
                    display.meta(&format!(
                        "  {} (dim {}): {}  ->  {}",
                        name, dim,
                        render_diagram(&boundary_in, complex),
                        render_diagram(&boundary_out, complex),
                    ));
                }
            }
            Some(CellData::Zero) => display.meta(&format!("  {} (dim 0): 0-cell", name)),
            _ => display.meta(&format!("  {} (no boundaries)", name)),
        }
    }
    if !any {
        if let Some(d) = filter_dim {
            display.meta(&format!("  (no rewrite rules at dim {})", d));
        } else {
            display.meta("  (no generators)");
        }
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

/// Print a named cell from the type complex.
///
/// - **Generator** (`name : src -> tgt`): shows dimension and boundary.
/// - **Let binding** (`let name = expr`): shows dimension, boundary, and the
///   full definition diagram.
/// - **Neither**: reports an error.
fn dispatch_print_cell(complex: &Complex, name: &str, display: &Display) {
    // Generators are in the generators table and have cell data.
    if let Some((_, dim)) = complex.find_generator(name) {
        if dim == 0 {
            display.meta(&format!("{} (dim 0): 0-cell", name));
        } else {
            // Reconstruct boundary from the diagram stored for this name.
            if let Some(diag) = complex.find_diagram(name) {
                print_diagram_with_boundary(name, diag, complex, display);
            } else {
                display.meta(&format!("{} (dim {}): (no diagram)", name, dim));
            }
        }
        return;
    }

    // Let bindings are in the diagrams table but not the generators table.
    if let Some(diag) = complex.find_diagram(name) {
        print_diagram_with_boundary(name, diag, complex, display);
        // Also show the definition (the full diagram render).
        display.meta(&format!("  = {}", render_diagram(diag, complex)));
        return;
    }

    display.error(&format!("'{}' not found in type", name));
}

fn print_diagram_with_boundary(name: &str, diag: &crate::core::diagram::Diagram, complex: &Complex, display: &Display) {
    let d = diag.top_dim();
    if d == 0 {
        display.meta(&format!("{} (dim 0): 0-cell", name));
        return;
    }
    let k = d - 1;
    match (
        crate::core::diagram::Diagram::boundary(Sign::Source, k, diag),
        crate::core::diagram::Diagram::boundary(Sign::Target, k, diag),
    ) {
        (Ok(src), Ok(tgt)) => display.meta(&format!(
            "{} (dim {}): {}  ->  {}",
            name, d,
            render_diagram(&src, complex),
            render_diagram(&tgt, complex),
        )),
        _ => display.meta(&format!("{} (dim {}): (boundary extraction failed)", name, d)),
    }
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
            dispatch_rules(engine.type_complex(), engine.store(), Some(n + 1), display);
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
                        display.meta(&format!("Loaded session from '{}'.", path));
                        show_state(engine, display);
                    }
                }
            }
        }
        Cmd::Help => print_help(display),
        Cmd::Quit => {}   // handled by caller
        // These are all handled before dispatch_engine_cmd is reached
        Cmd::Clear | Cmd::Source(_) | Cmd::Target(_) | Cmd::AtExpr(_) | Cmd::Types | Cmd::Print(_) => unreachable!(),
        Cmd::Unknown(s) => display.error(&format!("unknown command '{}' — type 'help' for a list", s)),
    }
}

fn print_help(display: &Display) {
    display.meta(
        "Commands:\n\
         \x20 @ <type>         Select a type from the loaded file\n\
         \x20 types            List all types in the file\n\
         \x20 print            Print the whole file (as alifib would)\n\
         \x20 print <name>     Print a cell: boundary, and definition if let-bound\n\
         \x20 source <name>    Set the source diagram\n\
         \x20 target <name>    Set the target diagram\n\
         \x20 apply <n>        Apply rewrite at index <n>            (alias: a)\n\
         \x20 undo             Undo the last step                    (alias: u)\n\
         \x20 undo <n>         Undo back to step <n>\n\
         \x20 undo all         Reset to source (= restart)\n\
         \x20 restart          Reset to source diagram\n\
         \x20 clear            Destroy engine and type, return to setup phase\n\
         \x20 show             Redisplay current state\n\
         \x20 rules            List generators in the selected type  (alias: r)\n\
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
    AtExpr(String),  // everything after the @ (handed to language::parse_complex)
    Types,
    Print(Option<String>),  // None = whole complex, Some(name) = specific cell
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
    // `@ ...` — type selection using the language parser
    if let Some(rest) = line.strip_prefix('@') {
        return Cmd::AtExpr(rest.trim().to_owned());
    }

    let mut parts = line.splitn(2, char::is_whitespace);
    let word = parts.next().unwrap_or("").trim();
    let rest = parts.next().map(str::trim).unwrap_or("");

    match word {
        "types" | "Types" => Cmd::Types,
        "print" => {
            if rest.is_empty() { Cmd::Print(None) }
            else { Cmd::Print(Some(rest.to_owned())) }
        }
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
