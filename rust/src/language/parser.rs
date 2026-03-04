use chumsky::input::MappedInput;
use chumsky::prelude::*;
use chumsky::recursive::Recursive;
use chumsky::span::SimpleSpan;

use super::ast::{
    Address, AssertStmt, AttachStmt, Block, Boundary, CInstr, Complex, DComponent, DExpr, DefPMap,
    Diagram, Generator, IncludeModule, IncludeStmt, LetDiag, LocalInst, NameWithBoundary, PMap,
    PMapBasic, PMapClause, PMapDef, PMapExt, Program, Span, Spanned, TypeInst,
};
use super::token::Token;

pub type TokenInput<'tokens, 'src> =
    MappedInput<'tokens, Token<'src>, SimpleSpan, &'tokens [(Token<'src>, SimpleSpan)]>;

type E<'tokens, 'src> = extra::Err<Rich<'tokens, Token<'src>, SimpleSpan>>;

/// Type-erased recursive parser alias. Using `recursive()` for type erasure
/// prevents symbol name explosion with deeply nested generic types.
type R<'tokens, 'src, O> = Recursive<
    chumsky::recursive::Direct<'tokens, 'tokens, TokenInput<'tokens, 'src>, O, E<'tokens, 'src>>,
>;

type RDiagram<'tokens, 'src> = R<'tokens, 'src, Spanned<Diagram>>;
type RPMap<'tokens, 'src> = R<'tokens, 'src, Spanned<PMap>>;
type RPMapDef<'tokens, 'src> = R<'tokens, 'src, Spanned<PMapDef>>;
type RComplex<'tokens, 'src> = R<'tokens, 'src, Spanned<Complex>>;

fn cspan(s: SimpleSpan) -> Span {
    Span {
        start: s.start,
        end: s.end,
    }
}

fn sp<T>(inner: T, span: Span) -> Spanned<T> {
    Spanned { inner, span }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn name<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<String>, E<'tokens, 'src>> + Clone {
    select_ref! {
        Token::Ident(s) => s.to_string(),
    }
    .map_with(|v, e| sp(v, cspan(e.span())))
}

fn nat<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Spanned<String>, E<'tokens, 'src>> + Clone {
    select_ref! {
        Token::Nat(s) => s.to_string(),
    }
    .map_with(|v, e| sp(v, cspan(e.span())))
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
        .map_with(|v, e| sp(v, cspan(e.span())))
}

// ---------------------------------------------------------------------------
// Let/Def shared enum
// ---------------------------------------------------------------------------

enum LetOrDef {
    Let(Spanned<Diagram>),
    Def(Spanned<Address>, Spanned<PMapDef>),
}

// ---------------------------------------------------------------------------
// Builder functions for mutually-referencing parsers.
//
// Each builder takes type-erased parser handles and returns a type-erased
// parser via recursive(). This prevents both:
// 1. Infinite recursion at parser construction time
// 2. Symbol name explosion from deeply nested generic types
// ---------------------------------------------------------------------------

/// Build PMapDef parser: `[clauses]` | PMap [`[clauses]`]
fn build_pmap_def<'tokens, 'src: 'tokens>(
    diagram: RDiagram<'tokens, 'src>,
    pmap: RPMap<'tokens, 'src>,
) -> RPMapDef<'tokens, 'src> {
    recursive(move |_| {
        let clause = diagram
            .clone()
            .then_ignore(t(Token::FatArrow))
            .then(diagram.clone())
            .map_with(|(lhs, rhs), e| sp(PMapClause { lhs, rhs }, cspan(e.span())));

        let bracketed = clause
            .separated_by(t(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(t(Token::LBrack), t(Token::RBrack));

        let bare_ext = bracketed.clone().map_with(|clauses, e| {
            sp(
                PMapDef::Ext(PMapExt {
                    prefix: None,
                    clauses,
                }),
                cspan(e.span()),
            )
        });

        let pmap_then_maybe_ext =
            pmap.clone()
                .then(bracketed.or_not())
                .map_with(|(pm, mc), e| match mc {
                    None => sp(PMapDef::PMap(pm.inner), cspan(e.span())),
                    Some(clauses) => sp(
                        PMapDef::Ext(PMapExt {
                            prefix: Some(Box::new(pm)),
                            clauses,
                        }),
                        cspan(e.span()),
                    ),
                });

        choice((bare_ext, pmap_then_maybe_ext))
    })
}

/// Build Complex parser: `Address? { CInstr, ... }` | `Address`
fn build_complex<'tokens, 'src: 'tokens>(
    diagram: RDiagram<'tokens, 'src>,
    pmap_def: RPMapDef<'tokens, 'src>,
) -> RComplex<'tokens, 'src> {
    recursive(move |_| {
        let let_or_def = t(Token::Let)
            .ignore_then(name())
            .then(choice((
                t(Token::DColon)
                    .ignore_then(address())
                    .then_ignore(t(Token::Eq))
                    .then(pmap_def.clone())
                    .map(|(a, v)| LetOrDef::Def(a, v)),
                t(Token::Eq)
                    .ignore_then(diagram.clone())
                    .map(LetOrDef::Let),
            )))
            .map_with(|(n, lod), e| match lod {
                LetOrDef::Let(v) => sp(
                    CInstr::LetDiag(LetDiag {
                        name: n,
                        value: v,
                    }),
                    cspan(e.span()),
                ),
                LetOrDef::Def(a, v) => sp(
                    CInstr::DefPMap(DefPMap {
                        name: n,
                        address: a,
                        value: v,
                    }),
                    cspan(e.span()),
                ),
            });

        let attach_stmt = t(Token::Attach)
            .ignore_then(name())
            .then_ignore(t(Token::DColon))
            .then(address())
            .then(t(Token::Along).ignore_then(pmap_def.clone()).or_not())
            .map_with(|((n, a), along), e| {
                sp(
                    CInstr::AttachStmt(AttachStmt {
                        name: n,
                        address: a,
                        along,
                    }),
                    cspan(e.span()),
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
                    cspan(e.span()),
                )
            });

        let boundary = diagram
            .clone()
            .then_ignore(t(Token::Arrow))
            .then(diagram.clone())
            .map_with(|(source, target), e| {
                sp(Boundary { source, target }, cspan(e.span()))
            });

        let nwb = name()
            .then(t(Token::Colon).ignore_then(boundary).or_not())
            .map_with(|(name, boundary), e| {
                sp(NameWithBoundary { name, boundary }, cspan(e.span()))
            })
            .map(|s| sp(CInstr::NameWithBoundary(s.inner), s.span));

        let cinstr = choice((attach_stmt, include_stmt, let_or_def, nwb));

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
                    cspan(e.span()),
                )
            });

        let complex_addr = address().map(|a| sp(Complex::Address(a.inner), a.span));

        choice((complex_block, complex_addr))
    })
}

/// Build PMap parser (actually recursive: PMap = PMapBasic [ "." PMap ])
fn build_pmap<'tokens, 'src: 'tokens>(
    diagram: RDiagram<'tokens, 'src>,
) -> RPMap<'tokens, 'src> {
    recursive(move |pmap: RPMap<'tokens, 'src>| {
        let pmap_def = build_pmap_def(diagram.clone(), pmap.clone());
        let complex = build_complex(diagram.clone(), pmap_def.clone());

        let anon_map = t(Token::LParen)
            .ignore_then(t(Token::Map))
            .ignore_then(pmap_def)
            .then_ignore(t(Token::DColon))
            .then(complex)
            .then_ignore(t(Token::RParen))
            .map(|(def, target)| PMapBasic::AnonMap {
                def: Box::new(def),
                target,
            });

        let paren_pmap = pmap
            .clone()
            .delimited_by(t(Token::LParen), t(Token::RParen))
            .map(|p| PMapBasic::Paren(Box::new(p)));

        let name_basic = name().map(|n| PMapBasic::Name(n.inner));
        let pmap_basic = choice((anon_map, paren_pmap, name_basic));

        pmap_basic
            .then(t(Token::Dot).ignore_then(pmap.clone()).or_not())
            .map_with(|(base, rest), e| match rest {
                None => sp(PMap::Basic(base), cspan(e.span())),
                Some(rest) => sp(
                    PMap::Dot {
                        base,
                        rest: Box::new(rest),
                    },
                    cspan(e.span()),
                ),
            })
    })
}

/// Build Diagram parser (actually recursive: through DComponent::Paren and AnonMap)
fn build_diagram<'tokens, 'src: 'tokens>() -> RDiagram<'tokens, 'src> {
    recursive(|diagram: RDiagram<'tokens, 'src>| {
        let pmap = build_pmap(diagram.clone());
        let pmap_def = build_pmap_def(diagram.clone(), pmap.clone());
        let complex = build_complex(diagram.clone(), pmap_def.clone());

        let anon_map_dcomp = t(Token::LParen)
            .ignore_then(t(Token::Map))
            .ignore_then(pmap_def)
            .then_ignore(t(Token::DColon))
            .then(complex)
            .then_ignore(t(Token::RParen))
            .map(|(def, target)| DComponent::PMap(PMapBasic::AnonMap {
                def: Box::new(def),
                target,
            }));

        let paren_pmap_dcomp = pmap
            .clone()
            .delimited_by(t(Token::LParen), t(Token::RParen))
            .map(|p| DComponent::PMap(PMapBasic::Paren(Box::new(p))));

        let dcomponent = choice((
            anon_map_dcomp,
            paren_pmap_dcomp,
            diagram
                .clone()
                .delimited_by(t(Token::LParen), t(Token::RParen))
                .map(|d| DComponent::Paren(Box::new(d))),
            t(Token::In).map(|_| DComponent::In),
            t(Token::Out).map(|_| DComponent::Out),
            t(Token::Question).map(|_| DComponent::Hole),
            select_ref! { Token::Ident(s) => DComponent::PMap(PMapBasic::Name(s.to_string())) },
        ))
        .map_with(|v, e| sp(v, cspan(e.span())));

        let dexpr = dcomponent
            .clone()
            .then(
                t(Token::Dot)
                    .ignore_then(dcomponent)
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(
                |(first, rest): (Spanned<DComponent>, Vec<Spanned<DComponent>>)| {
                    if rest.is_empty() {
                        sp(DExpr::Component(first.inner), first.span)
                    } else {
                        let mut expr: Spanned<DExpr> =
                            sp(DExpr::Component(first.inner), first.span);
                        for field in rest {
                            let new_span = Span {
                                start: expr.span.start,
                                end: field.span.end,
                            };
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
                },
            );

        let dprincipal = dexpr
            .clone()
            .repeated()
            .at_least(1)
            .collect::<Vec<_>>()
            .map_with(|exprs, e| sp(Diagram::Principal(exprs), cspan(e.span())));

        dprincipal.clone().foldl(
            t(Token::Hash)
                .ignore_then(nat())
                .then(dexpr.clone().repeated().at_least(1).collect::<Vec<_>>())
                .repeated(),
            |lhs: Spanned<Diagram>, (dim, rhs): (Spanned<String>, Vec<Spanned<DExpr>>)| {
                let end = rhs
                    .last()
                    .map(|r| r.span.end)
                    .unwrap_or(dim.span.end);
                let start = lhs.span.start;
                sp(
                    Diagram::Paste {
                        lhs: Box::new(lhs),
                        dim,
                        rhs,
                    },
                    Span { start, end },
                )
            },
        )
    })
}

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

pub fn program_parser<'tokens, 'src: 'tokens>(
) -> impl Parser<'tokens, TokenInput<'tokens, 'src>, Program, E<'tokens, 'src>> {
    let diagram = build_diagram();
    let pmap = build_pmap(diagram.clone());
    let pmap_def = build_pmap_def(diagram.clone(), pmap.clone());
    let complex = build_complex(diagram.clone(), pmap_def.clone());

    // --- Boundary & NameWithBoundary ---
    let boundary = diagram
        .clone()
        .then_ignore(t(Token::Arrow))
        .then(diagram.clone())
        .map_with(|(source, target), e| sp(Boundary { source, target }, cspan(e.span())));

    let name_with_boundary = name()
        .then(t(Token::Colon).ignore_then(boundary).or_not())
        .map_with(|(name, boundary), e| {
            sp(NameWithBoundary { name, boundary }, cspan(e.span()))
        });

    // --- Local instructions ---
    let assert_stmt = t(Token::Assert)
        .ignore_then(diagram.clone())
        .then_ignore(t(Token::Eq))
        .then(diagram.clone())
        .map_with(|(lhs, rhs), e| {
            sp(
                LocalInst::AssertStmt(AssertStmt { lhs, rhs }),
                cspan(e.span()),
            )
        });

    let let_or_def_local = t(Token::Let)
        .ignore_then(name())
        .then(choice((
            t(Token::DColon)
                .ignore_then(address())
                .then_ignore(t(Token::Eq))
                .then(pmap_def.clone())
                .map(|(a, v)| LetOrDef::Def(a, v)),
            t(Token::Eq)
                .ignore_then(diagram.clone())
                .map(LetOrDef::Let),
        )))
        .map_with(|(n, lod), e| match lod {
            LetOrDef::Let(v) => sp(
                LocalInst::LetDiag(LetDiag {
                    name: n,
                    value: v,
                }),
                cspan(e.span()),
            ),
            LetOrDef::Def(a, v) => sp(
                LocalInst::DefPMap(DefPMap {
                    name: n,
                    address: a,
                    value: v,
                }),
                cspan(e.span()),
            ),
        });

    let local_inst = choice((assert_stmt, let_or_def_local));

    // --- Type instructions ---
    let generator = name_with_boundary
        .then_ignore(t(Token::LArrow))
        .then(complex.clone())
        .map_with(|(name, complex), e| {
            sp(
                TypeInst::Generator(Generator { name, complex }),
                cspan(e.span()),
            )
        });

    let include_module = t(Token::Include)
        .ignore_then(name())
        .then(t(Token::As).ignore_then(name()).or_not())
        .map_with(|(name, alias), e| {
            sp(
                TypeInst::IncludeModule(IncludeModule { name, alias }),
                cspan(e.span()),
            )
        });

    let let_or_def_type = t(Token::Let)
        .ignore_then(name())
        .then(choice((
            t(Token::DColon)
                .ignore_then(address())
                .then_ignore(t(Token::Eq))
                .then(pmap_def)
                .map(|(a, v)| LetOrDef::Def(a, v)),
            t(Token::Eq)
                .ignore_then(diagram.clone())
                .map(LetOrDef::Let),
        )))
        .map_with(|(n, lod), e| match lod {
            LetOrDef::Let(v) => sp(
                TypeInst::LetDiag(LetDiag {
                    name: n,
                    value: v,
                }),
                cspan(e.span()),
            ),
            LetOrDef::Def(a, v) => sp(
                TypeInst::DefPMap(DefPMap {
                    name: n,
                    address: a,
                    value: v,
                }),
                cspan(e.span()),
            ),
        });

    let type_inst = choice((generator, include_module, let_or_def_type));

    // --- Blocks ---
    let type_block = t(Token::At)
        .then(t(Token::Type))
        .ignore_then(
            type_inst
                .separated_by(t(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .map_with(|insts, e| sp(Block::TypeBlock(insts), cspan(e.span())));

    let local_block = t(Token::At)
        .ignore_then(complex)
        .then(
            local_inst
                .separated_by(t(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .map_with(|(complex, body), e| {
            sp(Block::LocalBlock { complex, body }, cspan(e.span()))
        });

    let block = choice((type_block, local_block));

    block
        .repeated()
        .collect()
        .then_ignore(end())
        .map(|blocks| Program { blocks })
}
