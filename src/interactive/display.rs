//! Terminal display abstraction for the interactive REPL.
//!
//! All human-facing output flows through a single [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::io::IsTerminal;

// ── Colour palette ────────────────────────────────────────────────────────────
// Semantic roles mirror web/frontend/style.css.  16-colour ANSI for portability;
// swap any single line to a 24-bit code (`\x1b[38;2;R;G;Bm`) to retheme.

const C_DIM:    &str = "\x1b[90m";    // text-dim   grey (bright black): chrome, secondary text
const C_EM:     &str = "\x1b[1m";     // text-em    bold (inherits fg → theme-safe): emphasis
const C_ACCENT: &str = "\x1b[35m";    // accent     magenta: prompt, rewrite indices
const C_SEC:    &str = "\x1b[1;36m";  // accent2    bold cyan: section titles
const C_TGT:    &str = "\x1b[1;36m";  // accent2    bold cyan: rewrite target
const C_SRC:    &str = "\x1b[1;33m";  // warn       bold amber: matched source pattern
const C_OK:     &str = "\x1b[32m";    // ok         green: success
const C_ERR:    &str = "\x1b[31m";    // err        red: errors
const C_CELL:   &str = "\x1b[33m";    // yellow: cell/type inspection and file body

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

    /// Print a meta-level line: `>> text`.
    ///
    /// The `>>` prefix is dim chrome; the body is left in the default colour so
    /// callers add emphasis with [`hi`](Self::hi)/[`acc`](Self::acc)/[`sec`](Self::sec).
    /// If `text` contains newlines, each line is prefixed with `>> `.
    pub fn meta(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_DIM}>>{RESET} {line}");
            } else {
                println!(">> {line}");
            }
        }
    }

    /// Print a cell/type inspection line: `>> text` in yellow.
    ///
    /// Used by `print cell` and `print type` output.
    pub fn inspect(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_CELL}>>{RESET} {C_CELL}{line}{RESET}");
            } else {
                println!(">> {line}");
            }
        }
    }

    /// Print a cell (diagram) line — yellow, no prefix.
    pub fn cell(&self, text: &str) {
        if self.color {
            println!("{C_CELL}{text}{RESET}");
        } else {
            println!("{text}");
        }
    }

    /// Print an error: `>> error: text` in red.
    pub fn error(&self, text: &str) {
        if self.color {
            println!("{C_DIM}>>{RESET} {C_ERR}error: {text}{RESET}");
        } else {
            println!(">> error: {text}");
        }
    }

    /// Print file source: yellow, no prefix.
    pub fn file(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_CELL}{line}{RESET}");
            } else {
                println!("{line}");
            }
        }
    }

    /// Print an inspection line where content already contains embedded color codes.
    ///
    /// Like [`inspect`](Self::inspect) but does not re-wrap `text` in a colour;
    /// the caller is responsible for any inline colouring (via the painting
    /// helpers below). The `>> ` prefix is still dim chrome.
    pub fn inspect_rich(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{C_DIM}>>{RESET} {line}");
            } else {
                println!(">> {line}");
            }
        }
    }

    // ── Inline painting helpers ─────────────────────────────────────────────
    // Return a coloured fragment (or `s` unchanged when colour is off) so
    // callers can compose styled lines and print them via `inspect_rich`.

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color { format!("{code}{s}{RESET}") } else { s.to_string() }
    }

    /// Bold emphasis (diagram labels, rule names).
    pub fn hi(&self, s: &str)  -> String { self.paint(C_EM, s) }
    /// Dim secondary text (step numbers, separators, hints).
    pub fn dim(&self, s: &str) -> String { self.paint(C_DIM, s) }
    /// Section title (bold cyan).
    pub fn sec(&self, s: &str) -> String { self.paint(C_SEC, s) }
    /// Success (green).
    pub fn ok(&self, s: &str)  -> String { self.paint(C_OK, s) }
    /// Accent (magenta): prompt, rewrite indices.
    pub fn acc(&self, s: &str) -> String { self.paint(C_ACCENT, s) }

    /// Wrap `s` in the matched-source colour (bold amber) when colour is enabled.
    pub fn paint_source(&self, s: &str) -> String { self.paint(C_SRC, s) }

    /// Wrap `s` in the rewrite-target colour (bold cyan) when colour is enabled.
    pub fn paint_target(&self, s: &str) -> String { self.paint(C_TGT, s) }

    /// Colorize the `[matched]` segment in a match-display string.
    ///
    /// Everything inside the outermost `[...]` is painted in the source colour;
    /// the surrounding context is left unstyled.  Brackets do not nest in our
    /// output so a simple scan is sufficient.
    pub fn colorize_match_display(&self, s: &str) -> String {
        if !self.color {
            return s.to_string();
        }
        let mut result = String::with_capacity(s.len() + 32);
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '[' {
                result.push_str(C_SRC);
                result.push('[');
                for ch2 in chars.by_ref() {
                    result.push(ch2);
                    if ch2 == ']' {
                        result.push_str(RESET);
                        break;
                    }
                }
            } else {
                result.push(ch);
            }
        }
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
