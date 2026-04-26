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
//! Setup (always available):
//! ```text
//! @ <type>         Select a type from the loaded file
//! types            List all types in the file
//! type <name>      Inspect a type: generators, diagrams, maps
//! homology <name>  Compute cellular homology of a type
//! source <name>    Set the source diagram  (requires type to be selected)
//! target <name>    Set the target diagram  (requires type to be selected)
//! status / show    Session state, or setup state when idle
//! print            Print the whole source file
//! rules            List generators in the selected type  (alias: r)
//! clear            Destroy engine and type selection, return to setup
//! help / ?         Show command list
//! quit / exit / q  Exit
//! ```
//!
//! Rewriting (require active session):
//! ```text
//! apply <n>        Apply rewrite at index <n>            (alias: a)
//! auto <n>         Apply up to <n> rewrites automatically, always picking index 0
//! undo             Undo the last step                    (alias: u)
//! undo <n>         Undo back to step <n>
//! undo all         Reset to source diagram               (= restart)
//! rules            List rewrite rules at current dimension  (alias: r)
//! history          Show the move history                 (alias: h)
//! proof            Show the running proof diagram        (alias: p)
//! store <name>     Store the current proof as a named diagram
//! save <path>      Write source file with stored definitions appended
//! load <path>      Load and replay a session file        (alias: l)
//! ```

use std::sync::Arc;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::EditMode;

use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Diagram, Sign};
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

                for part in line.split(';') {
                    let part = part.trim();
                    if part.is_empty() { continue; }
                    match part {
                        "done" | "accept" | "d" | "a" => return GoalOutcome::Done,
                        "abandon" => return GoalOutcome::Abandoned,
                        _ => {
                            if dispatch_rewrite_command(engine, part, display) == DispatchResult::Quit {
                                return GoalOutcome::Abandoned;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Write the original file content with stored definitions appended as local blocks.
///
/// Each stored definition becomes `@ TypeName {\n  let name = label\n}\n`.
fn write_updated_file(
    source_file: &str,
    _file_output: &str,
    stored_defs: &[(String, String, String)],
    path: &str,
) -> Result<(), String> {
    use std::fmt::Write as _;
    let original = std::fs::read_to_string(source_file)
        .map_err(|e| format!("cannot read '{}': {}", source_file, e))?;
    let mut out = original.trim_end().to_owned();
    for (type_name, def_name, label) in stored_defs {
        write!(out, "\n\n@{}\nlet {} = {}", type_name, def_name, label)
            .map_err(|e| e.to_string())?;
    }
    out.push('\n');
    std::fs::write(path, &out).map_err(|e| format!("cannot write '{}': {}", path, e))
}

/// Run the interactive REPL starting from a loaded file.
///
/// `type_name`, `source_diagram`, and `target_diagram` may be given as CLI
/// arguments; any that are omitted must be set interactively.
/// `emacs_mode` selects Emacs keybindings; the default is vi mode.
#[allow(clippy::result_unit_err)]
pub fn run_repl(
    source_file: &str,
    type_name: Option<&str>,
    source_diagram: Option<&str>,
    target_diagram: Option<&str>,
    emacs_mode: bool,
) -> Result<(), ()> {
    let display = Display::new();

    let (mut store, canonical_path, file_output) = match load_file_context(source_file) {
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
    // Definitions stored this session: (type_name, def_name, rendered_label)
    let mut stored_defs: Vec<(String, String, String)> = Vec::new();

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

    'repl: loop {
        match rl.readline("> ") {
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break,
            Err(e) => { display.error(&format!("read error: {e}")); break; }
            Ok(line) => {
                let line = line.trim().to_owned();
                if line.is_empty() { continue; }
                rl.add_history_entry(&line).ok();

                for part in line.split(';') {
                let part = part.trim();
                if part.is_empty() { continue; }
                match parse_command(part) {
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
                    Cmd::Status => {
                        match engine.as_ref() {
                            Some(e) => show_state(e, &display),
                            None => dispatch_status(
                                source_file,
                                type_name_str.as_deref(),
                                type_complex.as_deref(),
                                pending_source.as_deref(),
                                pending_target.as_deref(),
                                &display,
                            ),
                        }
                    }
                    Cmd::PrintFile => {
                        let trimmed = file_output.trim_end();
                        if !trimmed.is_empty() { display.file(trimmed); }
                    }
                    Cmd::Type(name) => {
                        // Pass the live complex if it's for the active type, so that
                        // proofs stored this session are visible in the output.
                        let live_tc: Option<&Complex> =
                            if engine.as_ref().map(|e| e.type_name()) == Some(name.as_str()) {
                                engine.as_ref().map(|e| e.type_complex())
                            } else if type_name_str.as_deref() == Some(name.as_str()) {
                                type_complex.as_deref()
                            } else {
                                None
                            };
                        dispatch_print_type(&store, &canonical_path, &name, live_tc, &display);
                    }
                    Cmd::Homology(name) => {
                        dispatch_homology(&store, &canonical_path, &name, &display);
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
                    Cmd::Quit => break 'repl,

                    // ── Commands that need type to be set ─────────────
                    Cmd::Source(name) => {
                        if type_complex.is_none() {
                            display.error("no type selected — use '@ <TypeName>' first");
                        } else {
                            // Only pre-print if the engine won't start and it's a simple name.
                            let engine_will_start = type_name_str.is_some()
                                && pending_target.is_some();
                            let is_simple_name = !name.contains(char::is_whitespace);
                            if !engine_will_start && is_simple_name
                                && let Some(tc) = type_complex.as_deref() {
                                dispatch_print_cell(tc, &name, &display);
                            } else if !engine_will_start {
                                display.meta(&format!("source: {}", name));
                            }
                            pending_source = Some(name);
                            maybe_start_engine(
                                &type_complex, &type_name_str, &pending_source, &pending_target,
                                &store, source_file, &display, &mut engine,
                            );
                        }
                    }
                    Cmd::Target(name) => {
                        if type_complex.is_none() {
                            display.error("no type selected — use '@ <TypeName>' first");
                        } else {
                            // Only pre-print if the engine won't start and it's a simple name.
                            let engine_will_start = type_name_str.is_some()
                                && pending_source.is_some();
                            let is_simple_name = !name.contains(char::is_whitespace);
                            if !engine_will_start && is_simple_name
                                && let Some(tc) = type_complex.as_deref() {
                                dispatch_print_cell(tc, &name, &display);
                            } else if !engine_will_start {
                                display.meta(&format!("target: {}", name));
                            }
                            pending_target = Some(name);
                            maybe_start_engine(
                                &type_complex, &type_name_str, &pending_source, &pending_target,
                                &store, source_file, &display, &mut engine,
                            );
                        }
                    }
                    Cmd::Rules => {
                        match (&engine, &type_complex) {
                            (Some(e), _) => dispatch_rules(e.type_complex(), e.store(), Some(e.current_diagram().top_dim() + 1), &display),
                            (None, Some(tc)) => dispatch_rules(tc, &store, None, &display),
                            (None, None) => display.error("set type first (@ <TypeName>)"),
                        }
                    }
                    // ── Always dispatch errors regardless of engine state ──
                    Cmd::Unknown(s) => display.error(&format!("unrecognised command '{}' — type 'help' for a list", s)),
                    Cmd::UsageError(usage) => display.error(&format!("usage: {}", usage)),

                    // ── Rewriting-phase commands (require engine) ─────
                    cmd => match engine.as_mut() {
                        None => {
                            let msg = match (type_complex.is_some(), pending_source.is_some(), pending_target.is_some()) {
                                (false, _, _) => "no type selected — use '@ <TypeName>'",
                                (true, false, false) => "set source and target first",
                                (true, true, false) => "set target first",
                                (true, false, true) => "set source first",
                                (true, true, true) => "engine failed to start — check source/target names",
                            };
                            display.error(msg);
                        }
                        Some(e) => {
                            match cmd {
                                // Store/Save handled here: need access to outer type_complex and stored_defs.
                                Cmd::Store(name) => {
                                    let source_expr = if e.steps().is_empty() {
                                        None
                                    } else {
                                        let n = e.source_diagram().top_dim();
                                        let scope = e.type_complex();
                                        let mut steps = e.steps().iter();
                                        let first = render_diagram(steps.next().unwrap(), scope);
                                        let rest: String = steps
                                            .map(|s| format!("\n#{} {}", n, render_diagram(s, scope)))
                                            .collect();
                                        Some(format!("{}{}", first, rest))
                                    };
                                    match e.register_proof(&name) {
                                        Ok((new_store, new_complex)) => {
                                            store = new_store;
                                            type_complex = Some(new_complex);
                                            display.meta(&format!("Stored '{}'.", name));
                                            dispatch_print_cell(e.type_complex(), &name, &display);
                                            if let (Some(tn), Some(expr)) = (type_name_str.as_deref(), source_expr) {
                                                stored_defs.push((tn.to_owned(), name, expr));
                                            }
                                        }
                                        Err(err) => display.error(&err),
                                    }
                                }
                                Cmd::Save(path) => {
                                    match write_updated_file(source_file, &file_output, &stored_defs, &path) {
                                        Ok(()) => display.meta(&format!("Saved to '{}'.", path)),
                                        Err(e) => display.error(&e),
                                    }
                                }
                                cmd => dispatch_engine_cmd(e, cmd, &display),
                            }
                        }
                    },
                }
                } // end for part in line.split(';')
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
        Cmd::Clear | Cmd::Source(_) | Cmd::Target(_) | Cmd::AtExpr(_) | Cmd::Types | Cmd::PrintFile => {
            display.error("command not available here");
        }
        Cmd::Type(_) | Cmd::Homology(_) => display.error("command not available here"),
        Cmd::Status => show_state(engine, display),
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

/// Compute and display cellular homology of a type.
fn dispatch_homology(store: &GlobalStore, canonical_path: &str, name: &str, display: &Display) {
    use super::engine::resolve_type;
    match resolve_type(store, canonical_path, name) {
        Err(e) => display.error(&e),
        Ok(tc) => {
            let h = crate::core::homology::compute_homology(&tc);
            if h.groups.is_empty() {
                display.meta("  (no generators)");
            } else {
                for (dim, group) in &h.groups {
                    display.inspect(&format!("  H_{} = {}", dim, group));
                    if let Some(witnesses) = h.torsion_witnesses.get(dim) {
                        for w in witnesses {
                            display.meta(&format!("    {}", w));
                        }
                    }
                }
                display.meta(&format!("  χ = {}", h.euler_characteristic));
            }
        }
    }
}

/// List all types declared in the file.
fn dispatch_types(store: &GlobalStore, canonical_path: &str, display: &Display) {
    let normalized = store.normalize();
    let Some(module) = normalized.modules.iter().find(|m| m.path == canonical_path) else {
        display.error("module not found");
        return;
    };
    if module.types.is_empty() {
        display.meta("  (no types found)");
        return;
    }
    for ty in module.types.iter().filter(|t| !t.name.is_empty()) {
        let total_generators: usize = ty.dims.iter().map(|d| d.cells.len()).sum();
        let max_dim = ty.dims.iter().map(|d| d.dim).max();
        let mut parts = Vec::new();
        if let Some(d) = max_dim {
            parts.push(format!("dim {}", d));
        }
        if total_generators > 0 {
            parts.push(format!("{} generator{}", total_generators, if total_generators == 1 { "" } else { "s" }));
        }
        if !ty.diagrams.is_empty() {
            let n = ty.diagrams.len();
            parts.push(format!("{} diagram{}", n, if n == 1 { "" } else { "s" }));
        }
        if !ty.maps.is_empty() {
            let n = ty.maps.len();
            parts.push(format!("{} map{}", n, if n == 1 { "" } else { "s" }));
        }
        if parts.is_empty() {
            display.inspect(&format!("  {}", ty.name));
        } else {
            display.inspect(&format!("  {} ({})", ty.name, parts.join(", ")));
        }
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
                let e = engine.as_ref().unwrap();
                show_diagram_or_name(src, e.source_diagram(), e.type_complex(), display);
                if let Some(tgt_diag) = e.target_diagram() {
                    show_diagram_or_name(tgt, tgt_diag, e.type_complex(), display);
                }
            }
            Err(e) => display.error(&e),
        }
    }
}

/// Call `print_state` with fields drawn from `engine`.
///
/// When the proof is complete (`target_reached`), runs `typecheck_proof` and reports
/// any failure as an error before displaying the completion message.
fn show_state(engine: &RewriteEngine, display: &Display) {
    let src_label = render_diagram(engine.source_diagram(), engine.type_complex());
    let tgt_label = engine.target_diagram()
        .map(|t| render_diagram(t, engine.type_complex()));

    // Only assemble the full proof diagram when the goal is reached — the
    // rewrite steps are not pasted together until storage or typechecking.
    let proof_label = if engine.target_reached() {
        if let Err(e) = engine.typecheck_proof() {
            display.error(&format!("proof typecheck failed: {}", e));
        }
        match engine.proof_label() {
            Ok(pl) => pl,
            Err(e) => { display.error(&format!("assembling proof failed: {}", e)); None }
        }
    } else {
        None
    };

    let proof = match (&tgt_label, &proof_label) {
        (Some(tl), Some(pl)) => Some((src_label.as_str(), tl.as_str(), pl.as_str())),
        _ => None,
    };

    print_state(
        display,
        engine.current_diagram(),
        engine.target_diagram(),
        engine.rewrites(),
        engine.type_complex(),
        proof,
    );
}

/// Show the current proof status.
///
/// With an active engine: shows module, type, fully-expanded source and target,
/// and the cell built so far.  Without an engine: shows module, type (if set),
/// Show setup-phase state: module path, type name (if set), and any pending
/// source/target diagrams. Only called when no engine is active — in rewriting
/// mode `status` calls `show_state` directly.
fn dispatch_status(
    source_file: &str,
    type_name_str: Option<&str>,
    type_complex: Option<&Complex>,
    pending_source: Option<&str>,
    pending_target: Option<&str>,
    display: &Display,
) {
    display.meta(&format!("module: {}", source_file));
    match type_name_str {
        Some(tn) => display.meta(&format!("type:   {}", tn)),
        None     => display.meta("type:   (not set)"),
    }
    if let Some(tc) = type_complex {
        if let Some(src) = pending_source {
            display.meta("source:");
            dispatch_print_cell(tc, src, display);
        }
        if let Some(tgt) = pending_target {
            display.meta("target:");
            dispatch_print_cell(tc, tgt, display);
        }
    }
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
        if let Some(d) = filter_dim
            && dim != d { continue; }
        any = true;
        match store.cell_data_for_tag(complex, tag) {
            Some(CellData::Boundary { boundary_in, boundary_out }) => {
                if filter_dim.is_some() {
                    display.inspect(&format!(
                        "  {} : {}  ->  {}",
                        name,
                        render_diagram(&boundary_in, complex),
                        render_diagram(&boundary_out, complex),
                    ));
                } else {
                    display.inspect(&format!(
                        "  {} (dim {}): {}  ->  {}",
                        name, dim,
                        render_diagram(&boundary_in, complex),
                        render_diagram(&boundary_out, complex),
                    ));
                }
            }
            Some(CellData::Zero) => display.inspect(&format!("  {} (dim 0): 0-cell", name)),
            _ => display.inspect(&format!("  {} (no boundaries)", name)),
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

/// Returns `"generator"` or `"local definition"` for a named cell.
fn cell_kind(complex: &Complex, name: &str) -> &'static str {
    if complex.find_generator(name).is_some() { "generator" } else { "local definition" }
}

/// Print a named type from the module by looking it up in the normalized store.
///
/// Generators and maps use the standard normalized layout. Diagrams (let-bindings
/// and locally stored proofs) are read from `live_complex` so that definitions
/// added via `store` during this session are visible.
fn dispatch_print_type(
    store: &GlobalStore,
    canonical_path: &str,
    name: &str,
    live_complex: Option<&Complex>,
    display: &Display,
) {
    use crate::aux::Tag;
    use std::fmt::Write as _;

    let normalized = store.normalize();
    let module = normalized.modules.iter().find(|m| m.path == canonical_path);
    let Some(module) = module else {
        display.error(&format!("module '{}' not found", canonical_path));
        return;
    };
    let Some(ty) = module.types.iter().find(|t| t.name == name) else {
        display.error(&format!("type '{}' not found in file", name));
        return;
    };

    // Resolve the type complex: prefer the live one (includes session additions),
    // fall back to the global store's snapshot.
    let store_complex = (|| -> Option<&Complex> {
        let mc = store.find_module(canonical_path)?;
        let (tag, _) = mc.find_generator(name)?;
        let gid = match tag { Tag::Global(gid) => *gid, _ => return None };
        store.find_type(gid).map(|e| e.complex.as_ref())
    })();
    let tc: &Complex = match live_complex.or(store_complex) {
        Some(c) => c,
        None => { display.file(ty.to_string().trim_end()); return; }
    };

    // Build output manually so we can append `= expr` for each diagram.
    let mut out = String::new();
    let label = if ty.name.is_empty() { "<empty>" } else { &ty.name };
    writeln!(out, "Type {}", label).ok();
    if ty.dims.is_empty() {
        writeln!(out, "  (no cells)").ok();
    } else {
        for dg in &ty.dims {
            writeln!(out, "  [{}]", dg.dim).ok();
            for cell in &dg.cells {
                writeln!(out, "    {}", cell).ok();
            }
        }
    }
    // Iterate the live complex's diagrams so session-stored proofs appear too.
    let mut diag_list: Vec<(&str, &crate::core::diagram::Diagram)> =
        tc.diagrams_iter().map(|(n, d)| (n.as_str(), d)).collect();
    diag_list.sort_by_key(|(n, _)| *n);
    if !diag_list.is_empty() {
        writeln!(out, "  Diagrams").ok();
        for (diag_name, diag) in &diag_list {
            let k = diag.top_dim().checked_sub(1);
            let boundary = k.and_then(|k| {
                let src = crate::core::diagram::Diagram::boundary(Sign::Source, k, diag).ok()?;
                let tgt = crate::core::diagram::Diagram::boundary(Sign::Target, k, diag).ok()?;
                Some(format!("{} : {}  ->  {}", diag_name, render_diagram(&src, tc), render_diagram(&tgt, tc)))
            }).unwrap_or_else(|| diag_name.to_string());
            let expr = crate::output::render_diagram(diag, tc);
            writeln!(out, "    {}  = {}", boundary, expr).ok();
        }
    }
    if !ty.maps.is_empty() {
        writeln!(out, "  Maps").ok();
        for map in &ty.maps {
            writeln!(out, "    {}", map).ok();
        }
    }

    display.file(out.trim_end());
}

/// Print a named cell from the type complex.
///
/// - **Generator** (`name : src -> tgt`): labelled `generator`, shows boundary.
/// - **Let binding** (`let name = expr`): labelled `let`, shows boundary and `= definition`.
/// - **Neither**: reports an error.
fn dispatch_print_cell(complex: &Complex, name: &str, display: &Display) {
    // Generators are in the generators table and have cell data.
    if let Some((_, dim)) = complex.find_generator(name) {
        display.inspect(&format!("{} (dim {}, {})", name, dim, cell_kind(complex, name)));
        if dim > 0
            && let Some(diag) = complex.find_diagram(name) {
            print_diagram_with_boundary(diag, complex, display);
        }
        return;
    }

    // Let bindings are in the diagrams table but not the generators table.
    if let Some(diag) = complex.find_diagram(name) {
        let dim = diag.top_dim();
        display.inspect(&format!("{} (dim {}, {})", name, dim, cell_kind(complex, name)));
        if dim > 0 {
            print_diagram_with_boundary(diag, complex, display);
        }
        display.inspect(&format!("  = {}", crate::output::render_diagram(diag, complex)));
        return;
    }

    display.error(&format!("'{}' not found in type", name));
}

/// Display a source or target diagram after the engine starts.
///
/// If `label` is a known name in `complex`, delegates to [`dispatch_print_cell`]
/// for the rich name/dim/boundary output. Otherwise renders the diagram and
/// shows its boundary directly — handles expression inputs like `"f g"`.
fn show_diagram_or_name(label: &str, diag: &Diagram, complex: &Complex, display: &Display) {
    if complex.find_generator(label).is_some() || complex.find_diagram(label).is_some() {
        dispatch_print_cell(complex, label, display);
    } else {
        let rendered = render_diagram(diag, complex);
        display.inspect(&format!("{}  =  {}", label, rendered));
        if diag.top_dim() > 0 {
            print_diagram_with_boundary(diag, complex, display);
        }
    }
}

fn print_diagram_with_boundary(diag: &crate::core::diagram::Diagram, complex: &Complex, display: &Display) {
    let d = diag.top_dim();
    let k = d - 1;
    match (
        crate::core::diagram::Diagram::boundary(Sign::Source, k, diag),
        crate::core::diagram::Diagram::boundary(Sign::Target, k, diag),
    ) {
        (Ok(src), Ok(tgt)) => display.inspect(&format!(
            "  : {}  ->  {}",
            render_diagram(&src, complex),
            render_diagram(&tgt, complex),
        )),
        _ => display.inspect("  (boundary extraction failed)"),
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
        Cmd::Auto(n) => {
            match engine.auto(n) {
                Ok((applied, stop_reason)) => {
                    let tail = stop_reason.map(|r| format!(" ({})", r)).unwrap_or_default();
                    display.meta(&format!(
                        "Applied {} step{}{}.",
                        applied, if applied == 1 { "" } else { "s" }, tail,
                    ));
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
        Cmd::Rules => {
            let n = engine.current_diagram().top_dim();
            dispatch_rules(engine.type_complex(), engine.store(), Some(n + 1), display);
        }
        Cmd::History => {
            let sf = engine.to_session_file();
            let entries: Vec<(Option<usize>, &str)> = sf.moves.iter()
                .map(|m| (m.choice, m.rule_name.as_str()))
                .collect();
            print_history(display, engine.source_diagram(), &entries, engine.type_complex());
        }
        Cmd::Proof => {
            let steps = engine.steps();
            if steps.is_empty() {
                display.meta("(no proof built yet)");
            } else {
                let n = engine.source_diagram().top_dim();
                let scope = engine.type_complex();
                // Render as step1 #n step2 #n ... #n stepN — the individual
                // rewrite steps are not pasted together for display.
                let proof_expr = steps.iter()
                    .map(|s| render_diagram(s, scope))
                    .collect::<Vec<_>>()
                    .join(&format!(" #{} ", n));
                display.inspect(&format!(
                    "{} : {} -> {}",
                    proof_expr,
                    render_diagram(engine.source_diagram(), scope),
                    render_diagram(engine.current_diagram(), scope),
                ));
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
        Cmd::Parallel(Some(on)) => {
            engine.set_parallel(on);
            display.meta(&format!("Parallel mode {}.", if on { "on" } else { "off" }));
        }
        Cmd::Parallel(None) => {
            display.meta(&format!("Parallel mode {}.", if engine.parallel() { "on" } else { "off" }));
        }
        Cmd::Help => print_help(display),
        Cmd::Quit => {}   // handled by caller
        // These are all handled before dispatch_engine_cmd is reached
        Cmd::Clear | Cmd::Source(_) | Cmd::Target(_) | Cmd::AtExpr(_) | Cmd::Types
        | Cmd::PrintFile | Cmd::Type(_) | Cmd::Homology(_) | Cmd::Status
        | Cmd::Store(_) | Cmd::Save(_) | Cmd::Unknown(_) | Cmd::UsageError(_) => unreachable!(),
    }
}

fn print_help(display: &Display) {
    display.meta(
        "Setup commands (always available):\n\
         \x20 @ <type>         Select a type from the loaded file\n\
         \x20 types            List all types in the file\n\
         \x20 type <name>      Inspect a type: generators, diagrams, maps\n\
         \x20 homology <name>  Compute cellular homology of a type\n\
         \x20 source <name>    Set the source diagram  (requires type to be selected)\n\
         \x20 target <name>    Set the target diagram  (requires type to be selected)\n\
         \x20 status / show    Session state, or setup state when idle\n\
         \x20 print            Print the whole source file\n\
         \x20 rules            List generators in the selected type  (alias: r)\n\
         \x20 clear            Destroy engine and type selection, return to setup\n\
         \x20 help / ?         Show this help\n\
         \x20 quit / exit / q  Exit\n\
         \n\
         Rewriting commands (require active session):\n\
         \x20 apply <n>        Apply rewrite at index <n>            (alias: a)\n\
         \x20 auto <n>         Apply up to <n> rewrites, always picking index 0\n\
         \x20 parallel [on|off] Show or toggle parallel rewrite mode  (default: on)\n\
         \x20 undo             Undo the last step                    (alias: u)\n\
         \x20 undo <n>         Undo back to step <n>\n\
         \x20 undo all         Reset to source diagram               (= restart)\n\
         \x20 rules            List rewrite rules at current dimension  (alias: r)\n\
         \x20 history          Show the move history                 (alias: h)\n\
         \x20 proof            Show the running proof diagram        (alias: p)\n\
         \x20 store <name>     Store the current proof as a named diagram\n\
         \x20 save <path>      Write source file with stored definitions appended\n\
         \x20 load <path>      Load and replay a session file        (alias: l)"
    );
}

// ── Command parsing ───────────────────────────────────────────────────────────

/// A parsed REPL command.
enum Cmd {
    /// `@ <expr>` — select a type; the string is handed to `language::parse_complex`.
    AtExpr(String),
    /// `types` — list all types in the file.
    Types,
    /// `status` / `show` — show rewrite state when engine active, setup state otherwise.
    Status,
    /// `print` — print the full source file.
    PrintFile,
    /// `type <name>` — inspect a type and its generators.
    Type(String),
    /// `source <name>` — set the source diagram.
    Source(String),
    /// `target <name>` — set the target (goal) diagram.
    Target(String),
    /// `apply <n>` — apply the nth candidate rewrite.
    Apply(usize),
    /// `auto <n>` — apply up to `n` rewrites automatically, always picking the
    /// first available candidate each step.
    Auto(usize),
    /// `undo [<n>]` — undo the last step, or undo back to step n.
    Undo(Option<usize>),
    /// `undo all` — undo all steps.
    UndoAll,
    /// `restart` — alias for `undo all`.
    Restart,
    /// `clear` — destroy engine and type selection, return to setup phase.
    Clear,
    /// `rules` — list generators (or rewrite rules when engine active).
    Rules,
    /// `history` — display the move log.
    History,
    /// `proof` — display the running proof cell.
    Proof,
    /// `store <name>` — store the current proof as a named diagram.
    Store(String),
    /// `save <path>` — write the original file with stored definitions appended.
    Save(String),
    /// `load <path>` — load and replay a session file.
    Load(String),
    /// `homology <name>` — compute cellular homology of a type.
    Homology(String),
    /// `parallel [on|off]` — show or toggle parallel rewrite mode.
    Parallel(Option<bool>),
    Help,
    Quit,
    /// Unrecognised command word.
    Unknown(String),
    /// Recognised command with wrong arguments.
    UsageError(String),
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
        "status" | "show" => Cmd::Status,
        "print" => {
            if rest.is_empty() {
                Cmd::PrintFile
            } else {
                Cmd::UsageError("print  |  type <name>  |  cell <name>".to_owned())
            }
        }
        "type" => {
            if rest.is_empty() { Cmd::UsageError("type <name>".to_owned()) }
            else { Cmd::Type(rest.to_owned()) }
        }
        "homology" => {
            if rest.is_empty() { Cmd::UsageError("homology <name>".to_owned()) }
            else { Cmd::Homology(rest.to_owned()) }
        }
        "source" => {
            if rest.is_empty() { Cmd::UsageError("source <name>".to_owned()) }
            else { Cmd::Source(rest.to_owned()) }
        }
        "target" => {
            if rest.is_empty() { Cmd::UsageError("target <name>".to_owned()) }
            else { Cmd::Target(rest.to_owned()) }
        }
        "apply" | "a" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Apply(n),
                Err(_) => Cmd::UsageError("apply <n>".to_owned()),
            }
        }
        "auto" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Auto(n),
                Err(_) => Cmd::UsageError("auto <n>".to_owned()),
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
                    Err(_) => Cmd::UsageError("undo  |  undo <n>  |  undo all".to_owned()),
                }
            }
        }
        "restart" => Cmd::Restart,
        "clear"   => Cmd::Clear,
        "rules" | "r" => Cmd::Rules,
        "history" | "h" => Cmd::History,
        "proof" | "p"   => Cmd::Proof,
        "store" => {
            if rest.is_empty() { Cmd::UsageError("store <name>".to_owned()) }
            else { Cmd::Store(rest.to_owned()) }
        }
        "save" => {
            if rest.is_empty() { Cmd::UsageError("save <path>".to_owned()) }
            else { Cmd::Save(rest.to_owned()) }
        }
        "load" | "l" => {
            if rest.is_empty() { Cmd::UsageError("load <path>".to_owned()) }
            else { Cmd::Load(rest.to_owned()) }
        }
        "parallel" => {
            match rest {
                "on" => Cmd::Parallel(Some(true)),
                "off" => Cmd::Parallel(Some(false)),
                "" => Cmd::Parallel(None),
                _ => Cmd::UsageError("parallel [on|off]".to_owned()),
            }
        }
        "help" | "?" => Cmd::Help,
        "quit" | "exit" | "q" => Cmd::Quit,
        other => Cmd::Unknown(other.to_owned()),
    }
}
