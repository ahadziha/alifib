//! The REPL command language, parsed once and shared by both front-ends.
//!
//! A typed line becomes a [`Command`] or a user-facing error string — the *same*
//! parser, error wording, and aliases for the CLI and the web, so neither can
//! drift from the other.  Each front-end then dispatches the [`Command`] in its
//! own medium (the CLI applies it and styles the reply; the web routes it to a
//! backend request or a UI flow), but *what is a valid command, with what
//! arguments, and what the error reads* lives here and nowhere else.
//!
//! A few commands are front-end-specific — `print`/`save`/`quit` (CLI) and
//! `clear` (web) — so [`parse`] takes the [`Frontend`] and treats the others'
//! commands as unknown, exactly as a user would expect.

use super::protocol::Request;

/// Which front-end is parsing — gates the medium-specific commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frontend {
    Cli,
    Web,
}

/// A parsed, validated REPL command.  Error cases (unknown word, bad arguments)
/// are not variants here — [`parse`] returns them as the finished message.
#[derive(Debug, PartialEq)]
pub enum Command {
    // Always available
    Types,
    Type(String),
    Homology(String),
    Holes,
    Show,
    Start { type_name: String, initial: String, target: Option<String> },
    Resume { type_name: String, proof: String, target: Option<String> },
    Fill(usize),
    Backward(Option<bool>),
    Stop,
    Help,
    // Session commands
    Apply(Vec<usize>),
    Auto(usize),
    Random(usize),
    Undo(Option<usize>),
    UndoAll,
    Redo(Option<usize>),
    Restart,
    Rules,
    History,
    Proof,
    Store(String),
    Parallel(Option<bool>),
    Done,
    // Front-end-specific
    PrintFile,         // CLI
    Save(String),      // CLI
    Quit,              // CLI
    Clear,             // web
}

impl Command {
    /// The backend [`Request`] this command issues, where that mapping is the
    /// same in every front-end — the single source both adapters delegate to.
    /// `backward` seeds the rewrite direction of a `fill`.
    ///
    /// `None` for the commands with no such request: `start`/`resume` (which also
    /// need the session's source path, so the adapter builds them), the read-only
    /// `help`, and the front-end-only `print`/`quit`/`clear`.
    pub fn to_request(self, backward: bool) -> Option<Request> {
        use Command::*;
        Some(match self {
            Types => Request::Types,
            Type(name) => Request::TypeInfo { name },
            Homology(name) => Request::Homology { name },
            Show => Request::Show,
            Holes => Request::Holes,
            Fill(index) => Request::Fill { index, backward },
            Done => Request::Done,
            Apply(v) if v.len() == 1 => Request::Step { choice: v[0] },
            Apply(v) => Request::StepMulti { choices: v },
            Auto(n) => Request::Auto { max_steps: n },
            Random(n) => Request::Random { max_steps: n },
            Undo(None) => Request::Undo,
            Undo(Some(s)) => Request::UndoTo { step: s },
            UndoAll | Restart => Request::UndoTo { step: 0 },
            Redo(None) => Request::Redo,
            Redo(Some(s)) => Request::RedoTo { step: s },
            Stop => Request::Stop,
            Rules => Request::ListRules,
            History => Request::History,
            Proof => Request::Proof,
            Store(name) => Request::Store { name },
            Save(path) => Request::Save { path: Some(path) },
            Parallel(on) => Request::Parallel { on },
            Backward(on) => Request::Backward { on },
            Start { .. } | Resume { .. } | Help | PrintFile | Quit | Clear => return None,
        })
    }
}

/// Parse one REPL line for `fe`.  `Ok` is a dispatchable [`Command`]; `Err` is
/// the complete message to show (a `Usage:` line or an unknown-command notice),
/// identical for both front-ends.
pub fn parse(line: &str, fe: Frontend) -> Result<Command, String> {
    let mut parts = line.splitn(2, char::is_whitespace);
    let word = parts.next().unwrap_or("").trim();
    let rest = parts.next().map(str::trim).unwrap_or("");

    let usage = |u: &str| Err(format!("Usage: {}", u));
    let unknown = || Err(format!("Unrecognised command '{}' — type 'help' for a list", word));

    match word {
        "types" | "Types" => Ok(Command::Types),
        "status" | "show" => Ok(Command::Show),
        "type" => {
            if rest.is_empty() { usage("type <name>") } else { Ok(Command::Type(rest.to_owned())) }
        }
        "homology" => {
            if rest.is_empty() { usage("homology <name>") } else { Ok(Command::Homology(rest.to_owned())) }
        }
        "start" => match split_quoted_args(rest).as_slice() {
            [t, s] => Ok(Command::Start { type_name: t.clone(), initial: s.clone(), target: None }),
            [t, s, g] => Ok(Command::Start { type_name: t.clone(), initial: s.clone(), target: Some(g.clone()) }),
            _ => usage("start <type> <source> [<target>]"),
        },
        "resume" => match split_quoted_args(rest).as_slice() {
            [t, p] => Ok(Command::Resume { type_name: t.clone(), proof: p.clone(), target: None }),
            [t, p, g] => Ok(Command::Resume { type_name: t.clone(), proof: p.clone(), target: Some(g.clone()) }),
            _ => usage("resume <type> <proof> [<target>]"),
        },
        "apply" | "a" => {
            let nums: Result<Vec<usize>, _> =
                rest.split_whitespace().map(|s| s.parse::<usize>()).collect();
            match nums {
                Ok(v) if !v.is_empty() => Ok(Command::Apply(v)),
                _ => usage("apply <n> [<n2> ...]"),
            }
        }
        "auto" => rest.parse::<usize>().map(Command::Auto).or_else(|_| usage("auto <n>")),
        "random" => rest.parse::<usize>().map(Command::Random).or_else(|_| usage("random <n>")),
        "undo" | "u" => {
            if rest.is_empty() {
                Ok(Command::Undo(None))
            } else if rest == "all" {
                Ok(Command::UndoAll)
            } else {
                rest.parse::<usize>().map(|n| Command::Undo(Some(n)))
                    .or_else(|_| usage("undo  |  undo <n>  |  undo all"))
            }
        }
        "redo" => {
            if rest.is_empty() {
                Ok(Command::Redo(None))
            } else {
                rest.parse::<usize>().map(|n| Command::Redo(Some(n)))
                    .or_else(|_| usage("redo  |  redo <n>"))
            }
        }
        "holes" => Ok(Command::Holes),
        "fill" => rest.parse::<usize>().map(Command::Fill).or_else(|_| usage("fill <n>")),
        "done" => Ok(Command::Done),
        "restart" => Ok(Command::Restart),
        "stop" => Ok(Command::Stop),
        "rules" | "r" => Ok(Command::Rules),
        "history" | "h" => Ok(Command::History),
        "proof" | "p" => Ok(Command::Proof),
        "store" => {
            if rest.is_empty() { usage("store <name>") } else { Ok(Command::Store(rest.to_owned())) }
        }
        "parallel" => match rest {
            "on" => Ok(Command::Parallel(Some(true))),
            "off" => Ok(Command::Parallel(Some(false))),
            "" => Ok(Command::Parallel(None)),
            _ => usage("parallel [on|off]"),
        },
        "backward" => match rest {
            "on" => Ok(Command::Backward(Some(true))),
            "off" => Ok(Command::Backward(Some(false))),
            "" => Ok(Command::Backward(None)),
            _ => usage("backward [on|off]"),
        },
        "help" | "?" => Ok(Command::Help),
        // ── Front-end-specific ──
        "print" if fe == Frontend::Cli => {
            if rest.is_empty() { Ok(Command::PrintFile) } else { usage("print") }
        }
        "save" if fe == Frontend::Cli => {
            if rest.is_empty() { usage("save <path>") } else { Ok(Command::Save(rest.to_owned())) }
        }
        "quit" | "exit" | "q" if fe == Frontend::Cli => Ok(Command::Quit),
        "clear" if fe == Frontend::Web => Ok(Command::Clear),
        _ => unknown(),
    }
}

/// Split `s` into whitespace-separated tokens, honouring single/double quotes so
/// a diagram expression with spaces stays one argument.
pub fn split_quoted_args(s: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_args_parse_the_same_for_both_front_ends() {
        for fe in [Frontend::Cli, Frontend::Web] {
            assert_eq!(parse("a 0 1", fe), Ok(Command::Apply(vec![0, 1])));
            assert_eq!(parse("undo all", fe), Ok(Command::UndoAll));
            assert_eq!(parse("p", fe), Ok(Command::Proof));
            assert_eq!(parse("parallel on", fe), Ok(Command::Parallel(Some(true))));
            assert_eq!(parse("backward", fe), Ok(Command::Backward(None)));
            assert_eq!(
                parse("start C a b", fe),
                Ok(Command::Start { type_name: "C".into(), initial: "a".into(), target: Some("b".into()) })
            );
        }
    }

    #[test]
    fn errors_are_identical_strings() {
        assert_eq!(parse("type", Frontend::Cli), Err("Usage: type <name>".into()));
        assert_eq!(parse("type", Frontend::Web), Err("Usage: type <name>".into()));
        assert_eq!(
            parse("frobnicate", Frontend::Cli),
            Err("Unrecognised command 'frobnicate' — type 'help' for a list".into())
        );
        assert_eq!(parse("frobnicate", Frontend::Web), parse("frobnicate", Frontend::Cli));
    }

    #[test]
    fn medium_specific_commands_are_gated() {
        // `clear` is the web's; the CLI does not know it.
        assert_eq!(parse("clear", Frontend::Web), Ok(Command::Clear));
        assert!(parse("clear", Frontend::Cli).is_err());
        // `print`/`save`/`quit` are the CLI's; the web does not know them.
        assert_eq!(parse("print", Frontend::Cli), Ok(Command::PrintFile));
        assert!(parse("print", Frontend::Web).is_err());
        assert!(parse("quit", Frontend::Web).is_err());
    }
}
