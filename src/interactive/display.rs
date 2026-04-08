//! Terminal display abstraction for the interactive REPL.
//!
//! All human-facing output flows through a single [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::io::IsTerminal;

// ── Colour palette ────────────────────────────────────────────────────────────
// Change these to retheme the whole REPL output at once.

/// Colour for REPL meta lines (`>> ...`): responses, status, info.
const COLOR_META: &str  = "\x1b[32m";  // green

/// Colour for cell/type inspection output (`>> ...` from print commands).
const COLOR_CELL: &str  = "\x1b[33m";  // yellow

/// Colour for source file display (`<< ...`).
const COLOR_FILE: &str  = "\x1b[33m";  // yellow

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

    /// Print a meta-level line: `>> text` in green.
    ///
    /// If `text` contains newlines, each line is prefixed with `>> `.
    pub fn meta(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{COLOR_META}>>{RESET} {COLOR_META}{line}{RESET}");
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
                println!("{COLOR_CELL}>>{RESET} {COLOR_CELL}{line}{RESET}");
            } else {
                println!(">> {line}");
            }
        }
    }

    /// Print a cell (diagram) line — yellow, no prefix.
    pub fn cell(&self, text: &str) {
        if self.color {
            println!("{COLOR_CELL}{text}{RESET}");
        } else {
            println!("{text}");
        }
    }

    /// Print an error: `>> error: text` in green.
    pub fn error(&self, text: &str) {
        if self.color {
            println!("{COLOR_META}>>{RESET} {COLOR_META}error: {text}{RESET}");
        } else {
            println!(">> error: {text}");
        }
    }

    /// Print file source: yellow, no prefix.
    pub fn file(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{COLOR_FILE}{line}{RESET}");
            } else {
                println!("{line}");
            }
        }
    }

    /// Print a blank line.
    pub fn blank(&self) {
        println!();
    }
}
