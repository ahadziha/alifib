//! In-process interactive REPL for rewrite sessions.
//!
//! The REPL has two states:
//!
//! - **No session** — file is loaded.  Non-session commands work: `types`,
//!   `type`, `homology`, `status`, `print`.  Use `start <type> <source>
//!   [<target>]` to begin a rewrite session.
//! - **Session active** — engine running; `apply`, `undo`, `redo`,
//!   `restart`, `stop`, `show`, `history`, `proof`, `save`, etc.
//!
//! All human-readable output flows through a single [`Display`] value.
//! Readline (with vi or emacs mode) is provided by `rustyline`.
//!
//! # Commands
//!
//! Always available:
//! ```text
//! types            List all types in the file
//! type <name>      Inspect a type: generators, diagrams, maps
//! homology <name>  Compute cellular homology of a type
//! start <t> <s> [<g>]  Start a rewrite session (target optional)
//! status / show    Session state, or module path when idle
//! print            Print the whole source file
//! stop             End the active session
//! help / ?         Show command list
//! quit / exit / q  Exit
//! ```
//!
//! Require active session:
//! ```text
//! apply <n>        Apply rewrite at index <n> (alias: a)
//! auto <n>         Apply up to <n> rewrites automatically, always picking index 0
//! random <n>       Apply randomly selected rewrites automatically
//! undo             Undo the last step (alias: u)
//! undo <n>         Undo back to step <n>
//! undo all         Reset to source diagram (= restart)
//! redo             Redo the last undone step
//! redo <n>         Redo forward to step <n>
//! rules            List rewrite rules at current dimension (alias: r)
//! history          Show the move history (alias: h)
//! proof            Show the running proof diagram (alias: p)
//! store <name>     Store the current proof as a named diagram
//! save <path>      Write source file with stored definitions appended
//! ```

use std::borrow::Cow;
use std::sync::Arc;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::history::FileHistory;
use rustyline::{EditMode, Editor};

use crate::core::complex::Complex;
use crate::core::diagram::{CellData, Sign};
use crate::interpreter::GlobalStore;
use crate::output::render_diagram;
use super::display::Display;
use super::engine::{RewriteEngine, load_file_context, resolve_type};
use super::fill::{filled_report, finalize, list_open_holes, start_fill, FillContext, FillSession, ZeroCellFill};
use super::render::{print_history, print_state};

// ── Readline editor with a coloured prompt ──────────────────────────────────────

/// Minimal rustyline helper that renders the prompt in colour.
///
/// The `derive` feature is off, so the marker traits are implemented by hand;
/// only [`Highlighter::highlight_prompt`] does any work.
struct ReplHelper {
    prompt: String,
}

impl rustyline::completion::Completer for ReplHelper {
    type Candidate = String;
}
impl rustyline::hint::Hinter for ReplHelper {
    type Hint = String;
}
impl rustyline::validate::Validator for ReplHelper {}
impl Highlighter for ReplHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        _prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Borrowed(&self.prompt)
    }
}
impl rustyline::Helper for ReplHelper {}

type ReplEditor = Editor<ReplHelper, FileHistory>;

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Register the running proof as a named diagram and append its definition to
/// the running source.  Shared by free sessions and rewrite fills — `store`
/// edits the in-memory source, which `save` later commits to disk.
fn store_proof(
    e: &mut RewriteEngine,
    name: &str,
    store: &mut Arc<GlobalStore>,
    source: &mut String,
    display: &Display,
) {
    let expr = if e.steps().is_empty() {
        // With no steps, store the initial diagram itself (re-rendered, never
        // its name) — matching the web REPL, and safe for a fill's synthetic
        // boundary name.
        render_diagram(e.initial_diagram(), e.type_complex())
    } else {
        let n = e.initial_diagram().top_dim();
        let scope = e.type_complex();
        let ordered: Vec<_> = if e.backward() {
            e.steps().iter().rev().collect()
        } else {
            e.steps().iter().collect()
        };
        let mut steps = ordered.iter();
        let first = render_diagram(steps.next().unwrap(), scope);
        let rest: String = steps
            .map(|s| format!("\n#{} {}", n, render_diagram(s, scope)))
            .collect();
        format!("{}{}", first, rest)
    };
    let type_name = e.type_name().to_owned();
    match e.register_proof(name) {
        Ok((new_store, _)) => {
            *store = new_store;
            *source = format!("{}\n\n@{}\nlet {} = {}\n", source.trim_end(), type_name, name, expr);
            display.meta(&format!("Stored '{}'", name));
            dispatch_print_cell(e.type_complex(), name, display);
        }
        Err(err) => display.error(&err),
    }
}

/// Run the interactive REPL starting from a loaded file.
///
/// `type_name`, `initial_diagram`, and `target_diagram` may be given as CLI
/// arguments to auto-start a session; otherwise the user starts one
/// interactively with `start <type> <source> [<target>]`.
/// `emacs_mode` selects Emacs keybindings; the default is vi mode.
#[allow(clippy::result_unit_err)]
pub fn run_repl(
    source_file: &str,
    type_name: Option<&str>,
    initial_diagram: Option<&str>,
    target_diagram: Option<&str>,
    emacs_mode: bool,
) -> Result<(), ()> {
    let display = Display::new();

    let (mut store, canonical_path, _file_output) = match load_file_context(source_file) {
        Ok(r) => r,
        Err(e) => { display.error(&e); return Err(()); }
    };

    display.meta(&format!("Loaded {}", source_file));

    let mut rl = make_editor(emacs_mode, &display);
    let mut engine: Option<RewriteEngine> = None;
    let mut fill: Option<(FillContext, FillSession)> = None;
    let mut backward = false;

    // The working copy of the source, edited in memory on `done` and written on
    // `save`.  Re-evaluation after a fill reads this, not the disk.
    let mut source = std::fs::read_to_string(source_file).unwrap_or_default();

    // Auto-start from CLI flags when type and source are given.
    if let (Some(tn), Some(src)) = (type_name, initial_diagram) {
        try_start_session(
            &store, &canonical_path, tn, src, target_diagram,
            backward, &display, &mut engine,
        );
    }

    'repl: loop {
        match rl.readline("❯ ") {
            Err(ReadlineError::Eof) | Err(ReadlineError::Interrupted) => break,
            Err(e) => { display.error(&format!("Read error: {e}")); break; }
            Ok(line) => {
                let line = line.trim().to_owned();
                if line.is_empty() { continue; }
                rl.add_history_entry(&line).ok();

                for part in line.split(';') {
                let part = part.trim();
                if part.is_empty() { continue; }
                match parse_command(part) {
                    // ── Always-available commands ─────────────────────
                    Cmd::Types => dispatch_types(&store, &canonical_path, &display),
                    Cmd::Status => {
                        match (fill.as_ref(), engine.as_ref()) {
                            (Some((ctx, session)), _) => show_fill_state(ctx, session, &display),
                            (None, Some(e)) => show_state(e, &display),
                            (None, None) => display.meta(&format!("Module: {}", source_file)),
                        }
                    }
                    Cmd::PrintFile => {
                        let trimmed = source.trim_end();
                        if !trimmed.is_empty() { display.file(trimmed); }
                    }
                    Cmd::Type(name) => {
                        let live_tc: Option<&Complex> =
                            if engine.as_ref().map(|e| e.type_name()) == Some(name.as_str()) {
                                engine.as_ref().map(|e| e.type_complex())
                            } else {
                                None
                            };
                        dispatch_print_type(&store, &canonical_path, &name, live_tc, &display);
                    }
                    Cmd::Homology(name) => {
                        dispatch_homology(&store, &canonical_path, &name, &display);
                    }
                    Cmd::Stop => {
                        if fill.take().is_some() {
                            display.meta("Fill abandoned");
                        } else {
                            engine = None;
                            display.meta("Session stopped");
                        }
                    }
                    Cmd::Backward(arg) => {
                        if engine.is_some() {
                            let on = engine.as_ref().unwrap().backward();
                            display.meta(&format!("Backward mode {}", if on { "on" } else { "off" }));
                        } else {
                            match arg {
                                Some(on) => {
                                    backward = on;
                                    display.meta(&format!("Backward mode {}", if on { "on" } else { "off" }));
                                }
                                None => {
                                    display.meta(&format!("Backward mode {}", if backward { "on" } else { "off" }));
                                }
                            }
                        }
                    }
                    Cmd::Help => print_help(&display),
                    Cmd::Quit => break 'repl,

                    Cmd::Start(type_arg, source_arg, target_arg) => {
                        if engine.is_some() || fill.is_some() {
                            display.error("Session already active — use 'stop' first");
                        } else {
                            try_start_session(
                                &store, &canonical_path,
                                &type_arg, &source_arg, target_arg.as_deref(),
                                backward, &display, &mut engine,
                            );
                        }
                    }
                    Cmd::Resume(type_arg, proof_arg, target_arg) => {
                        if engine.is_some() || fill.is_some() {
                            display.error("Session already active — use 'stop' first");
                        } else {
                            try_resume_session(
                                &store, &canonical_path,
                                &type_arg, &proof_arg, target_arg.as_deref(),
                                backward, &display, &mut engine,
                            );
                        }
                    }

                    Cmd::Save(path) => {
                        // `save` commits the running source to disk; `store` and
                        // `done` edit that running source during a session.
                        let body = format!("{}\n", source.trim_end());
                        match std::fs::write(&path, &body) {
                            Ok(()) => display.meta(&format!("Saved to '{}'", path)),
                            Err(e) => display.error(&format!("Cannot write '{}': {}", path, e)),
                        }
                    }

                    // ── Hole-filling ─────────────────────────────────
                    Cmd::Holes => dispatch_holes(&store, &canonical_path, &display),
                    Cmd::Fill(n) => {
                        if engine.is_some() || fill.is_some() {
                            display.error("Session already active — use 'stop' first");
                        } else {
                            match start_fill(&store, &canonical_path, &canonical_path, n, backward) {
                                Ok((ctx, session)) => {
                                    announce_fill(&ctx, &session, &display);
                                    fill = Some((ctx, session));
                                }
                                Err(e) => display.error(&e),
                            }
                        }
                    }
                    Cmd::Done => match fill.as_ref() {
                        None => display.error("No active fill — use 'fill <n>'"),
                        Some((ctx, session)) => match session.filler() {
                            Err(e) => display.error(&e),
                            Ok(filler) => {
                                // Compose the report before finalising swaps the store out.
                                let message = filled_report(&store, ctx, &filler);
                                match finalize(&store, ctx, &filler, &canonical_path, &source) {
                                    Ok((new_store, new_source)) => {
                                        store = new_store;
                                        source = new_source;
                                        fill = None;
                                        display.meta(&message);
                                    }
                                    Err(e) => display.error(&e),
                                }
                            }
                        },
                    },

                    // ── Always dispatch errors regardless of engine state ──
                    Cmd::Unknown(s) => display.error(&format!("Unrecognised command '{}' — type 'help' for a list", s)),
                    Cmd::UsageError(usage) => display.error(&format!("Usage: {}", usage)),

                    // ── Session commands routed to the active fill ───
                    Cmd::Store(name) if fill.is_some() => {
                        match &mut fill.as_mut().unwrap().1 {
                            FillSession::Rewrite(e) => store_proof(e, &name, &mut store, &mut source, &display),
                            FillSession::ZeroCell(_) => display.error("Nothing to store in a 0-cell fill"),
                        }
                    }
                    cmd if fill.is_some() => {
                        let (_, session) = fill.as_mut().unwrap();
                        dispatch_fill_cmd(session, cmd, &display);
                    }

                    // ── Session commands (require engine) ────────────
                    cmd => match engine.as_mut() {
                        None => {
                            display.error("No active session — use 'start <type> <source> [<target>]'");
                        }
                        Some(e) => {
                            match cmd {
                                Cmd::Store(name) => store_proof(e, &name, &mut store, &mut source, &display),
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

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Build the readline editor with a coloured prompt.
///
/// rustyline measures prompt width from the string passed to `readline`, so we
/// pass the plain `❯ ` there and let [`ReplHelper`] substitute the coloured form
/// at render time — keeping the cursor correctly positioned.
fn make_editor(emacs_mode: bool, display: &Display) -> ReplEditor {
    let mut rl = ReplEditor::new().expect("readline init failed");
    rl.set_edit_mode(if emacs_mode { EditMode::Emacs } else { EditMode::Vi });
    rl.set_helper(Some(ReplHelper { prompt: display.acc("❯ ") }));
    rl
}

/// Resolve type, source, and optional target, then create the engine.
///
/// `canonical_path` is the store's module key (from [`load_file_context`]); the
/// engine uses it for in-store lookups such as `store`/`register_proof`.
#[allow(clippy::too_many_arguments)]
fn try_start_session(
    store: &Arc<GlobalStore>,
    canonical_path: &str,
    type_name: &str,
    source_name: &str,
    target_name: Option<&str>,
    backward: bool,
    display: &Display,
    engine: &mut Option<RewriteEngine>,
) {
    let tc = match resolve_type(store, canonical_path, type_name) {
        Ok(tc) => tc,
        Err(e) => { display.error(&e); return; }
    };
    match RewriteEngine::from_store(
        Arc::clone(store), tc, source_name, target_name,
        canonical_path.to_owned(), type_name.to_owned(), backward,
    ) {
        Ok(e) => {
            *engine = Some(e);
            display.meta("Started rewrite session");
            show_state(engine.as_ref().unwrap(), display);
        }
        Err(e) => display.error(&e),
    }
}

/// Resolve the type, then create a session by resuming from a proof diagram.
///
/// `canonical_path` is the store's module key (from [`load_file_context`]); the
/// engine uses it for in-store lookups such as `store`/`register_proof`.
#[allow(clippy::too_many_arguments)]
fn try_resume_session(
    store: &Arc<GlobalStore>,
    canonical_path: &str,
    type_name: &str,
    proof_name: &str,
    target_name: Option<&str>,
    backward: bool,
    display: &Display,
    engine: &mut Option<RewriteEngine>,
) {
    let tc = match resolve_type(store, canonical_path, type_name) {
        Ok(tc) => tc,
        Err(e) => { display.error(&e); return; }
    };
    match RewriteEngine::resume(
        Arc::clone(store), tc, proof_name, target_name,
        canonical_path.to_owned(), type_name.to_owned(), backward,
    ) {
        Ok(e) => {
            *engine = Some(e);
            display.meta("Resumed rewrite session");
            show_state(engine.as_ref().unwrap(), display);
        }
        Err(e) => display.error(&e),
    }
}

/// Compute and display cellular homology of a type.
fn dispatch_homology(store: &GlobalStore, canonical_path: &str, name: &str, display: &Display) {
    use super::engine::resolve_type;
    match resolve_type(store, canonical_path, name) {
        Err(e) => display.error(&e),
        Ok(tc) => {
            let h = crate::analysis::homology::compute_homology(&tc);
            if h.groups.is_empty() {
                display.meta("  (No generators)");
            } else {
                for (dim, group) in &h.groups {
                    display.meta(&format!("  H_{} = {}", dim, group));
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
        display.error("Module not found");
        return;
    };
    if module.types.is_empty() {
        display.meta("  (No types found)");
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
            display.inspect_rich(&format!("  {}", display.code(&ty.name)));
        } else {
            display.inspect_rich(&format!("  {} ({})", display.code(&ty.name), parts.join(", ")));
        }
    }
}

/// Call `print_state` with fields drawn from `engine`.
///
/// When the proof is complete (`target_reached`), runs `typecheck_proof` and reports
/// any failure as an error before displaying the completion message.
fn show_state(engine: &RewriteEngine, display: &Display) {
    let initial_label = render_diagram(engine.initial_diagram(), engine.type_complex());
    let goal_label = engine.target_diagram()
        .map(|t| render_diagram(t, engine.type_complex()));

    let proof_label = if engine.target_reached() {
        // Skip the typecheck on a step-0 (identity) proof: it is the initial
        // diagram, not a genuine (n+1)-cell, and `done` validates by re-evaluation.
        if engine.step_count() > 0 {
            if let Err(e) = engine.typecheck_proof() {
                display.error(&format!("Proof typecheck failed: {}", e));
            }
        }
        match engine.proof_label() {
            Ok(pl) => Some(pl),
            Err(e) => { display.error(&format!("Assembling proof failed: {}", e)); None }
        }
    } else {
        None
    };

    let proof = match (&goal_label, &proof_label) {
        (Some(gl), Some(pl)) => {
            if engine.backward() {
                Some((gl.as_str(), initial_label.as_str(), pl.as_str()))
            } else {
                Some((initial_label.as_str(), gl.as_str(), pl.as_str()))
            }
        }
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

/// Display generators in `complex`, optionally filtered to those at `filter_dim`.
///
/// Without a filter (setup phase, no source diagram yet), all generators are
/// shown with their dimensions.  With a filter (rewriting phase), only the
/// relevant rewrite rules are shown.
fn dispatch_rules(complex: &Complex, store: &GlobalStore, filter_dim: Option<usize>, display: &Display) {
    if let Some(d) = filter_dim {
        display.meta(&format!("Rewrite rules (dim {}):", d));
    } else {
        display.meta("Generators:");
    }
    let mut any = false;
    for (name, tag, dim) in complex.generators_iter() {
        if let Some(d) = filter_dim
            && dim != d { continue; }
        any = true;
        match store.cell_data_for_tag(complex, tag) {
            Some(CellData::Boundary { boundary_in, boundary_out }) => {
                let bd = format!("{} -> {}",
                    display.code(&render_diagram(&boundary_in, complex)),
                    display.code(&render_diagram(&boundary_out, complex)));
                if filter_dim.is_some() {
                    display.inspect_rich(&format!("  {} : {}", display.code(name), bd));
                } else {
                    display.inspect_rich(&format!("  {} (dim {}): {}", display.code(name), dim, bd));
                }
            }
            Some(CellData::Zero) => display.inspect_rich(&format!("  {} (dim 0): 0-cell", display.code(name))),
            _ => display.inspect_rich(&format!("  {} (no boundaries)", display.code(name))),
        }
    }
    if !any {
        if let Some(d) = filter_dim {
            display.meta(&format!("  (No rewrite rules at dim {})", d));
        } else {
            display.meta("  (No generators)");
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
        display.error(&format!("Module '{}' not found", canonical_path));
        return;
    };
    let Some(ty) = module.types.iter().find(|t| t.name == name) else {
        display.error(&format!("Type '{}' not found in file", name));
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
            let header = k.and_then(|k| {
                let src = crate::core::diagram::Diagram::boundary(Sign::Input, k, diag).ok()?;
                let tgt = crate::core::diagram::Diagram::boundary(Sign::Output, k, diag).ok()?;
                Some(format!("{} : {} -> {}", diag_name, render_diagram(&src, tc), render_diagram(&tgt, tc)))
            }).unwrap_or_else(|| diag_name.to_string());
            let expr = crate::output::render_diagram(diag, tc);
            writeln!(out, "    {}", header).ok();
            writeln!(out, "      = {}", expr).ok();
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
        display.inspect_rich(&format!("{} (dim {}, {})", display.code(name), dim, cell_kind(complex, name)));
        if dim > 0
            && let Some(diag) = complex.find_diagram(name) {
            print_diagram_with_boundary(diag, complex, display);
        }
        return;
    }

    // Let bindings are in the diagrams table but not the generators table.
    if let Some(diag) = complex.find_diagram(name) {
        let dim = diag.top_dim();
        display.inspect_rich(&format!("{} (dim {}, {})", display.code(name), dim, cell_kind(complex, name)));
        if dim > 0 {
            print_diagram_with_boundary(diag, complex, display);
        }
        display.inspect_rich(&format!("  = {}", display.code(&crate::output::render_diagram(diag, complex))));
        return;
    }

    display.error(&format!("'{}' not found in type", name));
}

fn print_diagram_with_boundary(diag: &crate::core::diagram::Diagram, complex: &Complex, display: &Display) {
    let d = diag.top_dim();
    let k = d - 1;
    match (
        crate::core::diagram::Diagram::boundary(Sign::Input, k, diag),
        crate::core::diagram::Diagram::boundary(Sign::Output, k, diag),
    ) {
        (Ok(src), Ok(tgt)) => display.inspect_rich(&format!(
            "  : {} -> {}",
            display.code(&render_diagram(&src, complex)),
            display.code(&render_diagram(&tgt, complex)),
        )),
        _ => display.inspect_rich("  (Boundary extraction failed)"),
    }
}

/// Dispatch all commands that require an active engine.
// ── Hole-filling ────────────────────────────────────────────────────────────

/// List the open holes of maps in the current module, numbered for `fill`.
fn dispatch_holes(store: &GlobalStore, canonical_path: &str, display: &Display) {
    let holes = list_open_holes(store, canonical_path);
    if holes.is_empty() {
        display.meta("No open holes");
        return;
    }
    display.meta("Open holes:");
    for h in &holes {
        display.inspect_rich(&format!("  {} @{} {} :: {}",
            display.acc(&format!("({})", h.index)), h.type_name, h.map_name, h.domain_name));
        display.inspect_rich(&format!("      {}", display.code(&h.boundary)));
    }
}

/// Announce a freshly started fill: the boundaries to bridge (rewrite) or the
/// candidate 0-cells to choose from (boundaryless).
fn announce_fill(ctx: &FillContext, session: &FillSession, display: &Display) {
    display.meta(&format!("Filling {}", ctx.boundary));
    show_fill_state(ctx, session, display);
}

/// Report the state of an active fill (delegating to the engine for rewrites).
fn show_fill_state(_ctx: &FillContext, session: &FillSession, display: &Display) {
    match session {
        FillSession::Rewrite(e) => show_state(e, display),
        FillSession::ZeroCell(zc) => show_zero_cell_state(zc, display),
    }
}

/// Render a 0-cell fill like a session: the candidates while unchosen, or the
/// chosen cell with a target-reached banner once picked (`undo` to re-choose).
fn show_zero_cell_state(zc: &ZeroCellFill, display: &Display) {
    match zc.chosen_name() {
        Some(name) => {
            display.inspect_rich(&format!("current   {}", display.code(name)));
            display.blank();
            display.inspect_rich(&display.ok("✓ Target reached"));
        }
        None => {
            display.inspect_rich("Choose a 0-cell:");
            for (i, (_, name)) in zc.choices.iter().enumerate() {
                display.inspect_rich(&format!("  {} {}", display.acc(&format!("({i})")), display.code(name)));
            }
        }
    }
}

/// Route an in-session command to the active fill.
fn dispatch_fill_cmd(session: &mut FillSession, cmd: Cmd, display: &Display) {
    match session {
        FillSession::Rewrite(e) => dispatch_engine_cmd(e, cmd, display),
        FillSession::ZeroCell(zc) => match cmd {
            Cmd::Apply(ref v) => match zc.choose(v[0]) {
                Ok(()) => {
                    display.meta(&format!("Chose {}", zc.chosen_name().unwrap_or("?")));
                    show_zero_cell_state(zc, display);
                }
                Err(e) => display.error(&e),
            },
            Cmd::Undo(_) | Cmd::UndoAll | Cmd::Restart => match zc.undo() {
                Ok(()) => show_zero_cell_state(zc, display),
                Err(e) => display.error(&e),
            },
            Cmd::Redo(_) => match zc.redo() {
                Ok(()) => show_zero_cell_state(zc, display),
                Err(e) => display.error(&e),
            },
            Cmd::Rules => show_zero_cell_state(zc, display),
            Cmd::Help => print_help(display),
            _ => display.error("In a 0-cell fill use 'apply <n>', 'undo', 'redo', or 'done'"),
        },
    }
}

fn dispatch_engine_cmd(engine: &mut RewriteEngine, cmd: Cmd, display: &Display) {
    match cmd {
        Cmd::Apply(ref choices) => {
            let result = if choices.len() == 1 {
                engine.step(choices[0])
            } else {
                if !engine.parallel() {
                    Err("multi-apply requires parallel mode".to_string())
                } else {
                    engine.step_multi(choices)
                }
            };
            match result {
                Ok(rule) => {
                    display.meta(&format!("Applied {}", rule));
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
                        "Applied {} step{}{}",
                        applied, if applied == 1 { "" } else { "s" }, tail,
                    ));
                    show_state(engine, display);
                }
                Err(e) => display.error(&e),
            }
        }
        Cmd::Random(n) => {
            match engine.random(n) {
                Ok((applied, stop_reason)) => {
                    let tail = stop_reason.map(|r| format!(" ({})", r)).unwrap_or_default();
                    display.meta(&format!(
                        "Applied {} step{}{}",
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
                    display.meta("Reset to source");
                    show_state(engine, display);
                }
                Err(e) => display.error(&e),
            }
        }
        Cmd::Redo(None) => {
            match engine.redo() {
                Ok(()) => show_state(engine, display),
                Err(e) => display.error(&e),
            }
        }
        Cmd::Redo(Some(target)) => {
            match engine.redo_to(target) {
                Ok(()) => show_state(engine, display),
                Err(e) => display.error(&e),
            }
        }
        Cmd::Rules => {
            let n = engine.current_diagram().top_dim();
            dispatch_rules(engine.type_complex(), engine.store(), Some(n + 1), display);
        }
        Cmd::History => {
            let entries: Vec<(Option<Vec<usize>>, &str)> = engine.history()
                .map(|e| (e.choice.clone(), e.rule_name.as_str()))
                .collect();
            print_history(display, engine.initial_diagram(), &entries, engine.type_complex());
        }
        Cmd::Proof => {
            let steps = engine.steps();
            if steps.is_empty() {
                display.meta("(No proof built yet)");
            } else {
                let n = engine.initial_diagram().top_dim();
                let scope = engine.type_complex();
                let ordered: Vec<_> = if engine.backward() {
                    steps.iter().rev().collect()
                } else {
                    steps.iter().collect()
                };
                let proof_expr = ordered.iter()
                    .map(|s| render_diagram(s, scope))
                    .collect::<Vec<_>>()
                    .join(&format!(" #{} ", n));
                let (src, tgt) = if engine.backward() {
                    (render_diagram(engine.current_diagram(), scope),
                     render_diagram(engine.initial_diagram(), scope))
                } else {
                    (render_diagram(engine.initial_diagram(), scope),
                     render_diagram(engine.current_diagram(), scope))
                };
                display.inspect(&format!("{} : {} -> {}", proof_expr, src, tgt));
            }
        }
        Cmd::Parallel(Some(on)) => {
            engine.set_parallel(on);
            display.meta(&format!("Parallel mode {}", if on { "on" } else { "off" }));
        }
        Cmd::Parallel(None) => {
            display.meta(&format!("Parallel mode {}", if engine.parallel() { "on" } else { "off" }));
        }
        Cmd::Help => print_help(display),
        Cmd::Quit => {}   // handled by caller
        // These are all handled before dispatch_engine_cmd is reached
        Cmd::Stop | Cmd::Types | Cmd::PrintFile | Cmd::Type(_) | Cmd::Homology(_)
        | Cmd::Status | Cmd::Start(..) | Cmd::Resume(..) | Cmd::Backward(_)
        | Cmd::Store(_) | Cmd::Save(_) | Cmd::Holes | Cmd::Fill(_) | Cmd::Done
        | Cmd::Unknown(_) | Cmd::UsageError(_) => unreachable!(),
    }
}

fn print_help(display: &Display) {
    // Command token in the code colour, left-justified in a 20-column field
    // (measured on the plain token, since painting changes byte length), then
    // the description.
    let cmd = |tok: &str, desc: &str| {
        let pad = " ".repeat(20usize.saturating_sub(tok.len()));
        display.inspect_rich(&format!("  {}{}{}", display.code(tok), pad, desc));
    };

    display.inspect_rich("Always available:");
    cmd("types",            "List all types in the file");
    cmd("type <name>",      "Inspect a type: generators, diagrams, maps");
    cmd("homology <name>",  "Compute cellular homology of a type");
    cmd("start <t> <s>",    "Start a rewrite session (target optional)");
    cmd("resume <t> <p>",   "Resume a session from a diagram (target optional)");
    cmd("holes",            "List open holes of maps in this module");
    cmd("fill <n>",         "Start a hole-filling session for hole <n>");
    cmd("backward [on|off]", "Show or toggle backward rewrite mode (default: off)");
    cmd("status / show",    "Session state, or module info when idle");
    cmd("print",            "Print the running source");
    cmd("save <path>",      "Write the running source to disk");
    cmd("stop",             "End the active session");
    cmd("help / ?",         "Show this help");
    cmd("quit / exit / q",  "Exit");
    display.blank();
    display.inspect_rich("Session commands (require active session):");
    cmd("apply <n> [<n2>..]", "Apply rewrite(s) at given indices (alias: a)");
    cmd("auto <n>",         "Apply up to <n> rewrites automatically");
    cmd("random <n>",       "Apply randomly selected rewrites");
    cmd("parallel [on|off]", "Show or toggle parallel rewrite mode (default: on)");
    cmd("undo [<n>]",       "Undo the last step, or back to step <n> (alias: u)");
    cmd("redo [<n>]",       "Redo the last undone step, or forward to step <n>");
    cmd("undo all / restart", "Reset to the source diagram");
    cmd("rules",            "List rewrite rules at current dimension (alias: r)");
    cmd("history",          "Show the move history (alias: h)");
    cmd("proof",            "Show the running proof diagram (alias: p)");
    cmd("store <name>",     "Store the current proof as a named diagram in the source");
    cmd("done",             "Finalise the active fill, extending the map");
}

// ── Command parsing ───────────────────────────────────────────────────────────

/// A parsed REPL command.
enum Cmd {
    /// `types` — list all types in the file.
    Types,
    /// `status` / `show` — show rewrite state when engine active, module path otherwise.
    Status,
    /// `print` — print the full source file.
    PrintFile,
    /// `type <name>` — inspect a type and its generators.
    Type(String),
    /// `start <type> <source> [<target>]` — start a rewrite session.
    Start(String, String, Option<String>),
    /// `resume <type> <proof> [<target>]` — resume a session from a diagram.
    Resume(String, String, Option<String>),
    /// `holes` — list the open holes of maps in the current module.
    Holes,
    /// `fill <n>` — start a hole-filling session for the n-th open hole.
    Fill(usize),
    /// `done` — finalize the active fill, extending the map's definition.
    Done,
    /// `apply <n> [<n2> ...]` — apply one or more candidate rewrites.
    Apply(Vec<usize>),
    /// `auto <n>` — apply up to `n` rewrites automatically, always picking the
    /// first available candidate each step.
    Auto(usize),
    /// `random` — apply one randomly selected candidate rewrite.
    Random(usize),
    /// `undo [<n>]` — undo the last step, or undo back to step n.
    Undo(Option<usize>),
    /// `undo all` — undo all steps.
    UndoAll,
    /// `redo [<n>]` — redo the last undone step, or redo forward to step n.
    Redo(Option<usize>),
    /// `restart` — alias for `undo all`.
    Restart,
    /// `stop` — destroy engine and type selection, return to no-session mode.
    Stop,
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
    /// `homology <name>` — compute cellular homology of a type.
    Homology(String),
    /// `parallel [on|off]` — show or toggle parallel rewrite mode.
    Parallel(Option<bool>),
    /// `backward [on|off]` — show or toggle backward rewrite mode (pre-session).
    Backward(Option<bool>),
    Help,
    Quit,
    /// Unrecognised command word.
    Unknown(String),
    /// Recognised command with wrong arguments.
    UsageError(String),
}

fn split_quoted_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut chars = s.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek() == Some(&' ') { chars.next(); }
        if chars.peek().is_none() { break; }
        let quote = match chars.peek() {
            Some(&q @ '\'' | &q @ '"') => { chars.next(); Some(q) }
            _ => None,
        };
        let mut tok = String::new();
        loop {
            match chars.peek() {
                None => break,
                Some(&c) if quote == Some(c) => { chars.next(); break; }
                Some(&c) if quote.is_none() && c.is_whitespace() => break,
                _ => tok.push(chars.next().unwrap()),
            }
        }
        if !tok.is_empty() { args.push(tok); }
    }
    args
}

fn parse_command(line: &str) -> Cmd {
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
        "start" => {
            let args = split_quoted_args(rest);
            match args.len() {
                2 => Cmd::Start(args[0].clone(), args[1].clone(), None),
                3 => Cmd::Start(args[0].clone(), args[1].clone(), Some(args[2].clone())),
                _ => Cmd::UsageError("start <type> <source> [<target>]".to_owned()),
            }
        }
        "resume" => {
            let args = split_quoted_args(rest);
            match args.len() {
                2 => Cmd::Resume(args[0].clone(), args[1].clone(), None),
                3 => Cmd::Resume(args[0].clone(), args[1].clone(), Some(args[2].clone())),
                _ => Cmd::UsageError("resume <type> <proof> [<target>]".to_owned()),
            }
        }
        "apply" | "a" => {
            let nums: Result<Vec<usize>, _> = rest.split_whitespace()
                .map(|s| s.parse::<usize>())
                .collect();
            match nums {
                Ok(v) if !v.is_empty() => Cmd::Apply(v),
                _ => Cmd::UsageError("apply <n> [<n2> ...]".to_owned()),
            }
        }
        "auto" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Auto(n),
                Err(_) => Cmd::UsageError("auto <n>".to_owned()),
            }
        }
        "random" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Random(n),
                Err(_) => Cmd::UsageError("random".to_owned()),
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
        "redo" => {
            if rest.is_empty() {
                Cmd::Redo(None)
            } else {
                match rest.parse::<usize>() {
                    Ok(n) => Cmd::Redo(Some(n)),
                    Err(_) => Cmd::UsageError("redo  |  redo <n>".to_owned()),
                }
            }
        }
        "holes" => Cmd::Holes,
        "fill" => {
            match rest.parse::<usize>() {
                Ok(n) => Cmd::Fill(n),
                _ => Cmd::UsageError("fill <n>".to_owned()),
            }
        }
        "done" => Cmd::Done,
        "restart" => Cmd::Restart,
        "stop"    => Cmd::Stop,
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
        "parallel" => {
            match rest {
                "on" => Cmd::Parallel(Some(true)),
                "off" => Cmd::Parallel(Some(false)),
                "" => Cmd::Parallel(None),
                _ => Cmd::UsageError("parallel [on|off]".to_owned()),
            }
        }
        "backward" => {
            match rest {
                "on" => Cmd::Backward(Some(true)),
                "off" => Cmd::Backward(Some(false)),
                "" => Cmd::Backward(None),
                _ => Cmd::UsageError("backward [on|off]".to_owned()),
            }
        }
        "help" | "?" => Cmd::Help,
        "quit" | "exit" | "q" => Cmd::Quit,
        other => Cmd::Unknown(other.to_owned()),
    }
}
