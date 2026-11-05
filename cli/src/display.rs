//! Terminal display for the REPL, the `do` client, and batch failures.
//!
//! All human-facing output flows through a [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::fmt;
use std::io::IsTerminal;

use alifib::interactive::kernel::{Reply, Status};
use alifib::interactive::richtext::{RichText, Role, Segment};
use alifib::language::error::Diagnostic;

// ── Colour palette ────────────────────────────────────────────────────────────
// Standard 16-colour ANSI, chosen as the closest analogues of the web REPL's
// dark-theme roles, so the two front-ends read alike on any terminal.  Colour is
// reserved for semantic roles, mirroring the web's span classes: bright white
// for values, grey for labels and connectives, yellow for the input/redex side
// of a rewrite, cyan for the output side and section titles, green for success,
// red for errors, magenta for the prompt.  Everything else stays in the default
// foreground.

const C_HI:     &str = "\x1b[97m";    // bright white: values (diagrams, names, counts)
const C_DIM:    &str = "\x1b[90m";    // bright black: labels, secondary, connectives
const C_OK:     &str = "\x1b[32m";    // green:        success
const C_ERR:    &str = "\x1b[31m";    // red:          errors
const C_SRC:    &str = "\x1b[33m";    // yellow:       input side / alifib code
const C_TGT:    &str = "\x1b[36m";    // cyan:         output side / section titles
const C_PROMPT: &str = "\x1b[35m";    // magenta:      the input prompt marker
const C_REDEX:  &str = "\x1b[1;33m";  // bold yellow:  the matched redex within a rewrite

const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

// ── Display ───────────────────────────────────────────────────────────────────

/// Controls all terminal output on one stream.
///
/// When that stream is a terminal, semantic roles and errors are coloured.
/// When it is redirected (pipes, files), colour codes are suppressed so the
/// output is clean plain text.
pub struct Display {
    color: bool,
    stream: Stream,
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

impl Display {
    /// A display on stdout: replies and the REPL.
    pub fn stdout() -> Self {
        Self { color: std::io::stdout().is_terminal(), stream: Stream::Stdout }
    }

    /// A display on stderr: failures of a batch run.
    pub fn stderr() -> Self {
        Self { color: std::io::stderr().is_terminal(), stream: Stream::Stderr }
    }

    fn print(&self, text: fmt::Arguments<'_>) {
        match self.stream {
            Stream::Stdout => print!("{text}"),
            Stream::Stderr => eprint!("{text}"),
        }
    }

    /// Print a kernel reply: a failure for an error, else its rich text or message.
    pub fn reply(&self, reply: &Reply) {
        match (&reply.status, &reply.rendered, &reply.message) {
            (Status::Error, _, message) => self.failure(message.as_deref(), &reply.diagnostics),
            (Status::Ok, Some(rendered), _) => self.print(format_args!("{}\n", self.style(rendered))),
            (Status::Ok, None, Some(message)) => self.print(format_args!("{message}\n")),
            (Status::Ok, None, None) => {}
        }
    }

    /// Print a failure as its source-bearing diagnostics, or else its message.
    pub fn failure(&self, message: Option<&str>, diagnostics: &[Diagnostic]) {
        if diagnostics.is_empty() {
            if let Some(message) = message {
                self.error(message);
            }
        } else {
            for diagnostic in diagnostics {
                self.print(format_args!("{}", diagnostic.render(self.color)));
            }
        }
    }

    /// Print an error in red — the colour carries the "error" signal, so no
    /// prefix when coloured.  In plain mode an `Error: ` marker stands in.
    pub fn error(&self, text: &str) {
        if self.color {
            self.print(format_args!("{C_ERR}{text}{RESET}\n"));
        } else {
            self.print(format_args!("Error: {text}\n"));
        }
    }

    // ── Inline painting ──────────────────────────────────────────────────────

    /// Wrap `s` in `code`…reset (or return it unchanged when colour is off).
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("{code}{s}{RESET}") } else { s.to_string() }
    }

    /// The input prompt marker (magenta).
    pub fn acc(&self, s: &str) -> String { self.paint(C_PROMPT, s) }

    /// Style a [`RichText`] to a terminal string: each segment painted by its
    /// role, lines joined by newlines.  This is the CLI half of the shared
    /// renderer — the web styles the same `RichText` with CSS spans — so the
    /// role→colour table lives here and nowhere else.
    fn style(&self, rt: &RichText) -> String {
        if !self.color {
            return rt.to_string();
        }
        rt.lines.iter()
            .map(|line| line.iter().map(|seg| self.style_segment(seg)).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn style_segment(&self, seg: &Segment) -> String {
        match seg.role {
            Role::Plain => seg.text.clone(),
            Role::Label => self.paint(C_DIM, &seg.text),
            Role::Value => self.paint(C_HI, &seg.text),
            Role::Src => self.paint(C_SRC, &seg.text),
            Role::Tgt => self.paint(C_TGT, &seg.text),
            Role::Ok => self.paint(C_OK, &seg.text),
            Role::Section => format!("{BOLD}{C_TGT}{}{RESET}", seg.text),
            // Plain-text redex brackets are supplied by RichText's Display.
            Role::Redex => self.paint(C_REDEX, &seg.text),
        }
    }

    /// Print a blank line.
    pub fn blank(&self) {
        self.print(format_args!("\n"));
    }
}
