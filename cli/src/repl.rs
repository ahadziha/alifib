//! Readline adapter for the shared interactive kernel.
//!
//! Every line is evaluated by [`Kernel`], except whole-line `quit`, `exit`,
//! and `q`, which exit the terminal. Replies use [`Display::reply`].

use std::borrow::Cow;
use std::process::ExitCode;

use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::history::FileHistory;
use rustyline::{EditMode, Editor};

use super::display::Display;
use alifib::interactive::kernel::{FsAccess, Kernel, Status};

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

/// Whether a whole line leaves the REPL instead of reaching the kernel.
fn is_quit(line: &str) -> bool {
    matches!(line.trim(), "quit" | "exit" | "q")
}

fn evaluate(kernel: &mut Kernel, display: &Display, line: &str) {
    for reply in kernel.eval(line) {
        display.reply(&reply);
    }
}

/// Run the interactive REPL, loading a file before evaluating an optional line.
/// `emacs_mode` selects Emacs keybindings; the default is vi mode.
pub fn run(source_file: &str, line: Option<&str>, emacs_mode: bool) -> ExitCode {
    let display = Display::stdout();
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    let loaded = kernel.open(source_file);
    display.reply(&loaded);
    if loaded.status == Status::Error {
        return ExitCode::FAILURE;
    }

    if let Some(line) = line {
        if is_quit(line) {
            return ExitCode::SUCCESS;
        }
        evaluate(&mut kernel, &display, line);
    }

    let mut rl = match make_editor(emacs_mode, &display) {
        Ok(rl) => rl,
        Err(e) => {
            display.error(&format!("could not start the line editor: {e}"));
            return ExitCode::FAILURE;
        }
    };
    loop {
        match rl.readline("❯ ") {
            Err(ReadlineError::Eof | ReadlineError::Interrupted) => break,
            Err(e) => { display.error(&format!("Read error: {e}")); break; }
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() { continue; }
                rl.add_history_entry(line).ok();
                if is_quit(line) { break; }
                evaluate(&mut kernel, &display, line);
            }
        }
    }

    display.blank();
    ExitCode::SUCCESS
}

// ── Editor ──────────────────────────────────────────────────────────────────────

/// Build the readline editor with a coloured prompt.
///
/// rustyline measures prompt width from the string passed to `readline`, so we
/// pass the plain `❯ ` there and let [`ReplHelper`] substitute the coloured form
/// at render time — keeping the cursor correctly positioned.
fn make_editor(emacs_mode: bool, display: &Display) -> rustyline::Result<ReplEditor> {
    let mut rl = ReplEditor::new()?;
    rl.set_edit_mode(if emacs_mode { EditMode::Emacs } else { EditMode::Vi });
    rl.set_helper(Some(ReplHelper { prompt: display.acc("❯ ") }));
    Ok(rl)
}
