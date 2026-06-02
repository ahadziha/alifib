//! Terminal display abstraction for the interactive REPL.
//!
//! All human-facing output flows through a single [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::io::IsTerminal;

// ── Colour palette ────────────────────────────────────────────────────────────
// Colour is reserved for semantic information only, mirroring syntax
// highlighting: alifib expressions are one colour, the active redex stands out,
// and success/error/prompt each have a role.  Everything else — labels,
// headers, indices, connectives — is left in the default foreground.
// 16-colour ANSI for portability; swap a line for a 24-bit code to retheme.

const C_CODE:   &str = "\x1b[33m";    // yellow:      alifib expressions (the "syntax" colour)
const C_REDEX:  &str = "\x1b[1;33m";  // bold yellow: the matched redex within a rewrite
const C_OK:     &str = "\x1b[32m";    // green:       success
const C_ERR:    &str = "\x1b[31m";    // red:         errors
const C_PROMPT: &str = "\x1b[35m";    // magenta:     the input prompt marker

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

    /// Print a meta-level line.
    ///
    /// The body is left in the default colour so callers add emphasis with
    /// [`hi`](Self::hi)/[`acc`](Self::acc)/[`sec`](Self::sec).  Output is
    /// unprefixed — only user input carries the `❯` prompt.
    pub fn meta(&self, text: &str) {
        for line in text.split('\n') {
            println!("{line}");
        }
    }

    /// Print an alifib expression / inspection line in the code colour.
    ///
    /// Used by `print cell` and `print type` output.
    pub fn inspect(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_CODE}{line}{RESET}");
            } else {
                println!("{line}");
            }
        }
    }

    /// Print a cell (diagram) line in the code colour, no prefix.
    pub fn cell(&self, text: &str) {
        if self.color {
            println!("{C_CODE}{text}{RESET}");
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
                println!("{C_CODE}{line}{RESET}");
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

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("{code}{s}{RESET}") } else { s.to_string() }
    }

    /// An alifib expression (yellow) — the one "syntax" colour.
    pub fn code(&self, s: &str) -> String { self.paint(C_CODE, s) }
    /// Success (green).
    pub fn ok(&self, s: &str)  -> String { self.paint(C_OK, s) }
    /// The input prompt marker (magenta).
    pub fn acc(&self, s: &str) -> String { self.paint(C_PROMPT, s) }

    /// Render a rewrite candidate: the expression in the code colour, with the
    /// matched redex (the `[…]` segment) in bold.  Brackets do not nest in our
    /// output so a single scan suffices.
    pub fn colorize_match_display(&self, s: &str) -> String {
        if !self.color {
            return s.to_string();
        }
        let mut result = String::with_capacity(s.len() + 32);
        result.push_str(C_CODE);
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '[' {
                result.push_str(C_REDEX);
                result.push('[');
                for ch2 in chars.by_ref() {
                    result.push(ch2);
                    if ch2 == ']' {
                        // End the bold redex, resume the base code colour.
                        result.push_str(RESET);
                        result.push_str(C_CODE);
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
