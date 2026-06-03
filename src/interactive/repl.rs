//! In-process interactive REPL for rewrite sessions.
//!
//! The REPL is a thin adapter over the shared [`Session`]: it parses a line into
//! a [`Request`], calls [`Session::apply`], turns the resulting [`ResponseData`]
//! into a [`RichText`](super::richtext::RichText) via
//! [`render_response`](super::richtext::render_response), and styles it to the
//! terminal with [`Display::style`].  Command semantics, state transitions, and
//! canonical messages live in `Session`; the *layout* lives in `richtext` — both
//! shared verbatim with the stdio daemon and the web REPL, so a new command, and
//! any change to how it reads, lands on all three at once.
//!
//! The only genuinely CLI-local concerns are the read-only queries served
//! straight from the loaded store (`types`/`type`/`homology`), the front-end
//! commands (`print`/`help`/`status`/`quit`), and the final styling via
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
//! undo all / restart  Reset to the initial diagram
//! rules            List rewrite rules at current dimension (alias: r)
//! history          Show the move history (alias: h)
//! proof            Show the running proof diagram (alias: p)
//! store <name>     Store the current proof as a named diagram
//! done             Finalise the hole-filling session
//! ```

use std::borrow::Cow;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::history::FileHistory;
use rustyline::{EditMode, Editor};

use super::command::{parse, Command, Frontend};
use super::display::Display;
use super::protocol::{
    build_homology_data, build_type_detail_from_store, build_types_from_store, Request, ResponseData,
};
use super::richtext::{help, render_kind_for, render_response, RenderKind};
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
        dispatch_request(&mut session, req, &display);
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
                    let quit = match parse(part, Frontend::Cli) {
                        Ok(cmd) => handle_command(cmd, &mut session, &display),
                        Err(msg) => { display.error(&msg); false }
                    };
                    if quit { break 'repl; }
                }
            }
        }
    }

    display.blank();
    Ok(())
}

// ── Command handling ────────────────────────────────────────────────────────────

/// Perform one parsed command.  Returns `true` when the REPL should quit.
fn handle_command(cmd: Command, session: &mut Session, display: &Display) -> bool {
    match cmd {
        Command::Quit => return true,
        Command::Help => display.inspect_rich(&display.style(&help(false))),

        // ── Front-end-only commands ───────────────────────────────────────
        Command::PrintFile => {
            let src = session.source().trim_end();
            if !src.is_empty() { display.file(src); }
        }

        // ── Read-only queries, served straight from the loaded store ──────
        Command::Types => {
            let mut data = ResponseData::empty();
            data.types = build_types_from_store(session.store(), session.root_path());
            show(display, RenderKind::Types, &data);
        }
        Command::Type(name) => {
            match build_type_detail_from_store(session.store(), session.root_path(), &name) {
                Ok(detail) => {
                    let mut data = ResponseData::empty();
                    data.type_detail = Some(detail);
                    show(display, RenderKind::TypeDetail, &data);
                }
                Err(e) => display.error(&e),
            }
        }
        Command::Homology(name) => match build_homology_data(session.store(), session.root_path(), &name) {
            Ok(h) => {
                let mut data = ResponseData::empty();
                data.homology = Some(h);
                show(display, RenderKind::Homology, &data);
            }
            Err(e) => display.error(&e),
        },

        // `clear` is the web's; never parsed for the CLI.
        Command::Clear => {}

        // ── Everything else routes through the shared Session ─────────────
        other => dispatch_request(session, to_request(other, session), display),
    }
    false
}

/// Map a session-bearing [`Command`] to its [`Request`].  `start`/`resume` also
/// need the session's source path, so they are built here; every other mapping
/// is the shared [`Command::to_request`], seeded with the idle backward mode.
fn to_request(cmd: Command, session: &Session) -> Request {
    let backward = session.backward();
    match cmd {
        Command::Start { type_name, initial, target } =>
            Request::Start { source_file: session.root_path().to_owned(), type_name, initial, target, backward },
        Command::Resume { type_name, proof, target } =>
            Request::Resume { source_file: session.root_path().to_owned(), type_name, proof, target, backward },
        other => other.to_request(backward).expect("query/front-end commands are handled before dispatch"),
    }
}

/// Apply a request and render its reply: the shared [`RichText`] view when the
/// request has one, else the canonical `data.message` (`stop`/`done`/`save`/
/// `backward`).
fn dispatch_request(session: &mut Session, req: Request, display: &Display) {
    let kind = render_kind_for(&req);
    match session.apply(req) {
        Err(e) => display.error(&e),
        Ok(data) => match kind {
            Some(k) => show(display, k, &data),
            None => if let Some(m) = &data.message { display.meta(m); },
        },
    }
}

/// Style a response in the requested view and print it.
fn show(display: &Display, kind: RenderKind, data: &ResponseData) {
    display.inspect_rich(&display.style(&render_response(kind, data)));
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

