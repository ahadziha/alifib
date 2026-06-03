//! Terminal display abstraction for the interactive REPL.
//!
//! All human-facing output flows through a single [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::io::IsTerminal;

use super::richtext::{RichText, Role, Segment};

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

/// Controls all terminal output for the REPL.
///
/// When stdout is a terminal, meta-level lines are coloured and prefixed.
/// When stdout is redirected (pipes, files), colour codes are suppressed so
/// the output is clean plain text.
pub struct Display {
    color: bool,
}

impl Display {
    /// Create a display that auto-detects whether to emit colour.
    pub fn new() -> Self {
        Self { color: std::io::stdout().is_terminal() }
    }

    /// Create a display that never emits colour — for tests and non-terminal
    /// rendering where deterministic plain text is wanted.
    pub fn plain() -> Self {
        Self { color: false }
    }

    /// Print a meta-level line.
    ///
    /// The body is left in the default colour so callers add emphasis with the
    /// painting helpers.  Output is unprefixed — only user input carries the
    /// `❯` prompt.
    pub fn meta(&self, text: &str) {
        for line in text.split('\n') {
            println!("{line}");
        }
    }

    /// Print an error in red — the colour carries the "error" signal, so no
    /// prefix when coloured.  In plain mode an `Error: ` marker stands in.
    pub fn error(&self, text: &str) {
        if self.color {
            println!("{C_ERR}{text}{RESET}");
        } else {
            println!("Error: {text}");
        }
    }

    /// Print file source in the bright value colour, no prefix.
    pub fn file(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_HI}{line}{RESET}");
            } else {
                println!("{line}");
            }
        }
    }

    /// Print already-styled content verbatim (it may contain embedded colour
    /// codes from [`style`](Self::style)); splits on newlines, no prefix.
    pub fn inspect_rich(&self, text: &str) {
        for line in text.split('\n') {
            println!("{line}");
        }
    }

    // ── Inline painting ──────────────────────────────────────────────────────

    /// Wrap `s` in `code`…reset (or return it unchanged when colour is off).
    fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("{code}{s}{RESET}") } else { s.to_string() }
    }

    /// The input prompt marker (violet).
    pub fn acc(&self, s: &str) -> String { self.paint(C_PROMPT, s) }

    /// Style a [`RichText`] to a terminal string: each segment painted by its
    /// role, lines joined by newlines.  This is the CLI half of the shared
    /// renderer — the web styles the same `RichText` with CSS spans — so the
    /// role→colour table lives here and nowhere else.
    pub fn style(&self, rt: &RichText) -> String {
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
            // Section titles are bold; the matched redex is bold amber when
            // coloured, else `[bracketed]` so it stays legible in plain text.
            Role::Section => if self.color { format!("{BOLD}{C_TGT}{}{RESET}", seg.text) } else { seg.text.clone() },
            Role::Redex => if self.color { self.paint(C_REDEX, &seg.text) } else { format!("[{}]", seg.text) },
        }
    }

    /// Print a blank line.
    pub fn blank(&self) {
        println!();
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}
