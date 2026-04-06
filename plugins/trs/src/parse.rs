use std::collections::HashSet;

use crate::types::{SExpr, Term, TRS};

/// Tokenize ARI input. Handles:
/// - Whitespace skipping
/// - `;` line comments
/// - `(` and `)` as tokens
/// - `|quoted|` pipe-delimited symbols (content between pipes, without the pipes)
/// - Plain atoms (stop at whitespace, `(`, `)`, `;`, `|`)
pub fn tokenize(input: &str) -> Vec<String> {
    let chars: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Skip whitespace
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Line comment
        if c == ';' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Parentheses
        if c == '(' || c == ')' {
            tokens.push(c.to_string());
            i += 1;
            continue;
        }

        // Pipe-quoted symbol: |...|
        if c == '|' {
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '|' {
                s.push(chars[i]);
                i += 1;
            }
            i += 1; // consume closing |
            tokens.push(s);
            continue;
        }

        // Plain atom: stop at whitespace, (, ), ;, |
        let mut s = String::new();
        while i < chars.len() {
            let ch = chars[i];
            if ch.is_whitespace() || ch == '(' || ch == ')' || ch == ';' || ch == '|' {
                break;
            }
            s.push(ch);
            i += 1;
        }
        if !s.is_empty() {
            tokens.push(s);
        }
    }

    tokens
}

/// Parse a flat token list into a list of S-expressions.
pub fn parse_sexprs(tokens: &[String]) -> Vec<SExpr> {
    let mut result = Vec::new();
    let mut pos = 0;
    while pos < tokens.len() {
        let (sexpr, new_pos) = parse_one(tokens, pos);
        pos = new_pos;
        result.push(sexpr);
    }
    result
}

fn parse_one(tokens: &[String], pos: usize) -> (SExpr, usize) {
    if tokens[pos] == "(" {
        let mut pos = pos + 1;
        let mut list = Vec::new();
        while pos < tokens.len() && tokens[pos] != ")" {
            let (child, new_pos) = parse_one(tokens, pos);
            pos = new_pos;
            list.push(child);
        }
        // consume the closing ")"
        (SExpr::List(list), pos + 1)
    } else {
        (SExpr::Atom(tokens[pos].clone()), pos + 1)
    }
}

/// Parse a term from an S-expression, given the set of known function names.
pub fn parse_term(sexpr: &SExpr, fun_names: &HashSet<String>) -> Term {
    match sexpr {
        SExpr::Atom(s) => {
            if fun_names.contains(s) {
                Term::App {
                    fun: s.clone(),
                    args: vec![],
                }
            } else {
                Term::Var(s.clone())
            }
        }
        SExpr::List(items) => {
            let fun = match &items[0] {
                SExpr::Atom(s) => s.clone(),
                _ => panic!("expected atom as function head"),
            };
            let args = items[1..].iter().map(|s| parse_term(s, fun_names)).collect();
            Term::App { fun, args }
        }
    }
}

/// Parse an ARI-format string into a TRS.
pub fn parse_ari(input: &str) -> TRS {
    let tokens = tokenize(input);
    let sexprs = parse_sexprs(&tokens);

    let mut funs = Vec::new();
    let mut fun_names = HashSet::new();
    let mut rules = Vec::new();

    for sexpr in &sexprs {
        match sexpr {
            SExpr::List(items) if !items.is_empty() => {
                let head = match &items[0] {
                    SExpr::Atom(s) => s.as_str(),
                    _ => continue,
                };
                match head {
                    "format" => {}
                    "fun" => {
                        if items.len() >= 3 {
                            let name = match &items[1] {
                                SExpr::Atom(s) => s.clone(),
                                _ => continue,
                            };
                            let arity: usize = match &items[2] {
                                SExpr::Atom(s) => s.parse().unwrap_or(0),
                                _ => continue,
                            };
                            fun_names.insert(name.clone());
                            funs.push(crate::types::FunDecl { name, arity });
                        }
                    }
                    "rule" => {
                        if items.len() >= 3 {
                            let lhs = parse_term(&items[1], &fun_names);
                            let rhs = parse_term(&items[2], &fun_names);
                            rules.push(crate::types::Rule { lhs, rhs });
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    TRS { funs, rules }
}
