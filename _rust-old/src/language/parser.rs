/// Chumsky-based parser for the alifib language.
///
/// Grammar summary (from grammar.md):
///
/// ```text
/// <Program>  ::= { <Block> }
/// <Block>    ::= "@" "Type" [ <CBlockType> ] | "@" <Complex> [ <CBlockLocal> ]
/// <Complex>  ::= <Address> | [ <Address> ] "{" [ <CBlock> ] "}"
/// <CBlockType> ::= <CInstrType> { "," <CInstrType> }
/// <CBlock>     ::= <CInstr>     { "," <CInstr> }
/// <CBlockLocal>::= <CInstrLocal>{ "," <CInstrLocal> }
/// <GeneratorType> ::= <Generator> "<<=" <Complex>
/// <Generator>     ::= <Name> [ ":" <Boundaries> ]
/// <Address>  ::= <Name> { "." <Name> }
/// <Morphism> ::= <MComp> | <Morphism> "." <MComp>
/// <MComp>    ::= <MTerm> | <Name>
/// <MTerm>    ::= "(" "map" <MExt> "::" <Complex> ")"
/// <MExt>     ::= [ <Morphism> ] "[" [ <MBlock> ] "]"
/// <MBlock>   ::= <MInstr> { "," <MInstr> }
/// <MInstr>   ::= <Pasting> "=>" <Pasting>
/// <MDef>     ::= <Morphism> | <MExt>
/// <MNamer>   ::= "let" <Name> "::" <Address> "=" <MDef>
/// <DNamer>   ::= "let" <Name> [ ":" <Boundaries> ] "=" <Diagram>
/// <Boundaries> ::= <Diagram> "->" <Diagram>
/// <IncludeStatement>  ::= "include" <Address> [ "as" <Name> ]
/// <IncludeModule>     ::= "include" <Name> [ "as" <Name> ]
/// <AttachStatement>   ::= "attach" <Name> "::" <Address> [ "along" <MDef> ]
/// <AssertStatement>   ::= "assert" <Pasting> "=" <Pasting>
/// <Diagram>  ::= <DConcat> | <Diagram> "#" <Nat> <DConcat>
/// <DConcat>  ::= <DExpr> | <DConcat> <DExpr>
/// <DExpr>    ::= <DComp> | <DExpr> "." <DComp>
/// <DComp>    ::= <MTerm> | <DTerm> | <Name> | <Bd> | "?"
/// <DTerm>    ::= "(" <Diagram> "#" <Nat> <DConcat> ")" | "(" <DConcat> <DExpr> ")"
/// <Pasting>  ::= <Concat> | <Pasting> "#" <Nat> <Concat>
/// <Concat>   ::= <DExpr> | <Concat> <DExpr>
/// ```
use chumsky::prelude::*;
use chumsky::recursive::Recursive;
use super::token::{Keyword, Token};
use super::ast::*;
use crate::helper::positions::{Span, span_from_range};
use crate::core::diagram::Sign as DiagramSign;
use super::diagnostics::Report;

type PError = Simple<Token>;

// ---- Helper: lift a range into a Span (without source info at this stage) ----

fn mk_span(range: &std::ops::Range<usize>, src_name: &str, src: &str) -> Span {
    span_from_range(src, src_name, range.clone())
}

// ---- Parse the whole thing ----

pub fn parse(
    tokens: Vec<(Token, std::ops::Range<usize>)>,
    src: &str,
    src_name: &str,
) -> (Program, Report) {
    // Attach span construction helper
    let make_span = |r: &std::ops::Range<usize>| mk_span(r, src_name, src);

    // Build the token stream chumsky expects
    let token_stream = tokens.iter().map(|(t, r)| (t.clone(), r.clone())).collect::<Vec<_>>();
    let len = src.len();
    let stream = chumsky::Stream::from_iter(len..len + 1, token_stream.into_iter());

    let parser = program_parser();
    let (ast, errors) = parser.parse_recovery(stream);

    let mut report = Report::empty();
    for err in errors {
        let span = make_span(&err.span());
        let msg = format_parse_error(&err);
        let diag = super::diagnostics::Diagnostic::error(
            super::diagnostics::parser_producer(),
            span,
            msg,
        );
        report.add(diag);
    }

    let program = ast.unwrap_or_else(empty_program);
    (program, report)
}

fn format_parse_error(err: &Simple<Token>) -> String {
    match err.reason() {
        chumsky::error::SimpleReason::Unexpected => {
            let found = err.found().map(|t| format!("`{}`", t)).unwrap_or_else(|| "end of input".into());
            let expected: Vec<String> = err.expected()
                .filter_map(|e| e.as_ref().map(|t| format!("`{}`", t)))
                .collect();
            if expected.is_empty() {
                format!("unexpected token {}", found)
            } else {
                format!("unexpected token {}, expected one of: {}", found, expected.join(", "))
            }
        }
        chumsky::error::SimpleReason::Unclosed { span: _, delimiter } => {
            format!("unclosed delimiter `{}`", delimiter)
        }
        chumsky::error::SimpleReason::Custom(msg) => msg.clone(),
    }
}

// ---- Token matchers ----

fn tok(t: Token) -> impl Parser<Token, Token, Error = PError> + Clone {
    just(t)
}

fn keyword(kw: Keyword) -> impl Parser<Token, Token, Error = PError> + Clone {
    tok(Token::Keyword(kw))
}

fn comma() -> impl Parser<Token, Token, Error = PError> + Clone {
    filter(|t: &Token| matches!(t, Token::Comma(_)))
}

fn ident_tok() -> impl Parser<Token, String, Error = PError> + Clone {
    filter_map(|span, t: Token| match t {
        Token::Identifier(s) => Ok(s),
        _ => Err(Simple::expected_input_found(span, [], Some(t))),
    })
}

fn nat_tok() -> impl Parser<Token, usize, Error = PError> + Clone {
    filter_map(|span, t: Token| match t {
        Token::Nat(ref s) => s.parse::<usize>().map_err(|_| Simple::expected_input_found(span, [], Some(t))),
        _ => Err(Simple::expected_input_found(span, [], Some(t))),
    })
}

// ---- AST node constructors ----

fn name_p() -> impl Parser<Token, Name, Error = PError> + Clone {
    ident_tok().map_with_span(|s, span| Node::with_span(s, dummy_span(span)))
}

fn nat_p() -> impl Parser<Token, Nat, Error = PError> + Clone {
    nat_tok().map_with_span(|n, span| Node::with_span(n, dummy_span(span)))
}

fn dummy_span(range: std::ops::Range<usize>) -> Span {
    use crate::helper::positions::Point;
    let p = Point::new("<parse>".to_owned(), range.start, 1, 1, 0);
    let q = Point::new("<parse>".to_owned(), range.end, 1, 1, 0);
    Span::new(p, q)
}

// ---- Address ----

fn address_p() -> impl Parser<Token, Address, Error = PError> + Clone {
    name_p()
        .separated_by(tok(Token::Dot))
        .at_least(1)
        .map_with_span(|names, span| Node::with_span(names, dummy_span(span)))
}

// ---- Bd ----

fn bd_p() -> impl Parser<Token, Bd, Error = PError> + Clone {
    keyword(Keyword::In)
        .to(DiagramSign::Input)
        .or(keyword(Keyword::Out).to(DiagramSign::Output))
        .map_with_span(|s, span| Node::with_span(s, dummy_span(span)))
}

// Converts DConcatDesc to ConcatDesc (same structure, different types)
fn dconcat_to_concat(dc: DConcat) -> Concat {
    match dc.value {
        DConcatDesc::Single(e) => Node { span: dc.span, value: ConcatDesc::Single(e) },
        DConcatDesc::Concat { left, right } => {
            let converted_left = dconcat_to_concat(*left);
            Node {
                span: dc.span,
                value: ConcatDesc::Concat { left: Box::new(converted_left), right },
            }
        }
    }
}

// ---- Full parser via mutual recursion ----

fn program_parser() -> impl Parser<Token, Program, Error = PError> {
    // We use chumsky's Recursive::declare()/define() for the mutually recursive parsers.
    //
    // The mutual recursion graph:
    //   complex <-> morphism (via c_block -> mnamer -> m_def -> morphism; morphism -> m_term -> complex)
    //   complex <-> diagram  (via c_block -> generator/dnamer -> diagram; diagram -> d_comp -> m_term -> complex)
    //   morphism <-> m_ext   (m_ext -> m_instr -> pasting -> d_comp -> m_term -> m_ext; morphism -> m_comp -> m_term -> m_ext)
    //   diagram  <-> d_comp  (d_comp -> d_term -> d_concat -> d_expr -> d_comp)
    //   (pasting similarly)
    //
    // We declare: complex, morphism, diagram, m_ext, d_comp, p_d_comp (for pasting)
    // Each pair of d_comp/p_d_comp is separate because Recursive is invariant over lifetime.

    let mut complex_rec: Recursive<Token, Complex, PError> = Recursive::declare();
    let mut morphism_rec: Recursive<Token, Morphism, PError> = Recursive::declare();
    let mut diagram_rec: Recursive<Token, Diagram, PError> = Recursive::declare();
    let mut m_ext_rec: Recursive<Token, MExt, PError> = Recursive::declare();
    // d_comp is recursive (d_comp -> d_term -> d_concat -> d_expr -> d_comp)
    let mut d_comp_rec: Recursive<Token, DComp, PError> = Recursive::declare();
    // A separate d_comp for pasting (same grammar, but different Recursive instance)
    let mut p_d_comp_rec: Recursive<Token, DComp, PError> = Recursive::declare();

    // ================================================================
    // Build diagram-side parsers using d_comp_rec
    // ================================================================

    // d_expr = d_comp ("." d_comp)*
    let d_expr = d_comp_rec.clone()
        .map_with_span(|c, span| Node::with_span(DExprDesc::Single(c), dummy_span(span)))
        .then(
            tok(Token::Dot)
                .ignore_then(d_comp_rec.clone())
                .repeated()
        )
        .foldl(|left, right| Node {
            span: None,
            value: DExprDesc::Dot { left: Box::new(left), right },
        });

    // d_concat = d_expr (d_expr)* (juxtaposition)
    let d_concat = d_expr.clone()
        .map_with_span(|e, span| Node::with_span(DConcatDesc::Single(e), dummy_span(span)))
        .then(d_expr.clone().repeated())
        .foldl(|left, right| Node {
            span: None,
            value: DConcatDesc::Concat { left: Box::new(left), right },
        });

    // d_term = "(" d_concat "#" nat d_concat ")" | "(" d_concat d_expr ")"
    //
    // IMPORTANT: We use `d_concat` (not `diagram_rec`) for the first sub-expression in
    // the indexed form.  The OCaml LALR grammar says `d_term_indexed = ( diagram # nat d_concat )`,
    // but because Menhir shifts `#` tokens greedily, `diagram` inside `(...)` is
    // effectively just a `d_concat` (no top-level `#`).  Using `diagram_rec` here would
    // cause `(x #0 y)` to fail: `diagram_rec` greedily captures `x #0 y`, leaving nothing
    // for the required `#` separator in `d_term_indexed`.
    let d_term_indexed = d_concat.clone()
        .then_ignore(tok(Token::Paste))
        .then(nat_p())
        .then(d_concat.clone())
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|((head, nat), tail), span| {
            // The OCaml AST wraps the head d_concat as a Diagram_single.
            let diag = Node::with_span(DiagramDesc::Single(head), dummy_span(span.clone()));
            Node::with_span(
                DTermDesc::Indexed { diagram: Box::new(diag), nat, tail },
                dummy_span(span),
            )
        });

    // pair form: "(" d_expr d_expr+ ")" — at least 2 d_exprs
    let d_term_pair = d_expr.clone()
        .then(d_expr.clone().repeated().at_least(1))
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|(first, rest), span| {
            let mut exprs = rest;
            let last_expr = exprs.pop().unwrap(); // safe: at_least(1)
            // Build d_concat from first and remaining exprs (all but last)
            let concat = if exprs.is_empty() {
                Node::with_span(DConcatDesc::Single(first), dummy_span(span.clone()))
            } else {
                let mut acc = Node::with_span(DConcatDesc::Single(first), dummy_span(span.clone()));
                for e in exprs {
                    acc = Node { span: None, value: DConcatDesc::Concat { left: Box::new(acc), right: e } };
                }
                acc
            };
            Node::with_span(
                DTermDesc::Pair { concat, expr: last_expr },
                dummy_span(span),
            )
        });

    let d_term = d_term_indexed.or(d_term_pair);

    // m_term (for diagram's d_comp) uses m_ext_rec and complex_rec
    let m_term_for_d = keyword(Keyword::Map)
        .ignore_then(m_ext_rec.clone())
        .then_ignore(tok(Token::OfShape))
        .then(complex_rec.clone())
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|(ext, target), span| {
            Node::with_span(MTermDesc { ext, target }, dummy_span(span))
        });

    // d_comp = m_term | d_term | bd | "?" | name
    // Order: m_term first (starts with "(map"), then d_term (starts with "("),
    // then bd (keywords "in"/"out"), then "?", then name (identifier).
    let d_comp_parser = m_term_for_d.clone()
        .map_with_span(|t, span| Node::with_span(DCompDesc::Mterm(t), dummy_span(span)))
        .or(d_term.map_with_span(|t, span| Node::with_span(DCompDesc::Dterm(Box::new(t)), dummy_span(span))))
        .or(bd_p().map_with_span(|b, span| Node::with_span(DCompDesc::Bd(b), dummy_span(span))))
        .or(just(Token::Hole).map_with_span(|_, span| Node::with_span(DCompDesc::Hole, dummy_span(span))))
        .or(name_p().map_with_span(|n, span| Node::with_span(DCompDesc::Name(n), dummy_span(span))));

    d_comp_rec.define(d_comp_parser);

    // diagram = d_concat ("#" nat d_concat)*
    let diagram_parser = d_concat.clone()
        .map_with_span(|c, span| Node::with_span(DiagramDesc::Single(c), dummy_span(span)))
        .then(
            tok(Token::Paste)
                .ignore_then(nat_p())
                .then(d_concat.clone())
                .repeated()
        )
        .foldl(|left, (nat, right)| Node {
            span: None,
            value: DiagramDesc::Paste { left: Box::new(left), nat, right },
        });

    diagram_rec.define(diagram_parser);

    // ================================================================
    // Build pasting-side parsers using p_d_comp_rec (separate from d_comp_rec)
    // Pasting uses the same grammar as diagram but different types for Concat/Pasting.
    // ================================================================

    // p_d_expr = p_d_comp ("." p_d_comp)*
    let p_d_expr = p_d_comp_rec.clone()
        .map_with_span(|c, span| Node::with_span(DExprDesc::Single(c), dummy_span(span)))
        .then(
            tok(Token::Dot)
                .ignore_then(p_d_comp_rec.clone())
                .repeated()
        )
        .foldl(|left, right| Node {
            span: None,
            value: DExprDesc::Dot { left: Box::new(left), right },
        });

    // p_concat_inner = p_d_expr (p_d_expr)* — this is the "concat" in pasting grammar
    let p_concat_inner = p_d_expr.clone()
        .map_with_span(|e, span| Node::with_span(DConcatDesc::Single(e), dummy_span(span)))
        .then(p_d_expr.clone().repeated())
        .foldl(|left, right| Node {
            span: None,
            value: DConcatDesc::Concat { left: Box::new(left), right },
        });

    // For pasting's d_term: same fix as d_term_indexed above — use p_concat_inner
    // (not diagram_rec) for the first sub-expression so that `#` is treated as the
    // separator rather than being greedily consumed inside the head.
    let p_d_term_indexed = p_concat_inner.clone()
        .then_ignore(tok(Token::Paste))
        .then(nat_p())
        .then(p_concat_inner.clone())
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|((head, nat), tail), span| {
            let diag = Node::with_span(DiagramDesc::Single(head), dummy_span(span.clone()));
            Node::with_span(
                DTermDesc::Indexed { diagram: Box::new(diag), nat, tail },
                dummy_span(span),
            )
        });

    let p_d_term_pair = p_d_expr.clone()
        .then(p_d_expr.clone().repeated().at_least(1))
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|(first, rest), span| {
            let mut exprs = rest;
            let last_expr = exprs.pop().unwrap();
            let concat = if exprs.is_empty() {
                Node::with_span(DConcatDesc::Single(first), dummy_span(span.clone()))
            } else {
                let mut acc = Node::with_span(DConcatDesc::Single(first), dummy_span(span.clone()));
                for e in exprs {
                    acc = Node { span: None, value: DConcatDesc::Concat { left: Box::new(acc), right: e } };
                }
                acc
            };
            Node::with_span(
                DTermDesc::Pair { concat, expr: last_expr },
                dummy_span(span),
            )
        });

    let p_d_term = p_d_term_indexed.or(p_d_term_pair);

    // m_term for pasting's d_comp (same grammar as m_term_for_d)
    let m_term_for_p = keyword(Keyword::Map)
        .ignore_then(m_ext_rec.clone())
        .then_ignore(tok(Token::OfShape))
        .then(complex_rec.clone())
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|(ext, target), span| {
            Node::with_span(MTermDesc { ext, target }, dummy_span(span))
        });

    // p_d_comp = m_term | d_term | bd | "?" | name (same as d_comp but uses p variants)
    let p_d_comp_parser = m_term_for_p
        .map_with_span(|t, span| Node::with_span(DCompDesc::Mterm(t), dummy_span(span)))
        .or(p_d_term.map_with_span(|t, span| Node::with_span(DCompDesc::Dterm(Box::new(t)), dummy_span(span))))
        .or(bd_p().map_with_span(|b, span| Node::with_span(DCompDesc::Bd(b), dummy_span(span))))
        .or(just(Token::Hole).map_with_span(|_, span| Node::with_span(DCompDesc::Hole, dummy_span(span))))
        .or(name_p().map_with_span(|n, span| Node::with_span(DCompDesc::Name(n), dummy_span(span))));

    p_d_comp_rec.define(p_d_comp_parser);

    // pasting = concat ("#" nat concat)*   where concat uses p_concat_inner
    // We convert DConcat -> Concat for the pasting type
    let pasting = p_concat_inner.clone()
        .map_with_span(|c, span| {
            let concat = dconcat_to_concat(c);
            Node::with_span(PastingDesc::Single(concat), dummy_span(span))
        })
        .then(
            tok(Token::Paste)
                .ignore_then(nat_p())
                .then(p_concat_inner.clone())
                .repeated()
        )
        .foldl(|left, (nat, right_dc)| {
            let right = dconcat_to_concat(right_dc);
            Node { span: None, value: PastingDesc::Paste { left: Box::new(left), nat, right } }
        });

    // ================================================================
    // Build m_instr / m_block / m_ext (m_ext_rec)
    // ================================================================

    // m_instr = pasting "=>" pasting
    let m_instr_p = pasting.clone()
        .then_ignore(tok(Token::MapsTo))
        .then(pasting.clone())
        .map_with_span(|(source, target), span| {
            Node::with_span(MInstrDesc { source, target }, dummy_span(span))
        });

    // m_block = m_instr ("," m_instr)*
    let m_block_p = m_instr_p
        .separated_by(comma())
        .map_with_span(|instrs, span| Node::with_span(instrs, dummy_span(span)));

    // m_ext = morphism? "[" m_block? "]"
    let m_ext_parser = morphism_rec.clone()
        .or_not()
        .then(
            m_block_p.clone()
                .or_not()
                .delimited_by(tok(Token::LBracket), tok(Token::RBracket))
        )
        .map_with_span(|(prefix, block), span| {
            Node::with_span(
                MExtDesc { prefix: prefix.map(Box::new), block },
                dummy_span(span),
            )
        });

    m_ext_rec.define(m_ext_parser);

    // ================================================================
    // Build morphism_rec
    // ================================================================

    // m_term for morphism (reuse m_term_for_d - same grammar, but need a fresh clone)
    let m_term_for_morph = keyword(Keyword::Map)
        .ignore_then(m_ext_rec.clone())
        .then_ignore(tok(Token::OfShape))
        .then(complex_rec.clone())
        .delimited_by(tok(Token::LParen), tok(Token::RParen))
        .map_with_span(|(ext, target), span| {
            Node::with_span(MTermDesc { ext, target }, dummy_span(span))
        });

    // m_comp = m_term | name
    let m_comp = m_term_for_morph
        .map_with_span(|t, span| Node::with_span(MCompDesc::Term(t), dummy_span(span)))
        .or(name_p().map_with_span(|n, span| Node::with_span(MCompDesc::Name(n), dummy_span(span))));

    // morphism = m_comp ("." m_comp)*
    let morphism_parser = m_comp.clone()
        .map_with_span(|mc, span| Node::with_span(MorphismDesc::Single(mc), dummy_span(span)))
        .then(
            tok(Token::Dot)
                .ignore_then(m_comp.clone())
                .repeated()
        )
        .foldl(|left, right| Node {
            span: None,
            value: MorphismDesc::Concat { left: Box::new(left), right },
        });

    morphism_rec.define(morphism_parser);

    // ================================================================
    // Build m_def = morphism ("[" m_block? "]")? | "[" m_block? "]"
    // ================================================================
    // We cannot simply use `m_ext_rec.or(morphism_rec)` because m_ext_rec has the form
    // `morphism? "[" m_block? "]"`, and if morphism_rec greedily consumes a morphism like
    // `Fst.Dom` and then fails to find `[...]`, chumsky will NOT backtrack to try the
    // bare `morphism_rec` alternative (tokens were already consumed).
    //
    // Fix: parse morphism first, then OPTIONALLY try `[...]`.  This way:
    //   - `Fst.Dom`          → morphism + no `[` → MDefDesc::Morphism
    //   - `Fst.Dom [...]`    → morphism + `[...]` → MDefDesc::Ext (with prefix)
    //   - `[...]`            → morphism fails immediately on `[` (no consumed tokens)
    //                          → fallback arm parses `[...]` → MDefDesc::Ext (no prefix)
    let m_block_opt_brackets = m_block_p.clone()
        .or_not()
        .delimited_by(tok(Token::LBracket), tok(Token::RBracket));

    let m_def_p = morphism_rec.clone()
        .then(m_block_opt_brackets.clone().or_not())
        .map_with_span(|(prefix, block_opt), span| {
            match block_opt {
                None => Node::with_span(MDefDesc::Morphism(prefix), dummy_span(span)),
                Some(block) => Node::with_span(
                    MDefDesc::Ext(Node::with_span(
                        MExtDesc { prefix: Some(Box::new(prefix)), block },
                        dummy_span(span.clone()),
                    )),
                    dummy_span(span),
                ),
            }
        })
        .or(
            m_block_opt_brackets
                .map_with_span(|block, span| {
                    Node::with_span(
                        MDefDesc::Ext(Node::with_span(
                            MExtDesc { prefix: None, block },
                            dummy_span(span.clone()),
                        )),
                        dummy_span(span),
                    )
                })
        );

    // ================================================================
    // Build diagram-based helpers (boundaries, generator, etc.)
    // ================================================================

    // boundaries = diagram "->" diagram
    let boundaries = diagram_rec.clone()
        .then_ignore(tok(Token::Arrow))
        .then(diagram_rec.clone())
        .map_with_span(|(source, target), span| {
            Node::with_span(BoundariesDesc { source, target }, dummy_span(span))
        });

    // generator = name (":" boundaries)?
    let generator = name_p()
        .then(
            tok(Token::Colon)
                .ignore_then(boundaries.clone())
                .or_not()
        )
        .map_with_span(|(name, bounds), span| {
            Node::with_span(GeneratorDesc { name, boundaries: bounds }, dummy_span(span))
        });

    // ================================================================
    // Build c_block instructions
    // ================================================================

    // dnamer = "let" name (":" boundaries)? "=" diagram
    let dnamer = keyword(Keyword::Let)
        .ignore_then(name_p())
        .then(tok(Token::Colon).ignore_then(boundaries.clone()).or_not())
        .then_ignore(tok(Token::Equal))
        .then(diagram_rec.clone())
        .map_with_span(|((name, bounds), body), span| {
            Node::with_span(DnamerDesc { name, boundaries: bounds, body }, dummy_span(span))
        });

    // mnamer = "let" name "::" address "=" m_def
    let mnamer = keyword(Keyword::Let)
        .ignore_then(name_p())
        .then_ignore(tok(Token::OfShape))
        .then(address_p())
        .then_ignore(tok(Token::Equal))
        .then(m_def_p.clone())
        .map_with_span(|((name, address), definition), span| {
            Node::with_span(MnamerDesc { name, address, definition }, dummy_span(span))
        });

    // include_stmt = "include" address ("as" name)?
    let include_stmt = keyword(Keyword::Include)
        .ignore_then(address_p())
        .then(keyword(Keyword::As).ignore_then(name_p()).or_not())
        .map_with_span(|(address, alias), span| {
            Node::with_span(IncludeStatementDesc { address, alias }, dummy_span(span))
        });

    // include_mod = "include" name ("as" name)?
    let include_mod = keyword(Keyword::Include)
        .ignore_then(name_p())
        .then(keyword(Keyword::As).ignore_then(name_p()).or_not())
        .map_with_span(|(name, alias), span| {
            Node::with_span(IncludeModuleDesc { name, alias }, dummy_span(span))
        });

    // attach_stmt = "attach" name "::" address ("along" m_def)?
    let attach_stmt = keyword(Keyword::Attach)
        .ignore_then(name_p())
        .then_ignore(tok(Token::OfShape))
        .then(address_p())
        .then(keyword(Keyword::Along).ignore_then(m_def_p.clone()).or_not())
        .map_with_span(|((name, address), along), span| {
            Node::with_span(AttachStatementDesc { name, address, along }, dummy_span(span))
        });

    // assert_stmt = "assert" pasting "=" pasting
    let assert_stmt = keyword(Keyword::Assert)
        .ignore_then(pasting.clone())
        .then_ignore(tok(Token::Equal))
        .then(pasting.clone())
        .map_with_span(|(left, right), span| {
            Node::with_span(AssertStatementDesc { left, right }, dummy_span(span))
        });

    // ================================================================
    // c_instr = generator | dnamer | mnamer | include_stmt | attach_stmt
    //
    // Disambiguation order:
    //   - "let" name "::" → mnamer  (try first, "::" is more specific)
    //   - "let" name ...  → dnamer
    //   - "include" ...   → include_stmt
    //   - "attach" ...    → attach_stmt
    //   - name ...        → generator
    // ================================================================
    let c_instr = mnamer.clone()
        .map_with_span(|m, span| Node::with_span(CInstrDesc::Mnamer(m), dummy_span(span)))
        .or(dnamer.clone()
            .map_with_span(|d, span| Node::with_span(CInstrDesc::Dnamer(d), dummy_span(span))))
        .or(include_stmt.clone()
            .map_with_span(|i, span| Node::with_span(CInstrDesc::Include(i), dummy_span(span))))
        .or(attach_stmt.clone()
            .map_with_span(|a, span| Node::with_span(CInstrDesc::Attach(a), dummy_span(span))))
        .or(generator.clone()
            .map_with_span(|g, span| Node::with_span(CInstrDesc::Generator(g), dummy_span(span))));

    let c_block_inner = c_instr
        .separated_by(comma())
        .map_with_span(|instrs, span| Node::with_span(instrs, dummy_span(span)));

    // ================================================================
    // complex = address? "{" c_block? "}" | address
    //
    // Strategy (as described in the task):
    //   - Parse optional address first
    //   - Then try to parse optional delimited block
    //   - If both are None, fail
    // ================================================================
    let complex_parser = address_p()
        .or_not()
        .then(
            c_block_inner
                .clone()
                .or_not()
                .delimited_by(tok(Token::LBrace), tok(Token::RBrace))
                .or_not()
        )
        .try_map(|(addr, block_with_braces), span| {
            match (&addr, &block_with_braces) {
                (None, None) => Err(Simple::expected_input_found(span, [], None)),
                _ => Ok(ComplexDesc {
                    address: addr,
                    block: block_with_braces.flatten(),
                })
            }
        })
        .map_with_span(|desc, span| Node::with_span(desc, dummy_span(span)));

    complex_rec.define(complex_parser);

    // ================================================================
    // generator_type = generator "<<=" complex
    // ================================================================
    let generator_type = generator.clone()
        .then_ignore(tok(Token::HasValue))
        .then(complex_rec.clone())
        .map_with_span(|(gen, def), span| {
            Node::with_span(GeneratorTypeDesc { generator: gen, definition: def }, dummy_span(span))
        });

    // ================================================================
    // c_instr_type = generator_type | dnamer | mnamer | include_module
    // ================================================================
    let c_instr_type = generator_type.clone()
        .map_with_span(|gt, span| Node::with_span(CInstrTypeDesc::Generator(gt), dummy_span(span)))
        .or(mnamer.clone()
            .map_with_span(|m, span| Node::with_span(CInstrTypeDesc::Mnamer(m), dummy_span(span))))
        .or(dnamer.clone()
            .map_with_span(|d, span| Node::with_span(CInstrTypeDesc::Dnamer(d), dummy_span(span))))
        .or(include_mod.clone()
            .map_with_span(|i, span| Node::with_span(CInstrTypeDesc::IncludeModule(i), dummy_span(span))));

    let c_block_type = c_instr_type
        .separated_by(comma())
        .map_with_span(|instrs, span| Node::with_span(instrs, dummy_span(span)));

    // ================================================================
    // c_instr_local = dnamer | mnamer | assert_stmt
    // ================================================================
    let c_instr_local = mnamer.clone()
        .map_with_span(|m, span| Node::with_span(CInstrLocalDesc::Mnamer(m), dummy_span(span)))
        .or(dnamer.clone()
            .map_with_span(|d, span| Node::with_span(CInstrLocalDesc::Dnamer(d), dummy_span(span))))
        .or(assert_stmt.clone()
            .map_with_span(|a, span| Node::with_span(CInstrLocalDesc::Assert(a), dummy_span(span))));

    let c_block_local = c_instr_local
        .separated_by(comma())
        .map_with_span(|instrs, span| Node::with_span(instrs, dummy_span(span)));

    // ================================================================
    // Block = "@" "Type" c_block_type? | "@" complex c_block_local?
    //
    // IMPORTANT: both alternatives start with "@".  We consume "@" once, then
    // branch on whether the next token is the "Type" keyword (block_type) or
    // something else (block_complex).  Using .or() on two parsers that each
    // start by consuming "@" would mean chumsky cannot backtrack after the
    // first "@" is consumed, causing @Foo blocks to silently fail.
    // ================================================================
    let block = tok(Token::At)
        .ignore_then(
            keyword(Keyword::Type)
                .ignore_then(c_block_type.or_not())
                .map(|body| BlockDesc::Type { body })
                .or(
                    complex_rec.clone()
                        .then(c_block_local.or_not())
                        .map(|(complex, local)| BlockDesc::Complex { complex, local })
                )
        )
        .map_with_span(|desc, span| Node::with_span(desc, dummy_span(span)));

    // ================================================================
    // Program = block*
    // ================================================================
    block.repeated()
        .map_with_span(|blocks, span| Node::with_span(ProgramDesc { blocks }, dummy_span(span)))
}
