use chumsky::prelude::*;
use super::token::{CommaOrigin, Keyword, Token};

pub type LexError = Simple<char>;
pub type Spanned<T> = (T, std::ops::Range<usize>);

/// Lex the input string into a sequence of (Token, span) pairs.
/// Whitespace and comments are skipped, but newlines produce implicit commas
/// in positions where the grammar allows them.
pub fn lex(src: &str) -> (Vec<Spanned<Token>>, Vec<LexError>) {
    lex_with_implicit_commas(src)
}

/// Scan the source text and inject implicit comma tokens wherever a newline
/// appears between two tokens that could be separated by a comma.
pub fn lex_with_implicit_commas(src: &str) -> (Vec<Spanned<Token>>, Vec<LexError>) {
    // First do a raw lex: collect tokens + their char positions.
    // chumsky's char-based parser (`Parser<char, ...>`) produces char-position spans,
    // not byte positions. We must convert to byte positions before slicing the source string.
    let raw_toks: Vec<Spanned<Token>> = {
        let lex_inner = raw_token_stream();
        let (toks, _errs) = lex_inner.parse_recovery(src);
        toks.unwrap_or_default()
    };
    let errors: Vec<LexError> = {
        let lex_inner = raw_token_stream();
        let (_toks, errs) = lex_inner.parse_recovery(src);
        errs
    };

    // Build char-index → byte-index mapping so we can convert spans.
    // `src.char_indices()` yields (byte_offset, char) pairs in order.
    let char_to_byte: Vec<usize> = src.char_indices().map(|(b, _)| b).collect();
    let to_byte = |ci: usize| char_to_byte.get(ci).copied().unwrap_or(src.len());

    // Convert all token spans from char positions to byte positions.
    let byte_toks: Vec<Spanned<Token>> = raw_toks
        .into_iter()
        .map(|(tok, span)| (tok, to_byte(span.start)..to_byte(span.end)))
        .collect();

    // Post-process: wherever a newline exists between two tokens in a position
    // where an implicit comma is valid, insert one.
    let with_commas = inject_implicit_commas(src, byte_toks);

    // Drop any comma (implicit or explicit) that is immediately followed by a
    // token that "closes" a context: @, }, ], or another comma (trailing), or
    // nothing (EOF).  This matches the OCaml parser_driver's filter_commas step.
    let result = filter_commas(with_commas);
    (result, errors)
}

fn raw_token_stream() -> impl Parser<char, Vec<Spanned<Token>>, Error = Simple<char>> {
    let comment = just("(*").then(take_until(just("*)"))).ignored();
    let ws = filter(|c: &char| c.is_whitespace()).ignored();

    let nat = text::int(10).map(Token::Nat);
    let ident = text::ident().map(|s: String| {
        match Keyword::from_str(&s) {
            Some(kw) => Token::Keyword(kw),
            None     => Token::Identifier(s),
        }
    });

    let has_value = just("<<=").to(Token::HasValue);
    let maps_to   = just("=>").to(Token::MapsTo);
    let arrow     = just("->").to(Token::Arrow);
    let of_shape  = just("::").to(Token::OfShape);
    let colon     = just(':').to(Token::Colon);

    let sym = choice((
        has_value, maps_to, arrow, of_shape, colon,
        just('@').to(Token::At),
        just('{').to(Token::LBrace),
        just('}').to(Token::RBrace),
        just('[').to(Token::LBracket),
        just(']').to(Token::RBracket),
        just('(').to(Token::LParen),
        just(')').to(Token::RParen),
        just(',').to(Token::Comma(CommaOrigin::Explicit)),
        just('.').to(Token::Dot),
        just('#').to(Token::Paste),
        just('=').to(Token::Equal),
        just('?').to(Token::Hole),
    ));

    let token = nat.or(ident).or(sym);
    let skippable = ws.or(comment);

    token
        .map_with_span(|tok, span| (tok, span))
        .padded_by(skippable.repeated())
        .repeated()
}

/// Drop every comma token that is immediately followed by a context-closing
/// token: `@`, `}`, `]`, `)`, another comma, or end-of-stream.  This mirrors
/// the OCaml parser_driver's `filter_commas` pass and prevents `separated_by`
/// parsers from greedily consuming inter-block separators.
fn filter_commas(tokens: Vec<Spanned<Token>>) -> Vec<Spanned<Token>> {
    let drops_after = |tok: &Token| matches!(
        tok,
        Token::At | Token::RBrace | Token::RBracket | Token::RParen | Token::Comma(_)
    );
    let n = tokens.len();
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        if matches!(&tokens[i].0, Token::Comma(_)) {
            // Drop this comma if it is a trailing comma (followed by a closer or EOF).
            let drop = match tokens.get(i + 1) {
                Some((next, _)) => drops_after(next),
                None => true, // trailing at EOF
            };
            if drop {
                continue;
            }
        }
        result.push(tokens[i].clone());
    }
    result
}

/// After a token at position `prev_end`, if the source text between two tokens
/// contains a "blank line" (2+ consecutive newlines, matching the OCaml lexer's
/// `newline_run >= 2` rule), inject an implicit comma.
fn inject_implicit_commas(src: &str, tokens: Vec<Spanned<Token>>) -> Vec<Spanned<Token>> {
    if tokens.is_empty() {
        return tokens;
    }
    let mut result = Vec::with_capacity(tokens.len());
    for (i, (tok, span)) in tokens.iter().enumerate() {
        if i > 0 {
            let prev_end = tokens[i - 1].1.end;
            let cur_start = span.start;
            let gap = &src[prev_end..cur_start];
            if gap_has_blank_line(gap)
                && comma_can_follow(&tokens[i - 1].0)
                && comma_can_precede(tok)
            {
                result.push((Token::Comma(CommaOrigin::FromNewline), prev_end..prev_end));
            }
        }
        result.push((tok.clone(), span.clone()));
    }
    result
}

/// Return true if `gap` contains at least two newlines without an intervening
/// real token, simulating the OCaml lexer's `newline_run >= 2` check.
///
/// OCaml rules (matching the .mll file exactly):
/// - `\n` or `\r\n` → increment newline_run; if ≥ 2, return true immediately
/// - horizontal whitespace `[' ' '\t' '\012' '\013']` → **no change** to newline_run
///   (the OCaml whitespace rule does NOT call `reset_newlines`)
/// - comment `(*…*)` → `(*` resets newline_run (like all real tokens)
/// - any other non-whitespace char → reset newline_run (it's a real token)
///
/// The key insight: `\n   \n` (newline, spaces, newline) still triggers a
/// comma because spaces do NOT reset the run counter.
fn gap_has_blank_line(gap: &str) -> bool {
    let bytes = gap.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut newline_run: usize = 0;
    while i < n {
        // Detect comment opening (*  — this resets the counter (OCaml: reset_newlines)
        if i + 1 < n && bytes[i] == b'(' && bytes[i + 1] == b'*' {
            newline_run = 0;
            i += 2;
            // Skip until matching *)
            while i + 1 < n && !(bytes[i] == b'*' && bytes[i + 1] == b')') {
                i += 1;
            }
            i += 2; // consume *)
            continue;
        }
        match bytes[i] {
            b'\n' => {
                newline_run += 1;
                if newline_run >= 2 {
                    return true;
                }
                i += 1;
            }
            b'\r' if i + 1 < n && bytes[i + 1] == b'\n' => {
                // \r\n counts as a single newline (matching OCaml's "\r\n" rule)
                newline_run += 1;
                if newline_run >= 2 {
                    return true;
                }
                i += 2;
            }
            b' ' | b'\t' | b'\r' | b'\x0C' => {
                // Horizontal whitespace: OCaml's whitespace rule does NOT call
                // reset_newlines, so we leave newline_run unchanged.
                i += 1;
            }
            _ => {
                // Any other character is a real token — reset (OCaml: reset_newlines).
                newline_run = 0;
                i += 1;
            }
        }
    }
    false
}

/// Whether an implicit comma can follow this token.
fn comma_can_follow(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Identifier(_)
        | Token::RBrace
        | Token::RBracket
        | Token::RParen
        | Token::Nat(_)
    )
}

/// Whether an implicit comma can precede this token (i.e., start a new instruction).
fn comma_can_precede(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Identifier(_)
        | Token::Keyword(Keyword::Include)
        | Token::Keyword(Keyword::Attach)
        | Token::Keyword(Keyword::Let)
        | Token::Keyword(Keyword::Assert)
        | Token::At
    )
}
