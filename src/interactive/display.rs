//! Terminal display abstraction for the interactive REPL.
//!
//! All human-facing output flows through a single [`Display`] value.
//! ANSI escape codes are defined here and **nowhere else** in the codebase.

use std::io::IsTerminal;

// ── The only ANSI codes in the codebase ──────────────────────────────────────

const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

// ── Display ───────────────────────────────────────────────────────────────────

/// Controls all terminal output for the REPL.
///
/// When stdout is a terminal, meta-level lines are coloured green and
/// prefixed with `>> `.  When stdout is redirected (pipes, files), colour
/// codes are suppressed so the output is clean plain text.
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
    /// If `text` contains newlines, each non-empty line is prefixed with `>> `.
    pub fn meta(&self, text: &str) {
        for line in text.split('\n') {
            if self.color {
                println!("{GREEN}>>{RESET} {GREEN}{line}{RESET}");
            } else {
                println!(">> {line}");
            }
        }
    }

    /// Print a cell (diagram) line — plain text, no prefix.
    pub fn cell(&self, text: &str) {
        println!("{text}");
    }

    /// Print an error: `>> error: text` in green.
    pub fn error(&self, text: &str) {
        if self.color {
            println!("{GREEN}>>{RESET} {GREEN}error: {text}{RESET}");
        } else {
            println!(">> error: {text}");
        }
    }

    /// Print a blank line.
    pub fn blank(&self) {
        println!();
    }
}
