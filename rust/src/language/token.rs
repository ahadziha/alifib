use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Token<'src> {
    // Keywords
    Include,
    Attach,
    Along,
    Assert,
    In,
    Out,
    Type,
    Let,
    Map,
    As,

    // Symbols
    At,        // @
    LBrace,    // {
    RBrace,    // }
    LBrack,    // [
    RBrack,    // ]
    LParen,    // (
    RParen,    // )
    Dot,       // .
    Comma,     // ,
    Hash,      // #
    Colon,     // :
    DColon,    // ::
    FatArrow,  // =>
    Arrow,     // ->
    LArrow,    // <<=
    Eq,        // =
    Question,  // ?

    // Data
    Ident(&'src str),
    Nat(&'src str),
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Token::Include => write!(f, "include"),
            Token::Attach => write!(f, "attach"),
            Token::Along => write!(f, "along"),
            Token::Assert => write!(f, "assert"),
            Token::In => write!(f, "in"),
            Token::Out => write!(f, "out"),
            Token::Type => write!(f, "Type"),
            Token::Let => write!(f, "let"),
            Token::Map => write!(f, "map"),
            Token::As => write!(f, "as"),
            Token::At => write!(f, "@"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::LBrack => write!(f, "["),
            Token::RBrack => write!(f, "]"),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::Dot => write!(f, "."),
            Token::Comma => write!(f, ","),
            Token::Hash => write!(f, "#"),
            Token::Colon => write!(f, ":"),
            Token::DColon => write!(f, "::"),
            Token::FatArrow => write!(f, "=>"),
            Token::Arrow => write!(f, "->"),
            Token::LArrow => write!(f, "<<="),
            Token::Eq => write!(f, "="),
            Token::Question => write!(f, "?"),
            Token::Ident(s) => write!(f, "{s}"),
            Token::Nat(s) => write!(f, "{s}"),
        }
    }
}
