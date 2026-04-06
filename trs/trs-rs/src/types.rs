#[derive(Debug, Clone)]
pub struct FunDecl {
    pub name: String,
    pub arity: usize,
}

#[derive(Debug, Clone)]
pub enum Term {
    Var(String),
    App { fun: String, args: Vec<Term> },
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub lhs: Term,
    pub rhs: Term,
}

#[derive(Debug, Clone)]
pub struct TRS {
    pub funs: Vec<FunDecl>,
    pub rules: Vec<Rule>,
}

/// S-expression type used during parsing.
#[derive(Debug, Clone)]
pub enum SExpr {
    Atom(String),
    List(Vec<SExpr>),
}
