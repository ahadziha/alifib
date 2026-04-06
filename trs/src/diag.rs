use alifib::language::ast::{self, Span, Spanned};

/// Wrap a value in a synthetic (zero-span) Spanned.
pub fn syn<T>(inner: T) -> Spanned<T> {
    Spanned {
        inner,
        span: Span::synthetic(),
    }
}

/// A builder wrapper around `ast::Diagram` for convenient composition.
pub struct Diag(pub ast::Diagram);

impl Diag {
    /// Single generator reference: produces `PrincipalPaste([Name(name)])`.
    pub fn atom(name: &str) -> Self {
        Diag(ast::Diagram::PrincipalPaste(vec![syn(
            ast::DExpr::Component(ast::DComponent::PartialMap(
                ast::PartialMapBasic::Name(name.to_owned()),
            )),
        )]))
    }

    /// Vertical composition (principal paste): `self other`.
    /// Flattens PrincipalPaste items; wraps Paste items in Paren.
    pub fn then(self, other: Self) -> Self {
        let mut parts = to_dexprs(self.0);
        parts.push(syn(to_single_dexpr(other.0)));
        Diag(ast::Diagram::PrincipalPaste(parts))
    }

    /// Flat vertical composition: extend self's PrincipalPaste with other's elements.
    /// Semantically equivalent to string space-joining: `self other` where both
    /// are sequences that should be merged into one flat sequence.
    /// Use this instead of `then` when concatenating two sequences at the same level.
    pub fn then_flat(self, other: Self) -> Self {
        let mut parts = to_dexprs(self.0);
        parts.extend(to_dexprs(other.0));
        Diag(ast::Diagram::PrincipalPaste(parts))
    }

    /// Horizontal composition: `self #0 other`.
    pub fn par(self, other: Self) -> Self {
        Diag(ast::Diagram::Paste {
            lhs: Box::new(syn(self.0)),
            dim: syn("0".to_owned()),
            rhs: vec![syn(to_single_dexpr(other.0))],
        })
    }

    pub fn into_ast(self) -> ast::Diagram {
        self.0
    }
}

/// Extract DExprs from a PrincipalPaste; wrap other Diagrams in Paren.
fn to_dexprs(d: ast::Diagram) -> Vec<Spanned<ast::DExpr>> {
    match d {
        ast::Diagram::PrincipalPaste(v) => v,
        other => vec![syn(ast::DExpr::Component(ast::DComponent::Paren(
            Box::new(syn(other)),
        )))],
    }
}

/// Convert a Diagram to a single DExpr.
/// A single-element PrincipalPaste is unwrapped; anything else is wrapped in Paren.
fn to_single_dexpr(d: ast::Diagram) -> ast::DExpr {
    match d {
        ast::Diagram::PrincipalPaste(mut v) if v.len() == 1 => v.pop().unwrap().inner,
        other => ast::DExpr::Component(ast::DComponent::Paren(Box::new(syn(other)))),
    }
}

/// Vertical sequence: fold a non-empty vec with `then`.
/// Each element after the first is wrapped in Paren if multi-element.
pub fn seq(parts: Vec<Diag>) -> Diag {
    parts
        .into_iter()
        .reduce(|a, b| a.then(b))
        .expect("seq called with empty vec")
}

/// Flat vertical sequence: collect all DExprs into one PrincipalPaste.
/// Equivalent to space-joining strings: all parts are flattened into one sequence.
pub fn seq_flat(parts: Vec<Diag>) -> Diag {
    let mut all: Vec<Spanned<ast::DExpr>> = Vec::new();
    for d in parts {
        all.extend(to_dexprs(d.0));
    }
    Diag(ast::Diagram::PrincipalPaste(all))
}

/// Horizontal sequence: fold a non-empty vec with `par` (left-associative `#0`).
pub fn hseq(parts: Vec<Diag>) -> Diag {
    parts
        .into_iter()
        .reduce(|a, b| a.par(b))
        .expect("hseq called with empty vec")
}

/// Check if a Diag is a single named atom.
pub fn is_atom(d: &Diag, name: &str) -> bool {
    match &d.0 {
        ast::Diagram::PrincipalPaste(v) if v.len() == 1 => matches!(
            &v[0].inner,
            ast::DExpr::Component(ast::DComponent::PartialMap(
                ast::PartialMapBasic::Name(n)
            )) if n == name
        ),
        _ => false,
    }
}
