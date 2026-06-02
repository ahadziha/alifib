//! Terminal display abstraction for the interactive REPL.
//!
//! All human-facing output flows through a single [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::io::IsTerminal;

// ── Colour palette ────────────────────────────────────────────────────────────
// 24-bit truecolor matching the web REPL's dark theme, so the two front-ends
// share one palette.  Colour is reserved for semantic roles, mirroring the web's
// span classes: bright text for values, dim grey for labels and connectives,
// amber for the input/redex side of a rewrite, blue for the output side and
// section titles, green for success, red for errors, violet for the prompt.
// Everything else stays in the default foreground.

const C_HI:     &str = "\x1b[38;2;244;244;245m";  // --text-em:  values (diagrams, names, counts)
const C_DIM:    &str = "\x1b[38;2;113;113;122m";  // --text-dim: labels, secondary, connectives
const C_OK:     &str = "\x1b[38;2;74;222;128m";   // --ok:       success
const C_ERR:    &str = "\x1b[38;2;248;113;113m";  // --err:      errors
const C_SRC:    &str = "\x1b[38;2;251;191;36m";   // --warn:     input side / alifib code
const C_TGT:    &str = "\x1b[38;2;95;168;211m";   // --accent2:  output side / section titles
const C_PROMPT: &str = "\x1b[38;2;124;106;242m";  // --accent:   the input prompt marker
const C_REDEX:  &str = "\x1b[1;38;2;251;191;36m"; // bold amber: the matched redex within a rewrite

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

    /// Print an alifib expression / inspection line in the code colour.
    pub fn inspect(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_SRC}{line}{RESET}");
            } else {
                println!("{line}");
            }
        }
    }

    /// Print a cell (diagram) line in the code colour, no prefix.
    pub fn cell(&self, text: &str) {
        if self.color {
            println!("{C_SRC}{text}{RESET}");
        } else {
            println!("{text}");
        }
    }

    /// Print an error: `error: text` in red.
    pub fn error(&self, text: &str) {
        if self.color {
            println!("{C_ERR}error: {text}{RESET}");
        } else {
            println!("error: {text}");
        }
    }

    /// Print file source in the code colour, no prefix.
    pub fn file(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_SRC}{line}{RESET}");
            } else {
                println!("{line}");
            }
        }
    }

    /// Print an inspection line where content already contains embedded color codes.
    ///
    /// Like [`inspect`](Self::inspect) but does not re-wrap `text` in a colour;
    /// the caller is responsible for any inline colouring (via the painting
    /// helpers below).  Output is unprefixed.
    pub fn inspect_rich(&self, text: &str) {
        for line in text.split('\n') {
            println!("{line}");
        }
    }

    // ── Inline painting helpers ─────────────────────────────────────────────
    // Return a coloured fragment (or `s` unchanged when colour is off) so
    // callers can compose styled lines and print them via `inspect_rich`.
    // The roles mirror the web REPL's span classes one-to-one.

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("{code}{s}{RESET}") } else { s.to_string() }
    }

    /// An alifib expression in the code colour (amber).
    pub fn code(&self, s: &str) -> String { self.paint(C_SRC, s) }
    /// A highlighted value — a diagram, name, or count (bright text).
    pub fn hi(&self, s: &str)  -> String { self.paint(C_HI, s) }
    /// A label, connective, or secondary text (dim grey).
    pub fn dim(&self, s: &str) -> String { self.paint(C_DIM, s) }
    /// Success (green).
    pub fn ok(&self, s: &str)  -> String { self.paint(C_OK, s) }
    /// The input side of a rewrite (amber).
    pub fn src(&self, s: &str) -> String { self.paint(C_SRC, s) }
    /// The output side of a rewrite (blue).
    pub fn tgt(&self, s: &str) -> String { self.paint(C_TGT, s) }
    /// A section title (bold blue).
    pub fn sec(&self, s: &str) -> String {
        if self.color { format!("{BOLD}{C_TGT}{s}{RESET}") } else { s.to_string() }
    }
    /// The input prompt marker (violet).
    pub fn acc(&self, s: &str) -> String { self.paint(C_PROMPT, s) }

    /// Render a rewrite candidate's match: the expression in the dim colour, with
    /// the matched redex (the `[…]` segment) in bold amber.  Brackets are kept so
    /// the redex stays legible without colour; they do not nest in our output, so
    /// a single scan suffices.
    pub fn colorize_match_display(&self, s: &str) -> String {
        if !self.color {
            return s.to_string();
        }
        let mut result = String::with_capacity(s.len() + 32);
        result.push_str(C_DIM);
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '[' {
                result.push_str(C_REDEX);
                result.push('[');
                for ch2 in chars.by_ref() {
                    result.push(ch2);
                    if ch2 == ']' {
                        // End the bold redex, resume the dim base colour.
                        result.push_str(RESET);
                        result.push_str(C_DIM);
                        break;
                    }
                }
            } else {
                result.push(ch);
            }
        }
        result.push_str(RESET);
        result
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
