use chumsky::input::MappedInput;
use chumsky::prelude::*;
use chumsky::span::Spanned as Sp;

use super::ast::*;
use super::token::Token;

pub type TokenInput<'tokens, 'src> =
    MappedInput<'tokens, Token<'src>, Span, &'tokens [(Token<'src>, Span)]>;

type E<'tokens, 'src> = extra::Err<Rich<'tokens, Token<'src>, Span>>;

fn sp<T>(inner: T, span: Span) -> Spanned<T> {
    Sp { inner, span }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn name<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<String>, E<'tokens, 'src>> + Clone {
    select_ref! {
        Token::Ident(s) => s.to_string(),
    }
    .spanned()
}

fn nat<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<String>, E<'tokens, 'src>> + Clone {
    select_ref! {
        Token::Nat(s) => s.to_string(),
    }
    .spanned()
}

fn t<'tokens, 'src: 'tokens>(
    tok: Token<'src>,
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, (), E<'tokens, 'src>> + Clone {
    just(tok).ignored()
}

// ---------------------------------------------------------------------------
// Address = Name { "." Name }
// ---------------------------------------------------------------------------

fn address<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<Address>, E<'tokens, 'src>> + Clone {
    name()
        .separated_by(t(Token::Dot))
        .at_least(1)
        .collect::<Vec<_>>()
        .spanned()
}

// ---------------------------------------------------------------------------
// Diagram grammar (recursive)
// ---------------------------------------------------------------------------

fn diagram_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<Diagram>, E<'tokens, 'src>> + Clone {
    recursive(|diagram| {
        // DComponent = Name | "in" | "out" | "(" Diagram ")" | "?"
        let dcomponent = choice((
            diagram
                .clone()
                .delimited_by(t(Token::LParen), t(Token::RParen))
                .map(|d| DComponent::Paren(Box::new(d))),
            t(Token::In).map(|_| DComponent::In),
            t(Token::Out).map(|_| DComponent::Out),
            t(Token::Question).map(|_| DComponent::Hole),
            select_ref! { Token::Ident(s) => DComponent::Name(s.to_string()) },
        ))
        .spanned();

        // DExpr = DComponent { "." DComponent }
        let dexpr = dcomponent
            .clone()
            .then(
                t(Token::Dot)
                    .ignore_then(dcomponent)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(first, rest): (Spanned<DComponent>, Vec<Spanned<DComponent>>)| {
                if rest.is_empty() {
                    sp(DExpr::Component(first.inner), first.span)
                } else {
                    let mut expr: Spanned<DExpr> =
                        sp(DExpr::Component(first.inner), first.span);
                    for field in rest {
                        let new_span =
                            Span::new(expr.span.context(), expr.span.start()..field.span.end());
                        expr = sp(
                            DExpr::Dot {
                                base: Box::new(expr),
                                field,
                            },
                            new_span,
                        );
                    }
                    expr
                }
            });

        // DPrincipal = DExpr { DExpr }  (juxtaposition = implicit pasting)
        let dprincipal = dexpr
            .clone()
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .map_with(|exprs, e| sp(Diagram::Principal(exprs), e.span()));

        // Diagram = DPrincipal { "#" Nat DPrincipal }
        dprincipal.clone().foldl(
            t(Token::Hash)
                .ignore_then(nat())
                .then(dexpr.clone().repeated().at_least(1).collect::<Vec<_>>())
                .repeated(),
            |lhs: Spanned<Diagram>, (dim, rhs): (Spanned<String>, Vec<Spanned<DExpr>>)| {
                let end = rhs
                    .last()
                    .map(|r| r.span.end())
                    .unwrap_or(dim.span.end());
                let new_span = Span::new(lhs.span.context(), lhs.span.start()..end);
                sp(
                    Diagram::Paste {
                        lhs: Box::new(lhs),
                        dim,
                        rhs,
                    },
                    new_span,
                )
            },
        )
    })
}

// ---------------------------------------------------------------------------
// Boundary
// ---------------------------------------------------------------------------

fn boundary<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<Boundary>, E<'tokens, 'src>> + Clone
{
    diagram_parser()
        .then_ignore(t(Token::Arrow))
        .then(diagram_parser())
        .map_with(|(source, target), e| sp(Boundary { source, target }, e.span()))
}

fn name_with_boundary<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<NameWithBoundary>, E<'tokens, 'src>>
       + Clone {
    name()
        .then(t(Token::Colon).ignore_then(boundary()).or_not())
        .map_with(|(name, boundary), e| sp(NameWithBoundary { name, boundary }, e.span()))
}

// ---------------------------------------------------------------------------
// PMap grammar (recursive)
// ---------------------------------------------------------------------------

fn pmap_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<PMap>, E<'tokens, 'src>> + Clone {
    recursive(|pmap| {
        let clause = diagram_parser()
            .then_ignore(t(Token::FatArrow))
            .then(diagram_parser())
            .map_with(|(lhs, rhs), e| sp(PMapClause { lhs, rhs }, e.span()));

        let clauses = clause
            .separated_by(t(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>();

        let bracketed = clauses.delimited_by(t(Token::LBrack), t(Token::RBrack));

        // Name optionally followed by [clauses] (handles "foo[x => y]" and bare "foo")
        let name_atom = name()
            .then(bracketed.clone().or_not())
            .map(|(n, maybe_clauses)| match maybe_clauses {
                None => PMapBasic::Name(n.inner),
                Some(clauses) => PMapBasic::System(PMSystem {
                    extend: Some(Box::new(sp(
                        PMap::Basic(PMapBasic::Name(n.inner)),
                        n.span,
                    ))),
                    clauses,
                }),
            });

        // Bare [clauses]
        let bare_system = bracketed.map(|clauses| {
            PMapBasic::System(PMSystem {
                extend: None,
                clauses,
            })
        });

        let pmap_basic = choice((name_atom, bare_system));

        // PMap = PMapBasic [ "." PMap ]
        // Recursion only after consuming ".", so always makes progress
        pmap_basic
            .then(t(Token::Dot).ignore_then(pmap).or_not())
            .map_with(|(base, rest), e| match rest {
                None => sp(PMap::Basic(base), e.span()),
                Some(rest) => sp(
                    PMap::Dot {
                        base,
                        rest: Box::new(rest),
                    },
                    e.span(),
                ),
            })
    })
}

// ---------------------------------------------------------------------------
// Complex
// ---------------------------------------------------------------------------

fn complex_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<Complex>, E<'tokens, 'src>> + Clone {
    let let_diag = t(Token::Let)
        .ignore_then(name())
        .then(t(Token::Colon).ignore_then(boundary()).or_not())
        .then_ignore(t(Token::Eq))
        .then(diagram_parser())
        .map_with(|((n, b), v), e| {
            sp(
                CInstr::LetDiag(LetDiag {
                    name: n,
                    boundary: b,
                    value: v,
                }),
                e.span(),
            )
        });

    let def_pmap = t(Token::Def)
        .ignore_then(name())
        .then_ignore(t(Token::DColon))
        .then(address())
        .then_ignore(t(Token::Eq))
        .then(pmap_parser())
        .map_with(|((n, a), v), e| {
            sp(
                CInstr::DefPMap(DefPMap {
                    name: n,
                    address: a,
                    value: v,
                }),
                e.span(),
            )
        });

    let attach_stmt = t(Token::Attach)
        .ignore_then(name())
        .then_ignore(t(Token::DColon))
        .then(address())
        .then(t(Token::Along).ignore_then(pmap_parser()).or_not())
        .map_with(|((n, a), along), e| {
            sp(
                CInstr::AttachStmt(AttachStmt {
                    name: n,
                    address: a,
                    along,
                }),
                e.span(),
            )
        });

    let include_stmt = t(Token::Include)
        .ignore_then(address())
        .then(t(Token::As).ignore_then(name()).or_not())
        .map_with(|(a, alias), e| {
            sp(
                CInstr::IncludeStmt(IncludeStmt {
                    address: a,
                    alias,
                }),
                e.span(),
            )
        });

    let nwb = name_with_boundary().map(|s| sp(CInstr::NameWithBoundary(s.inner), s.span));

    let cinstr = choice((attach_stmt, include_stmt, let_diag, def_pmap, nwb));

    let complex_body = cinstr
        .separated_by(t(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>();

    let complex_block = address()
        .or_not()
        .then(complex_body.delimited_by(t(Token::LBrace), t(Token::RBrace)))
        .map_with(|(addr, body), e| {
            sp(
                Complex::Block {
                    address: addr.map(|a| a.inner),
                    body,
                },
                e.span(),
            )
        });

    let complex_addr = address().map(|a| sp(Complex::Address(a.inner), a.span));

    choice((complex_block, complex_addr))
}

// ---------------------------------------------------------------------------
// Let/Def for local and type blocks (standalone)
// ---------------------------------------------------------------------------

fn let_diag_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, LetDiag, E<'tokens, 'src>> + Clone {
    t(Token::Let)
        .ignore_then(name())
        .then(t(Token::Colon).ignore_then(boundary()).or_not())
        .then_ignore(t(Token::Eq))
        .then(diagram_parser())
        .map(|((n, b), v)| LetDiag {
            name: n,
            boundary: b,
            value: v,
        })
}

fn def_pmap_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, DefPMap, E<'tokens, 'src>> + Clone {
    t(Token::Def)
        .ignore_then(name())
        .then_ignore(t(Token::DColon))
        .then(address())
        .then_ignore(t(Token::Eq))
        .then(pmap_parser())
        .map(|((n, a), v)| DefPMap {
            name: n,
            address: a,
            value: v,
        })
}

// ---------------------------------------------------------------------------
// Local instructions (at a complex)
// ---------------------------------------------------------------------------

fn local_inst<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<LocalInst>, E<'tokens, 'src>> + Clone
{
    let assert_stmt = t(Token::Assert)
        .ignore_then(diagram_parser())
        .then_ignore(t(Token::Eq))
        .then(diagram_parser())
        .map_with(|(lhs, rhs), e| sp(LocalInst::AssertStmt(AssertStmt { lhs, rhs }), e.span()));

    let let_local =
        let_diag_parser().map_with(|l, e| sp(LocalInst::LetDiag(l), e.span()));
    let def_local =
        def_pmap_parser().map_with(|d, e| sp(LocalInst::DefPMap(d), e.span()));

    choice((assert_stmt, def_local, let_local))
}

// ---------------------------------------------------------------------------
// Type instructions
// ---------------------------------------------------------------------------

fn type_inst<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<TypeInst>, E<'tokens, 'src>> + Clone
{
    let generator = name_with_boundary()
        .then_ignore(t(Token::LArrow))
        .then(complex_parser())
        .map_with(|(name, complex), e| {
            sp(TypeInst::Generator(Generator { name, complex }), e.span())
        });

    let include_module = t(Token::Include)
        .ignore_then(name())
        .then(t(Token::As).ignore_then(name()).or_not())
        .map_with(|(name, alias), e| {
            sp(
                TypeInst::IncludeModule(IncludeModule { name, alias }),
                e.span(),
            )
        });

    let let_type =
        let_diag_parser().map_with(|l, e| sp(TypeInst::LetDiag(l), e.span()));
    let def_type =
        def_pmap_parser().map_with(|d, e| sp(TypeInst::DefPMap(d), e.span()));

    choice((generator, include_module, def_type, let_type))
}

// ---------------------------------------------------------------------------
// Block
// ---------------------------------------------------------------------------

fn block<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<Block>, E<'tokens, 'src>> + Clone {
    let type_block = t(Token::At)
        .then(t(Token::Type))
        .ignore_then(
            type_inst()
                .separated_by(choice((t(Token::Semi), t(Token::Comma))))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .map_with(|insts, e| sp(Block::TypeBlock(insts), e.span()));

    let local_block = t(Token::At)
        .ignore_then(complex_parser())
        .then(
            local_inst()
                .separated_by(choice((t(Token::Semi), t(Token::Comma))))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .map_with(|(complex, body), e| sp(Block::LocalBlock { complex, body }, e.span()));

    choice((type_block, local_block))
}

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

pub fn program_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Program, E<'tokens, 'src>> {
    block()
        .repeated()
        .collect()
        .then_ignore(end())
        .map(|blocks| Program { blocks })
}
