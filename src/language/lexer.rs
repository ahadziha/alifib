use chumsky::prelude::*;

use super::token::Token;

pub type Span = SimpleSpan;
pub type Spanned<T> = (T, Span);

pub fn lexer<'src>(
) -> impl Parser<'src, &'src str, Vec<Spanned<Token<'src>>>, extra::Err<Rich<'src, char>>> {
    let comment = recursive(|comment| {
        just("(*")
            .then(choice((comment.to(()), any().and_is(just("*)").not()).to(()))).repeated())
            .then(just("*)"))
    })
    .padded();

    let ident_or_nat_or_kw = any()
        .filter(|c: &char| c.is_alphanumeric() || *c == '_')
        .repeated()
        .at_least(1)
        .to_slice()
        .map(|s: &str| match s {
            "include" => Token::Include,
            "attach" => Token::Attach,
            "along" => Token::Along,
            "assert" => Token::Assert,
            "in" => Token::In,
            "out" => Token::Out,
            "Type" => Token::Type,
            "let" => Token::Let,
            "total" => Token::Total,
            "map" => Token::Map,
            "as" => Token::As,
            "index" => Token::Index,
            "for" => Token::For,
            _ => {
                if s.chars().all(|c| c.is_ascii_digit()) {
                    Token::Nat(s)
                } else {
                    Token::Ident(s)
                }
            }
        });

    let symbol = choice((
        just("<<=").to(Token::LArrow),
        just("::").to(Token::DColon),
        just("=>").to(Token::FatArrow),
        just("->").to(Token::Arrow),
        just('@').to(Token::At),
        just('{').to(Token::LBrace),
        just('}').to(Token::RBrace),
        just('[').to(Token::LBrack),
        just(']').to(Token::RBrack),
        just('(').to(Token::LParen),
        just(')').to(Token::RParen),
        just('<').to(Token::LAngle),
        just('>').to(Token::RAngle),
        just('.').to(Token::Dot),
        just(',').to(Token::Comma),
        just('#').to(Token::Hash),
        just(':').to(Token::Colon),
        just('=').to(Token::Eq),
        just('?').to(Token::Question),
    ));

    let token = choice((symbol, ident_or_nat_or_kw));

    token
        .map_with(|tok, e| (tok, e.span()))
        .padded_by(comment.repeated())
        .padded()
        .repeated()
        .collect()
}

#[cfg(test)]
mod tests {
    use chumsky::Parser;
    use super::*;
    use super::super::token::Token;

    fn lex(src: &str) -> Vec<Token<'_>> {
        lexer()
            .parse(src)
            .unwrap()
            .into_iter()
            .map(|(tok, _)| tok)
            .collect()
    }

    fn lex_spanned(src: &str) -> Vec<(Token<'_>, (usize, usize))> {
        lexer()
            .parse(src)
            .unwrap()
            .into_iter()
            .map(|(tok, span)| (tok, (span.start, span.end)))
            .collect()
    }

    // --- Keywords ---

    #[test]
    fn test_keywords() {
        assert_eq!(lex("include"), vec![Token::Include]);
        assert_eq!(lex("attach"), vec![Token::Attach]);
        assert_eq!(lex("along"), vec![Token::Along]);
        assert_eq!(lex("assert"), vec![Token::Assert]);
        assert_eq!(lex("as"), vec![Token::As]);
    }

    // Keywords that share prefixes don't shadow one another.
    #[test]
    fn test_as_vs_assert() {
        assert_eq!(lex("as assert"), vec![Token::As, Token::Assert]);
    }

    // A keyword that appears as a prefix of an identifier must lex as Ident.
    #[test]
    fn test_keyword_prefix_of_ident() {
        assert_eq!(lex("asdf"), vec![Token::Ident("asdf")]);
    }

    // --- Identifiers ---

    #[test]
    fn test_ident_simple() {
        assert_eq!(lex("foo"), vec![Token::Ident("foo")]);
    }

    #[test]
    fn test_ident_with_underscore() {
        assert_eq!(lex("foo_bar"), vec![Token::Ident("foo_bar")]);
    }

    #[test]
    fn test_ident_leading_underscore() {
        assert_eq!(lex("_foo"), vec![Token::Ident("_foo")]);
    }

    #[test]
    fn test_ident_mixed_alphanumeric() {
        assert_eq!(lex("foo123"), vec![Token::Ident("foo123")]);
        assert_eq!(lex("a1b2c3"), vec![Token::Ident("a1b2c3")]);
    }

    // --- Naturals ---

    #[test]
    fn test_nat_single_digit() {
        assert_eq!(lex("0"), vec![Token::Nat("0")]);
    }

    #[test]
    fn test_nat_multi_digit() {
        assert_eq!(lex("123"), vec![Token::Nat("123")]);
        assert_eq!(lex("007"), vec![Token::Nat("007")]);
    }

    // --- Symbols ---

    #[test]
    fn test_symbols_individual() {
        assert_eq!(lex("@"), vec![Token::At]);
        assert_eq!(lex("{"), vec![Token::LBrace]);
        assert_eq!(lex("}"), vec![Token::RBrace]);
        assert_eq!(lex("["), vec![Token::LBrack]);
        assert_eq!(lex("]"), vec![Token::RBrack]);
        assert_eq!(lex("("), vec![Token::LParen]);
        assert_eq!(lex(")"), vec![Token::RParen]);
        assert_eq!(lex("?"), vec![Token::Question]);
    }

    #[test]
    fn test_symbols_run() {
        assert_eq!(lex("@{}[]()"), vec![
            Token::At,
            Token::LBrace, Token::RBrace,
            Token::LBrack, Token::RBrack,
            Token::LParen, Token::RParen,
        ]);
    }

    // --- Whitespace ---

    #[test]
    fn test_leading_trailing_whitespace() {
        assert_eq!(lex("\n\t  foo  \n"), vec![Token::Ident("foo")]);
    }

    #[test]
    fn test_whitespace_between_tokens() {
        assert_eq!(lex("foo   bar"), vec![Token::Ident("foo"), Token::Ident("bar")]);
    }

    #[test]
    fn test_newlines_ignored() {
        assert_eq!(lex("foo\nbar"), vec![Token::Ident("foo"), Token::Ident("bar")]);
    }

    // --- Comments ---

    #[test]
    fn test_comment_between_tokens() {
        assert_eq!(lex("foo (* comment *) bar"), vec![
            Token::Ident("foo"),
            Token::Ident("bar"),
        ]);
    }

    #[test]
    fn test_empty_comment() {
        assert_eq!(lex("(**) foo"), vec![Token::Ident("foo")]);
    }

    #[test]
    fn test_nested_comment() {
        assert_eq!(lex("(* outer (* inner *) *) foo"), vec![Token::Ident("foo")]);
        assert_eq!(lex("(* a (* b (* c *) b *) a *) foo"), vec![Token::Ident("foo")]);
    }

    #[test]
    fn test_comment_adjacent_to_tokens() {
        // No whitespace between comment and tokens.
        assert_eq!(lex("foo(**)bar"), vec![
            Token::Ident("foo"),
            Token::Ident("bar"),
        ]);
    }

    #[test]
    fn test_multiple_comments() {
        assert_eq!(lex("(* a *) foo (* b *) bar (* c *) foo"), vec![
            Token::Ident("foo"),
            Token::Ident("bar"),
            Token::Ident("foo"),
        ]);
    }

    // --- Unicode identifiers ---

    #[test]
    fn test_ident_unicode_letters() {
        assert_eq!(lex("héllo"), vec![Token::Ident("héllo")]);
    }

    #[test]
    fn test_ident_cjk() {
        assert_eq!(lex("变量"), vec![Token::Ident("变量")]);
    }

    #[test]
    fn test_ident_greek() {
        assert_eq!(lex("αβγ"), vec![Token::Ident("αβγ")]);
    }

    #[test]
    fn test_ident_mixed_unicode_ascii() {
        assert_eq!(lex("café_42"), vec![Token::Ident("café_42")]);
    }

    #[test]
    fn test_ident_unicode_mixed_with_keyword() {
        // "as" followed immediately by a unicode char is an ident, not the keyword.
        assert_eq!(lex("asé"), vec![Token::Ident("asé")]);
    }

    #[test]
    fn test_unicode_digit_not_nat() {
        // Unicode digits like Arabic-Indic numerals should not lex as Nat.
        assert_eq!(lex("٣"), vec![Token::Ident("٣")]);
    }

    #[test]
    fn test_multiple_unicode_idents() {
        assert_eq!(lex("αβ γδ"), vec![
            Token::Ident("αβ"),
            Token::Ident("γδ"),
        ]);
    }

    #[test]
    fn test_unicode_ident_span() {
        // "αβ" is 4 bytes (2 bytes per char), so span end should be 4.
        assert_eq!(lex_spanned("αβ"), vec![(Token::Ident("αβ"), (0, 4))]);
    }

    #[test]
    fn test_unicode_ident_span_after_ascii() {
        // "ab" is 2 bytes, space is 1, "αβ" starts at byte 3 and ends at byte 7.
        assert_eq!(lex_spanned("ab αβ"), vec![
            (Token::Ident("ab"), (0, 2)),
            (Token::Ident("αβ"), (3, 7)),
        ]);
    }

    // --- Errors ---

    #[test]
    fn test_invalid_char() {
        assert!(lexer().parse("~").has_errors());
    }

    #[test]
    fn test_invalid_char_mid_input() {
        assert!(lexer().parse("foo ~ bar").has_errors());
    }

    #[test]
    fn test_unclosed_comment() {
        assert!(lexer().parse("(* unclosed").has_errors());
    }

    // --- Spans ---

    #[test]
    fn test_span_single_token() {
        assert_eq!(lex_spanned("foo"), vec![(Token::Ident("foo"), (0, 3))]);
    }

    #[test]
    fn test_span_nat() {
        assert_eq!(lex_spanned("123"), vec![(Token::Nat("123"), (0, 3))]);
    }

    #[test]
    fn test_span_symbol() {
        assert_eq!(lex_spanned("@"), vec![(Token::At, (0, 1))]);
    }

    #[test]
    fn test_span_two_tokens() {
        assert_eq!(lex_spanned("foo bar"), vec![
            (Token::Ident("foo"), (0, 3)),
            (Token::Ident("bar"), (4, 7)),
        ]);
    }

    #[test]
    fn test_span_leading_whitespace_excluded() {
        // The span should point at the token itself, not the preceding whitespace.
        assert_eq!(lex_spanned("   foo"), vec![(Token::Ident("foo"), (3, 6))]);
    }

    #[test]
    fn test_span_after_comment() {
        // Span should reflect position after the comment, not before it.
        assert_eq!(lex_spanned("(* hi *) foo"), vec![(Token::Ident("foo"), (9, 12))]);
    }

    #[test]
    fn test_span_adjacent_symbols() {
        assert_eq!(lex_spanned("{}"), vec![
            (Token::LBrace, (0, 1)),
            (Token::RBrace, (1, 2)),
        ]);
    }

    #[test]
    fn test_span_keyword() {
        assert_eq!(lex_spanned("include"), vec![(Token::Include, (0, 7))]);
    }

    #[test]
    fn test_span_multiple_tokens_mixed() {
        assert_eq!(lex_spanned("foo 42 @"), vec![
            (Token::Ident("foo"), (0, 3)),
            (Token::Nat("42"),   (4, 6)),
            (Token::At,          (7, 8)),
        ]);
    }
}
