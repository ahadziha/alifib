use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Keyword {
    Include,
    Attach,
    Along,
    Assert,
    In,
    Out,
    Type,
    Let,
    As,
    Map,
}

impl Keyword {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "include" => Some(Self::Include),
            "attach"  => Some(Self::Attach),
            "along"   => Some(Self::Along),
            "assert"  => Some(Self::Assert),
            "in"      => Some(Self::In),
            "out"     => Some(Self::Out),
            "Type"    => Some(Self::Type),
            "let"     => Some(Self::Let),
            "as"      => Some(Self::As),
            "map"     => Some(Self::Map),
            _         => None,
        }
    }
}

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Include => "include",
            Self::Attach  => "attach",
            Self::Along   => "along",
            Self::Assert  => "assert",
            Self::In      => "in",
            Self::Out     => "out",
            Self::Type    => "Type",
            Self::Let     => "let",
            Self::As      => "as",
            Self::Map     => "map",
        };
        write!(f, "{}", s)
    }
}

/// Whether a comma was written explicitly or implied by a newline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommaOrigin {
    Explicit,
    FromNewline,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    At,
    Keyword(Keyword),
    Identifier(String),
    Nat(String),
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Comma(CommaOrigin),
    Dot,
    Paste,      // #
    Colon,      // :
    OfShape,    // ::
    MapsTo,     // =>
    Arrow,      // ->
    HasValue,   // <<=
    Equal,      // =
    Hole,       // ?
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::At              => write!(f, "@"),
            Self::Keyword(k)     => write!(f, "{}", k),
            Self::Identifier(s)  => write!(f, "{}", s),
            Self::Nat(n)         => write!(f, "{}", n),
            Self::LBrace         => write!(f, "{{"),
            Self::RBrace         => write!(f, "}}"),
            Self::LBracket       => write!(f, "["),
            Self::RBracket       => write!(f, "]"),
            Self::LParen         => write!(f, "("),
            Self::RParen         => write!(f, ")"),
            Self::Comma(_)       => write!(f, ","),
            Self::Dot            => write!(f, "."),
            Self::Paste          => write!(f, "#"),
            Self::Colon          => write!(f, ":"),
            Self::OfShape        => write!(f, "::"),
            Self::MapsTo         => write!(f, "=>"),
            Self::Arrow          => write!(f, "->"),
            Self::HasValue       => write!(f, "<<="),
            Self::Equal          => write!(f, "="),
            Self::Hole           => write!(f, "?"),
        }
    }
}
