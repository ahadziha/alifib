//! The command language shared by every front end.
//!
//! A line is scanned whole before any part can run. Strings, nested comments,
//! and brackets protect their contents from the top-level word and part splits.

/// A command answered by the kernel, or delegated to a loaded session.
#[derive(Debug, PartialEq)]
pub enum Input {
    Load(Option<String>),
    Save(Option<String>),
    Help,
    Session(Command),
}

/// A command answered by `Session::apply`, without reaching the disk.
#[derive(Debug, PartialEq)]
pub enum Command {
    Types,
    Type(String),
    Homology(String),
    Holes,
    Show,
    Print,
    Start {
        type_name: String,
        initial: String,
        target: Option<String>,
    },
    Resume {
        type_name: String,
        proof: String,
        target: Option<String>,
    },
    Fill(usize),
    Done,
    Stop,
    Backward(Option<bool>),
    Apply(Vec<usize>),
    Auto(usize),
    Random(usize),
    Undo(Option<usize>),
    Redo(Option<usize>),
    Rules,
    History,
    Proof,
    Store(String),
    Parallel(Option<bool>),
}

impl Command {
    /// Whether running the command may change the session, even if it fails.
    pub fn mutates(&self) -> bool {
        match self {
            Self::Types
            | Self::Type(_)
            | Self::Homology(_)
            | Self::Holes
            | Self::Show
            | Self::Print
            | Self::Rules
            | Self::History
            | Self::Proof
            | Self::Backward(None)
            | Self::Parallel(None) => false,
            Self::Start { .. }
            | Self::Resume { .. }
            | Self::Fill(_)
            | Self::Done
            | Self::Stop
            | Self::Backward(Some(_))
            | Self::Apply(_)
            | Self::Auto(_)
            | Self::Random(_)
            | Self::Undo(_)
            | Self::Redo(_)
            | Self::Store(_)
            | Self::Parallel(Some(_)) => true,
        }
    }
}

impl Input {
    /// Whether running it may change the session and move the revision.
    pub fn mutates(&self) -> bool {
        match self {
            Self::Load(_) => true,
            Self::Session(command) => command.mutates(),
            Self::Save(_) | Self::Help => false,
        }
    }

    /// Mutations and source writes must refer to the state the sender saw.
    pub fn guarded(&self) -> bool {
        self.mutates() || matches!(self, Self::Save(_))
    }
}

/// A whole parsed line, or one balance error that prevents every part running.
#[derive(Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub parts: Result<Vec<Part>, String>,
}

#[derive(Debug, PartialEq)]
pub struct Part {
    pub text: String,
    pub input: Result<Input, String>,
}

impl Line {
    /// Whether any successfully parsed part is guarded.
    pub fn guarded(&self) -> bool {
        self.parts.as_ref().is_ok_and(|parts| {
            parts
                .iter()
                .any(|part| part.input.as_ref().is_ok_and(Input::guarded))
        })
    }
}

pub fn parse_line(line: &str) -> Line {
    Line {
        text: line.to_owned(),
        parts: scan(line),
    }
}

/// Split both parts and words in one pass, retaining slices of the original
/// text: only top-level comments are dropped from words.
fn scan(line: &str) -> Result<Vec<Part>, String> {
    let mut chars = line.char_indices().peekable();
    let mut parts = Vec::new();
    let mut words = Vec::new();
    let mut word_start = None;
    let mut part_start = 0;
    let mut quote = None;
    let mut escaped = false;
    let mut comment_depth = 0;
    let mut brackets = Vec::new();

    while let Some((i, c)) = chars.next() {
        if comment_depth > 0 {
            if c == '(' && chars.peek().is_some_and(|&(_, next)| next == '*') {
                chars.next();
                comment_depth += 1;
            } else if c == '*' && chars.peek().is_some_and(|&(_, next)| next == ')') {
                chars.next();
                comment_depth -= 1;
            }
            continue;
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == delimiter {
                quote = None;
            }
            continue;
        }
        if c == '(' && chars.peek().is_some_and(|&(_, next)| next == '*') {
            if brackets.is_empty()
                && let Some(start) = word_start.take()
            {
                words.push(&line[start..i]);
            }
            chars.next();
            comment_depth = 1;
            continue;
        }
        if brackets.is_empty() && (c.is_whitespace() || c == ';') {
            if let Some(start) = word_start.take() {
                words.push(&line[start..i]);
            }
            if c == ';' {
                if !words.is_empty() {
                    parts.push(Part {
                        text: line[part_start..i].trim().to_owned(),
                        input: parse(&words),
                    });
                    words.clear();
                }
                part_start = i + c.len_utf8();
            }
            continue;
        }

        word_start.get_or_insert(i);
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' | '{' => brackets.push(c),
            ')' | ']' | '}' => {
                let opening = match c {
                    ')' => '(',
                    ']' => '[',
                    '}' => '{',
                    _ => unreachable!(),
                };
                if brackets.pop() != Some(opening) {
                    return Err(format!("Unbalanced line: unexpected '{}'", c));
                }
            }
            _ => {}
        }
    }

    if quote.is_some() {
        return Err("Unbalanced line: unterminated string".into());
    }
    if comment_depth > 0 {
        return Err("Unbalanced line: unterminated comment".into());
    }
    if let Some(opening) = brackets.last() {
        return Err(format!("Unbalanced line: unclosed '{}'", opening));
    }
    if let Some(start) = word_start {
        words.push(&line[start..]);
    }
    if !words.is_empty() {
        parts.push(Part {
            text: line[part_start..].trim().to_owned(),
            input: parse(&words),
        });
    }
    Ok(parts)
}

/// Read one part's already-scanned words; names and diagrams stay verbatim.
fn parse(words: &[&str]) -> Result<Input, String> {
    let (&word, args) = words.split_first().expect("scanner skips empty parts");
    let usage = |form: &str| Err(format!("Usage: {}", form));
    let session = |command| Ok(Input::Session(command));
    let bare = |command, form: &str| if args.is_empty() { session(command) } else { usage(form) };
    let named = |make: fn(String) -> Command, form: &str| match args {
        [name] => session(make((*name).to_owned())),
        _ => usage(form),
    };
    let count = |make: fn(usize) -> Command, form: &str| match args {
        [n] => n.parse().map(|n| Input::Session(make(n))).or_else(|_| usage(form)),
        _ => usage(form),
    };
    let toggle = |make: fn(Option<bool>) -> Command, form: &str| match args {
        [] => session(make(None)),
        ["on"] => session(make(Some(true))),
        ["off"] => session(make(Some(false))),
        _ => usage(form),
    };
    let path = |make: fn(Option<String>) -> Input, form: &str| match args {
        [] => Ok(make(None)),
        [path] => Ok(make(Some(decode_path(path)))),
        _ => usage(form),
    };

    match word {
        "types" | "Types" => bare(Command::Types, "types"),
        "status" | "show" => bare(Command::Show, "show"),
        "type" => named(Command::Type, "type <name>"),
        "homology" => named(Command::Homology, "homology <name>"),
        "start" => match args {
            [t, s, target @ ..] if target.len() <= 1 => session(Command::Start {
                type_name: (*t).to_owned(),
                initial: (*s).to_owned(),
                target: target.first().map(|g| (*g).to_owned()),
            }),
            _ => usage("start <type> <source> [<target>] — group a multi-word diagram in ( )"),
        },
        "resume" => match args {
            [t, p, target @ ..] if target.len() <= 1 => session(Command::Resume {
                type_name: (*t).to_owned(),
                proof: (*p).to_owned(),
                target: target.first().map(|g| (*g).to_owned()),
            }),
            _ => usage("resume <type> <proof> [<target>]"),
        },
        "apply" | "a" => match args.iter().map(|n| n.parse()).collect::<Result<Vec<usize>, _>>() {
            Ok(choices) if !choices.is_empty() => session(Command::Apply(choices)),
            _ => usage("apply <n> [<n2> ...]"),
        },
        "auto" => count(Command::Auto, "auto <n>"),
        "random" => count(Command::Random, "random <n>"),
        "undo" | "u" => match args {
            [] => session(Command::Undo(None)),
            ["all"] => session(Command::Undo(Some(0))),
            _ => count(|n| Command::Undo(Some(n)), "undo  |  undo <n>  |  undo all"),
        },
        "redo" => match args {
            [] => session(Command::Redo(None)),
            _ => count(|n| Command::Redo(Some(n)), "redo  |  redo <n>"),
        },
        "holes" => bare(Command::Holes, "holes"),
        "fill" => count(Command::Fill, "fill <n>"),
        "done" => bare(Command::Done, "done"),
        "restart" => bare(Command::Undo(Some(0)), "restart"),
        "stop" => bare(Command::Stop, "stop"),
        "rules" | "r" => bare(Command::Rules, "rules"),
        "history" | "h" => bare(Command::History, "history"),
        "proof" | "p" => bare(Command::Proof, "proof"),
        "store" => match args {
            [name] if crate::language::is_name(name) => session(Command::Store((*name).to_owned())),
            _ => usage("store <name> — a name, as in 'let <name> = …'"),
        },
        "parallel" => toggle(Command::Parallel, "parallel [on|off]"),
        "backward" => toggle(Command::Backward, "backward [on|off]"),
        "help" | "?" => if args.is_empty() { Ok(Input::Help) } else { usage("help") },
        "print" => bare(Command::Print, "print"),
        "load" => path(Input::Load, "load [<path>] — quote a path with spaces"),
        "save" => path(Input::Save, "save [<path>]"),
        _ => Err(format!(
            "Unrecognised command '{}' — type 'help' for a list",
            word
        )),
    }
}

/// Decode only a whole string literal, with the language's backslash escapes.
fn decode_path(word: &str) -> String {
    let mut chars = word.char_indices();
    let Some((_, delimiter @ ('\'' | '"'))) = chars.next() else {
        return word.into();
    };
    let mut decoded = String::new();
    while let Some((i, c)) = chars.next() {
        if c == delimiter {
            return if i + c.len_utf8() == word.len() {
                decoded
            } else {
                word.into()
            };
        }
        decoded.push(if c == '\\' {
            match chars.next().expect("scanner balances string escapes").1 {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                other => other,
            }
        } else {
            c
        });
    }
    unreachable!("scanner balances strings")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(line: &str) -> Result<Input, String> {
        let mut parts = parse_line(line).parts.expect("balanced line");
        assert_eq!(parts.len(), 1, "{line:?}");
        parts.remove(0).input
    }

    #[test]
    fn aliases_and_args_parse() {
        for (text, command) in [
            ("a 0 1", Command::Apply(vec![0, 1])),
            ("apply 2", Command::Apply(vec![2])),
            ("undo all", Command::Undo(Some(0))),
            ("restart", Command::Undo(Some(0))),
            ("undo", Command::Undo(None)),
            ("u 3", Command::Undo(Some(3))),
            ("redo", Command::Redo(None)),
            ("redo 2", Command::Redo(Some(2))),
            ("types", Command::Types),
            ("Types", Command::Types),
            ("status", Command::Show),
            ("show", Command::Show),
            ("rules", Command::Rules),
            ("r", Command::Rules),
            ("history", Command::History),
            ("h", Command::History),
            ("proof", Command::Proof),
            ("p", Command::Proof),
            ("parallel on", Command::Parallel(Some(true))),
            ("parallel off", Command::Parallel(Some(false))),
            ("parallel", Command::Parallel(None)),
            ("backward on", Command::Backward(Some(true))),
            ("backward off", Command::Backward(Some(false))),
            ("backward", Command::Backward(None)),
            ("holes", Command::Holes),
            ("fill 1", Command::Fill(1)),
            ("done", Command::Done),
            ("stop", Command::Stop),
            ("auto 10", Command::Auto(10)),
            ("random 4", Command::Random(4)),
            (
                "start C a b",
                Command::Start {
                    type_name: "C".into(),
                    initial: "a".into(),
                    target: Some("b".into()),
                },
            ),
            (
                "resume C p b",
                Command::Resume {
                    type_name: "C".into(),
                    proof: "p".into(),
                    target: Some("b".into()),
                },
            ),
        ] {
            assert_eq!(one(text), Ok(Input::Session(command)), "{text}");
        }
        assert_eq!(one("help"), Ok(Input::Help));
        assert_eq!(one("?"), Ok(Input::Help));
    }

    #[test]
    fn errors_keep_their_wording() {
        assert_eq!(one("type"), Err("Usage: type <name>".into()));
        assert_eq!(
            one("frobnicate"),
            Err("Unrecognised command 'frobnicate' — type 'help' for a list".into())
        );
        for (text, form) in [
            ("homology", "homology <name>"),
            (
                "start",
                "start <type> <source> [<target>] — group a multi-word diagram in ( )",
            ),
            ("resume", "resume <type> <proof> [<target>]"),
            ("apply", "apply <n> [<n2> ...]"),
            ("apply 1 bad", "apply <n> [<n2> ...]"),
            ("auto", "auto <n>"),
            ("random bad", "random <n>"),
            ("undo bad", "undo  |  undo <n>  |  undo all"),
            ("redo bad", "redo  |  redo <n>"),
            ("fill", "fill <n>"),
            ("store", "store <name> — a name, as in 'let <name> = …'"),
            ("parallel yes", "parallel [on|off]"),
            ("backward yes", "backward [on|off]"),
            ("print extra", "print"),
            ("save a b", "save [<path>]"),
            ("load a b", "load [<path>] — quote a path with spaces"),
        ] {
            assert_eq!(one(text), Err(format!("Usage: {form}")), "{text}");
        }
    }

    #[test]
    fn every_command_rejects_excess_arguments() {
        for text in [
            "types",
            "Types",
            "status",
            "show",
            "holes",
            "done",
            "restart",
            "stop",
            "rules",
            "r",
            "history",
            "h",
            "proof",
            "p",
            "help",
            "?",
            "print",
            "type T",
            "homology T",
            "start T a b",
            "resume T p b",
            "auto 1",
            "random 1",
            "undo 1",
            "undo all",
            "u 1",
            "redo 1",
            "fill 1",
            "store p",
            "parallel on",
            "backward off",
            "load a",
            "save a",
        ] {
            let text = format!("{text} extra");
            assert!(one(&text).unwrap_err().starts_with("Usage: "), "{text}");
        }
    }

    #[test]
    fn load_print_and_save_are_universal_but_local_commands_are_unknown() {
        assert_eq!(one("print"), Ok(Input::Session(Command::Print)));
        assert_eq!(one("load"), Ok(Input::Load(None)));
        assert_eq!(one("load F.ali"), Ok(Input::Load(Some("F.ali".into()))));
        assert_eq!(one("save"), Ok(Input::Save(None)));
        assert_eq!(one("save F.ali"), Ok(Input::Save(Some("F.ali".into()))));
        for local in ["quit", "exit", "q", "clear"] {
            assert_eq!(
                one(local),
                Err(format!(
                    "Unrecognised command '{local}' — type 'help' for a list"
                ))
            );
            let parts = parse_line(&format!("show; {local}")).parts.unwrap();
            assert_eq!(parts.len(), 2);
            assert!(parts[1].input.is_err());
        }
    }

    #[test]
    fn chain_splits_at_top_level_and_keeps_part_text() {
        let text = " ; show ; ; start T (a; [b; {c}]) \"a;b\"; (* only a comment *) ; help; ";
        let line = parse_line(text);
        assert_eq!(line.text, text);
        let parts = line.parts.unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].text, "show");
        assert_eq!(parts[1].text, "start T (a; [b; {c}]) \"a;b\"");
        assert_eq!(
            parts[1].input,
            Ok(Input::Session(Command::Start {
                type_name: "T".into(),
                initial: "(a; [b; {c}])".into(),
                target: Some("\"a;b\"".into()),
            }))
        );
        assert_eq!(parts[2].input, Ok(Input::Help));
        for empty in ["", " ; ; \n", "(* comment *)", " ; (* nested (* ; *) *) ;"] {
            assert!(parse_line(empty).parts.unwrap().is_empty());
        }
    }

    #[test]
    fn unbalanced_line_has_no_runnable_parts() {
        for text in [
            "show; start T (a; apply 0",
            "show; start T \"a; apply 0",
            "show; start T 'a",
            "show; start T (a]",
            "show; start T [a}",
            "show; start T {a)",
            "show; )",
            "show; ]",
            "show; }",
            "show; (* unclosed",
            "show; (* outer (* inner *)",
            "show; load \"a\\\"",
        ] {
            let line = parse_line(text);
            assert!(line.parts.is_err(), "{text:?}");
            assert!(!line.guarded(), "a rejected line runs nothing: {text:?}");
        }
    }

    #[test]
    fn words_keep_literals_and_brackets() {
        for initial in [
            "\"aaa\"",
            "'aaa'",
            "\"\"",
            "''",
            "(a #0 b)",
            "[a {b (c)}]",
            r#""a\";b""#,
            r"'a\';b'",
        ] {
            assert_eq!(
                one(&format!("start Assoc {initial}")),
                Ok(Input::Session(Command::Start {
                    type_name: "Assoc".into(),
                    initial: initial.into(),
                    target: None,
                }))
            );
            assert_eq!(
                one(&format!("resume Assoc {initial} c")),
                Ok(Input::Session(Command::Resume {
                    type_name: "Assoc".into(),
                    proof: initial.into(),
                    target: Some("c".into()),
                }))
            );
        }
    }

    #[test]
    fn names_are_taken_as_written() {
        for name in ["1Simplex", "2Simplex", "M.T", "map", "\"T\"", "(T)"] {
            assert_eq!(
                one(&format!("type {name}")),
                Ok(Input::Session(Command::Type(name.into())))
            );
            assert_eq!(
                one(&format!("homology {name}")),
                Ok(Input::Session(Command::Homology(name.into())))
            );
            assert_eq!(
                one(&format!("start {name} a")),
                Ok(Input::Session(Command::Start {
                    type_name: name.into(),
                    initial: "a".into(),
                    target: None,
                }))
            );
        }
    }

    #[test]
    fn comments_are_lexed() {
        let diagram = "(a (* author's note (* \" ] ; *) *) #0 b)";
        assert_eq!(
            one(&format!("start T {diagram}")),
            Ok(Input::Session(Command::Start {
                type_name: "T".into(),
                initial: diagram.into(),
                target: None,
            }))
        );
        assert_eq!(
            one("start(* note ' [ ; *)T(* nested (* x *) *)a"),
            one("start T a")
        );
        assert_eq!(
            one("start T \"(* not a comment *)\""),
            Ok(Input::Session(Command::Start {
                type_name: "T".into(),
                initial: "\"(* not a comment *)\"".into(),
                target: None,
            }))
        );
        assert_eq!(
            one("start T (a(* note *)b)"),
            Ok(Input::Session(Command::Start {
                type_name: "T".into(),
                initial: "(a(* note *)b)".into(),
                target: None,
            }))
        );
        assert_eq!(one("show(* trailing *)"), one("show"));
    }

    #[test]
    fn path_literals_decode_language_escapes() {
        for (word, path) in [
            ("\"my project/F.ali\"", "my project/F.ali"),
            ("'my project/F.ali'", "my project/F.ali"),
            (r#""a\nb\tc\rd\\e\"f\'g\ch""#, "a\nb\tc\rd\\e\"f'gch"),
            ("\"\"", ""),
            ("'a;b'", "a;b"),
            ("plain\\name.ali", "plain\\name.ali"),
            ("\"prefix\"suffix", "\"prefix\"suffix"),
            ("(a b)", "(a b)"),
        ] {
            assert_eq!(
                one(&format!("load {word}")),
                Ok(Input::Load(Some(path.into())))
            );
            assert_eq!(
                one(&format!("save {word}")),
                Ok(Input::Save(Some(path.into())))
            );
        }
    }

    #[test]
    fn tab_and_other_whitespace_separate_args() {
        for whitespace in ["\t", "\n", "\r\n", "\u{2003}", "\t \n"] {
            assert_eq!(
                one(&format!("start{whitespace}T{whitespace}s")),
                one("start T s")
            );
            assert_eq!(
                one(&format!("apply{whitespace}0{whitespace}1")),
                one("apply 0 1")
            );
        }
    }

    #[test]
    fn store_name_must_be_a_name() {
        for name in ["map", "in", "(p)", "\"p\"", "'p'", "a.b", "a b"] {
            assert_eq!(
                one(&format!("store {name}")),
                Err("Usage: store <name> — a name, as in 'let <name> = …'".into())
            );
        }
        for name in ["2p", "0Simplex", "2Morphism", "Δ", "_x", "42"] {
            assert_eq!(
                one(&format!("store {name}")),
                Ok(Input::Session(Command::Store(name.into())))
            );
        }
    }

    #[test]
    fn mutations_and_saves_are_guarded_but_queries_are_not() {
        for text in [
            "load",
            "load F.ali",
            "start T a",
            "resume T p",
            "fill 0",
            "done",
            "stop",
            "backward on",
            "backward off",
            "apply 0",
            "auto 2",
            "random 2",
            "undo",
            "undo 2",
            "undo all",
            "restart",
            "redo",
            "redo 2",
            "store p",
            "parallel on",
            "parallel off",
        ] {
            let input = one(text).unwrap();
            assert!(input.mutates(), "{text}");
            assert!(input.guarded(), "{text}");
            assert!(parse_line(text).guarded(), "{text}");
        }
        for text in ["save", "save F.ali"] {
            let input = one(text).unwrap();
            assert!(!input.mutates(), "{text}");
            assert!(input.guarded(), "{text}");
            assert!(parse_line(text).guarded(), "{text}");
        }
        for text in [
            "types",
            "type T",
            "homology T",
            "holes",
            "show",
            "print",
            "rules",
            "history",
            "proof",
            "backward",
            "parallel",
            "help",
            "?",
        ] {
            let input = one(text).unwrap();
            assert!(!input.mutates(), "{text}");
            assert!(!input.guarded(), "{text}");
            assert!(!parse_line(text).guarded(), "{text}");
        }
        assert!(parse_line("show; save").guarded());
        assert!(parse_line("unknown; load F.ali").guarded());
        assert!(!parse_line("show; store map").guarded());
        assert!(!parse_line("(* save; load F.ali *) help").guarded());
        assert!(!parse_line(" ").guarded());
    }
}
