use chumsky::prelude::*;

use super::token::Token;

pub type Span = SimpleSpan;
pub type Spanned<T> = (T, Span);

pub fn lexer<'src>(
) -> impl Parser<'src, &'src str, Vec<Spanned<Token<'src>>>, extra::Err<Rich<'src, char>>> {
    let comment = just("(*")
        .then(any().and_is(just("*)").not()).repeated())
        .then(just("*)"))
        .padded();

    let nat = any()
        .filter(|c: &char| c.is_ascii_digit())
        .repeated()
        .at_least(1)
        .to_slice()
        .map(Token::Nat);

    let ident_or_kw = text::unicode::ident()
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
            _ => Token::Ident(s)
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
        just('.').to(Token::Dot),
        just(',').to(Token::Comma),
        just('#').to(Token::Hash),
        just(':').to(Token::Colon),
        just('=').to(Token::Eq),
        just('?').to(Token::Question),
    ));

    let token = choice((symbol, nat, ident_or_kw));

    token
        .map_with(|tok, e| (tok, e.span()))
        .padded_by(comment.repeated())
        .padded()
        .repeated()
        .collect()
}
