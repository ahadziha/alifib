//! In-process interactive REPL for rewrite sessions.
//!
//! The REPL is a thin adapter over the shared [`Session`]: it parses a line into
//! a [`Request`], calls [`Session::apply`], and renders the resulting
//! [`ResponseData`] in the web front-end's textual style (see [`super::render`]).
//! All command semantics, state transitions, and canonical messages therefore
//! live in `Session`, shared verbatim with the stdio daemon and the web REPL —
//! a new command lands on all three at once.
//!
//! The only genuinely CLI-local concerns are the read-only queries served
//! straight from the loaded store (`types`/`type`/`homology`), the front-end
//! commands (`print`/`help`/`status`/`quit`), and the final byte-rendering via
//! [`Display`].  Readline (vi or emacs mode) is provided by `rustyline`.
//!
//! # Commands
//!
//! Always available:
//! ```text
//! types            List all types in the file
//! type <name>      Inspect a type: generators, diagrams, maps
//! homology <name>  Compute cellular homology of a type
//! start <t> <s> [<g>]  Start a rewrite session (target optional)
//! resume <t> <p> [<g>] Resume a session from a diagram
//! holes            List open holes of maps in this module
//! fill <n>         Start a hole-filling session for hole <n>
//! backward [on|off] Show or toggle backward rewrite mode
//! status / show    Session state, or module path when idle
//! print            Print the running source
//! save <path>      Write the running source to disk
//! stop             End the active session
//! help / ?         Show command list
//! quit / exit / q  Exit
//! ```
//!
//! Require an active session:
//! ```text
//! apply <n> [<n2>..]  Apply rewrite(s) at given indices (alias: a)
//! auto <n>         Apply up to <n> rewrites automatically
//! random <n>       Apply randomly selected rewrites
//! parallel [on|off] Show or toggle parallel rewrite mode
//! undo [<n>]       Undo the last step, or back to step <n> (alias: u)
//! redo [<n>]       Redo the last undone step, or forward to step <n>
//! undo all / restart  Reset to the source diagram
//! rules            List rewrite rules at current dimension (alias: r)
//! history          Show the move history (alias: h)
//! proof            Show the running proof diagram (alias: p)
//! store <name>     Store the current proof as a named diagram
//! done             Finalise the active fill, extending the map
//! ```

use std::borrow::Cow;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::history::FileHistory;
use rustyline::{EditMode, Editor};

use crate::analysis::homology::compute_homology;
use super::display::Display;
use super::engine::resolve_type;
use super::protocol::{build_type_detail_from_store, build_types_from_store, Request, ResponseData};
use super::render::{
    render_auto, render_history, render_holes, render_homology, render_proof, render_rules,
    render_state, render_store, render_type_detail, render_types, render_zero_cell,
};
use super::session::Session;

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

    let mut session = match Session::from_disk(source_file) {
        Ok(s) => s,
        Err(e) => { display.error(&e); return Err(()); }
    };

    display.meta(&format!("Loaded {}", source_file));

    let mut rl = make_editor(emacs_mode, &display);

    // Auto-start from CLI flags when type and source are given.
    if let (Some(tn), Some(src)) = (type_name, initial_diagram) {
        let req = Request::Start {
            source_file: session.root_path().to_owned(),
            type_name: tn.to_owned(),
            initial: src.to_owned(),
            target: target_diagram.map(str::to_owned),
            backward: session.backward(),
        };
        dispatch_request(&mut session, req, Render::State, &display);
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
                    if handle_command(parse_command(part), &mut session, source_file, &display) {
                        break 'repl;
                    }
                }
            }
        }
    }

    display.blank();
    Ok(())
}

// ── Command handling ────────────────────────────────────────────────────────────

/// Which renderer turns the resulting [`ResponseData`] into a transcript line.
///
/// Mirrors the web front-end's `renderCommandResult` switch: most commands show
/// the rewrite state; a few have a bespoke list/summary; message-only commands
/// (`stop`/`done`/`save`/`backward`) echo the canonical `data.message`.
enum Render {
    State,
    Auto,
    Rules,
    History,
    Proof,
    Store,
    Holes,
    Message,
}

/// Perform one parsed command.  Returns `true` when the REPL should quit.
fn handle_command(cmd: Cmd, session: &mut Session, source_file: &str, display: &Display) -> bool {
    match cmd {
        Cmd::Quit => return true,
        Cmd::Help => print_help(display),

        // ── Front-end-only commands ───────────────────────────────────────
        Cmd::PrintFile => {
            let src = session.source().trim_end();
            if !src.is_empty() { display.file(src); }
        }
        Cmd::Status => {
            if session.session_active() {
                let data = session.state();
                show(display, render_session_state(display, &data));
            } else {
                display.meta(&format!("Module: {}", source_file));
            }
        }

        // ── Read-only queries, served straight from the loaded store ──────
        // `types`/`type` keep the CLI's own layout (generators by dimension with
        // boundaries, diagrams with `= expr`, maps) rather than the web's terse
        // summary — the one deliberate exception to web-style rendering, shared
        // verbatim with the web REPL.
        Cmd::Types => {
            let types = build_types_from_store(session.store(), session.root_path());
            show(display, render_types(display, &types));
        }
        Cmd::Type(name) => {
            match build_type_detail_from_store(session.store(), session.root_path(), &name) {
                Ok(detail) => show(display, render_type_detail(display, &detail)),
                Err(e) => display.error(&e),
            }
        }
        Cmd::Homology(name) => match resolve_type(session.store(), session.root_path(), &name) {
            Ok(tc) => show(display, render_homology(display, &compute_homology(&tc))),
            Err(e) => display.error(&e),
        },

        // ── Diagnostics ───────────────────────────────────────────────────
        Cmd::Unknown(s) => display.error(&format!("Unrecognised command '{}' — type 'help' for a list", s)),
        Cmd::UsageError(usage) => display.error(&format!("Usage: {}", usage)),

        // ── Parallel reports its mode line, setting it first when given an arg ──
        Cmd::Parallel(set) => {
            if let Some(on) = set {
                match session.apply(Request::Parallel { on }) {
                    Ok(data) => display.meta(&format!("parallel mode: {}", if data.parallel { "on" } else { "off" })),
                    Err(e) => display.error(&e),
                }
            } else if session.session_active() {
                let d = session.state();
                display.meta(&format!("parallel mode: {}", if d.parallel { "on" } else { "off" }));
            } else {
                display.error("No active session");
            }
        }

        // ── Everything else routes through the shared Session ─────────────
        other => {
            let (req, render) = to_request(other, session);
            dispatch_request(session, req, render, display);
        }
    }
    false
}

/// Map a session-bearing [`Cmd`] to its [`Request`] and the renderer for the
/// reply.  `session.backward()` seeds `start`/`resume`/`fill` with the idle
/// backward-mode flag.
fn to_request(cmd: Cmd, session: &Session) -> (Request, Render) {
    let backward = session.backward();
    match cmd {
        Cmd::Start(t, s, g) => (Request::Start {
            source_file: session.root_path().to_owned(),
            type_name: t, initial: s, target: g, backward,
        }, Render::State),
        Cmd::Resume(t, p, g) => (Request::Resume {
            source_file: session.root_path().to_owned(),
            type_name: t, proof: p, target: g, backward,
        }, Render::State),
        Cmd::Holes => (Request::Holes, Render::Holes),
        Cmd::Fill(n) => (Request::Fill { index: n, backward }, Render::State),
        Cmd::Done => (Request::Done, Render::Message),
        Cmd::Apply(v) if v.len() == 1 => (Request::Step { choice: v[0] }, Render::State),
        Cmd::Apply(v) => (Request::StepMulti { choices: v }, Render::State),
        Cmd::Auto(n) => (Request::Auto { max_steps: n }, Render::Auto),
        Cmd::Random(n) => (Request::Random { max_steps: n }, Render::Auto),
        Cmd::Undo(None) => (Request::Undo, Render::State),
        Cmd::Undo(Some(s)) => (Request::UndoTo { step: s }, Render::State),
        Cmd::UndoAll | Cmd::Restart => (Request::UndoTo { step: 0 }, Render::State),
        Cmd::Redo(None) => (Request::Redo, Render::State),
        Cmd::Redo(Some(s)) => (Request::RedoTo { step: s }, Render::State),
        Cmd::Stop => (Request::Stop, Render::Message),
        Cmd::Rules => (Request::ListRules, Render::Rules),
        Cmd::History => (Request::History, Render::History),
        Cmd::Proof => (Request::Proof, Render::Proof),
        Cmd::Store(name) => (Request::Store { name }, Render::Store),
        Cmd::Save(path) => (Request::Save { path: Some(path) }, Render::Message),
        Cmd::Backward(on) => (Request::Backward { on }, Render::Message),
        // The variants above are routed before `to_request`; all front-end and
        // query commands are handled in `handle_command`.
        _ => unreachable!("non-session command reached to_request"),
    }
}

/// Apply a request to the session and render (or report) the reply.
fn dispatch_request(session: &mut Session, req: Request, render: Render, display: &Display) {
    match session.apply(req) {
        Err(e) => display.error(&e),
        Ok(data) => match render {
            Render::Message => if let Some(m) = &data.message { display.meta(m); },
            Render::State => show(display, render_session_state(display, &data)),
            Render::Auto => show(display, render_auto(display, &data)),
            Render::Rules => show(display, render_rules(display, &data.rules)),
            Render::History => show(display, render_history(display, &data)),
            Render::Proof => show(display, render_proof(display, &data)),
            Render::Store => show(display, render_store(display, &data)),
            Render::Holes => show(display, render_holes(display, &data.holes)),
        },
    }
}

/// Render a rewrite state, picking the 0-cell fill view when one is active.
fn render_session_state(display: &Display, data: &ResponseData) -> String {
    match &data.zero_cell {
        Some(zc) => render_zero_cell(display, zc),
        None => render_state(display, data),
    }
}

/// Print a rendered (possibly multi-line, already-coloured) transcript block.
fn show(display: &Display, block: String) {
    display.inspect_rich(&block);
}

// ── Editor ──────────────────────────────────────────────────────────────────────

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
