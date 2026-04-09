/// Programmatic code generation for alifib programs.
///
/// Plugins use this module to build alifib type definitions without touching
/// internal AST types. All types here are opaque; conversion to `ast::Program`
/// and interpretation happen inside the library.
use crate::interpreter::{Context, InterpResult, interpret_program};
use crate::language::ast;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn syn<T>(inner: T) -> ast::Spanned<T> {
    ast::Spanned { inner, span: ast::Span::synthetic() }
}

// ---------------------------------------------------------------------------
// DiagRepr — internal clonable representation of a diagram expression
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum DiagRepr {
    Cell(String),
    /// Vertical (principal) composition: elements are space-joined.
    /// Compound elements are parenthesised when emitted.
    Seq(Vec<DiagRepr>),
    /// Horizontal composition: `lhs #0 rhs`.
    Par(Box<DiagRepr>, Box<DiagRepr>),
}

/// Convert a `DiagRepr` to the internal `ast::Diagram`.
fn repr_to_ast(d: DiagRepr) -> ast::Diagram {
    match d {
        DiagRepr::Cell(name) => ast::Diagram::PrincipalPaste(vec![syn(
            ast::DExpr::Component(ast::DComponent::PartialMap(
                ast::PartialMapBasic::Name(name),
            )),
        )]),
        DiagRepr::Seq(parts) => ast::Diagram::PrincipalPaste(
            parts.into_iter().map(|p| syn(repr_to_dexpr(p))).collect(),
        ),
        DiagRepr::Par(lhs, rhs) => ast::Diagram::Paste {
            lhs: Box::new(syn(repr_to_ast(*lhs))),
            dim: syn("0".to_owned()),
            rhs: vec![syn(repr_to_dexpr(*rhs))],
        },
    }
}

/// Convert a `DiagRepr` to a single `ast::DExpr`, parenthesising if compound.
fn repr_to_dexpr(d: DiagRepr) -> ast::DExpr {
    match d {
        DiagRepr::Cell(name) => ast::DExpr::Component(ast::DComponent::PartialMap(
            ast::PartialMapBasic::Name(name),
        )),
        other => ast::DExpr::Component(ast::DComponent::Paren(Box::new(syn(repr_to_ast(other))))),
    }
}

/// Serialise a `DiagRepr` directly to `.ali` text (for `to_ali`).
fn repr_to_str(d: &DiagRepr) -> String {
    match d {
        DiagRepr::Cell(name) => name.clone(),
        DiagRepr::Seq(parts) => parts.iter().map(repr_to_dexpr_str).collect::<Vec<_>>().join(" "),
        DiagRepr::Par(lhs, rhs) => format!("{} #0 {}", repr_to_str(lhs), repr_to_dexpr_str(rhs)),
    }
}

/// Serialise as a single token — wrap in parens if compound.
fn repr_to_dexpr_str(d: &DiagRepr) -> String {
    match d {
        DiagRepr::Cell(name) => name.clone(),
        other => format!("({})", repr_to_str(other)),
    }
}

// ---------------------------------------------------------------------------
// Public Diag type
// ---------------------------------------------------------------------------

/// An opaque diagram expression.
///
/// Build one with [`Diag::cell`], compose with [`Diag::then`] / [`Diag::par`],
/// or use the free functions [`seq`], [`seq_flat`], [`par_seq`], [`obs`].
#[derive(Clone, Debug)]
pub struct Diag(DiagRepr);

impl Diag {
    /// A single named cell (generator reference).
    pub fn cell(name: &str) -> Self {
        Diag(DiagRepr::Cell(name.to_owned()))
    }

    /// Vertical composition: `self other` (principal paste).
    ///
    /// If `self` is already a sequence, `other` is appended to it.
    /// `other` is parenthesised during emission if it is compound.
    pub fn then(self, other: Self) -> Self {
        let mut parts = match self.0 {
            DiagRepr::Seq(v) => v,
            single => vec![single],
        };
        parts.push(other.0);
        Diag(DiagRepr::Seq(parts))
    }

    /// Horizontal composition: `self #0 other`.
    pub fn par(self, other: Self) -> Self {
        Diag(DiagRepr::Par(Box::new(self.0), Box::new(other.0)))
    }

    /// Whether this diagram is a bare cell with the given name.
    pub fn is_cell(&self, name: &str) -> bool {
        matches!(&self.0, DiagRepr::Cell(n) if n == name)
    }
}

// ---------------------------------------------------------------------------
// Free composition helpers
// ---------------------------------------------------------------------------

/// Vertical sequence: fold a non-empty iterator with [`Diag::then`].
///
/// Each element after the first is parenthesised if compound, giving
/// `a b (c d) e ...`.
pub fn seq(parts: impl IntoIterator<Item = Diag>) -> Diag {
    parts
        .into_iter()
        .reduce(|a, b| a.then(b))
        .expect("seq: empty iterator")
}

/// Flat vertical sequence: all elements are placed at the same level with no
/// extra parentheses between them. Use this when the elements are already
/// individually correct and should be treated as peers.
pub fn seq_flat(parts: impl IntoIterator<Item = Diag>) -> Diag {
    let mut all = Vec::new();
    for d in parts {
        match d.0 {
            DiagRepr::Seq(v) => all.extend(v),
            other => all.push(other),
        }
    }
    Diag(DiagRepr::Seq(all))
}

/// Horizontal sequence: fold a non-empty iterator with [`Diag::par`].
///
/// Left-associative: `a #0 b #0 c` = `(a #0 b) #0 c`.
pub fn par_seq(parts: impl IntoIterator<Item = Diag>) -> Diag {
    parts
        .into_iter()
        .reduce(|a, b| a.par(b))
        .expect("par_seq: empty iterator")
}

/// `n` copies of `ob` composed vertically: `ob ob ... ob`.
pub fn obs(n: usize) -> Diag {
    seq((0..n).map(|_| Diag::cell("ob")))
}

/// If `pieces` has one element return it directly; otherwise compose with
/// `compose` and return the result (which will be parenthesised automatically
/// when embedded in a larger sequence).
pub fn compose_or_single(
    pieces: Vec<Diag>,
    compose: impl FnOnce(Vec<Diag>) -> Diag,
) -> Diag {
    if pieces.len() == 1 {
        pieces.into_iter().next().unwrap()
    } else {
        compose(pieces)
    }
}

// ---------------------------------------------------------------------------
// InstrRepr — internal instruction representation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum InstrRepr {
    Gen {
        name: String,
        src: Option<DiagRepr>,
        tgt: Option<DiagRepr>,
    },
    Attach {
        name: String,
        type_path: Vec<String>,
        map: Vec<(String, DiagRepr)>,
    },
}

fn instr_to_ast(instr: InstrRepr) -> ast::Spanned<ast::ComplexInstr> {
    match instr {
        InstrRepr::Gen { name, src, tgt } => {
            syn(ast::ComplexInstr::NameWithBoundary(ast::NameWithBoundary {
                name: syn(name),
                boundary: match (src, tgt) {
                    (Some(s), Some(t)) => Some(syn(ast::Boundary {
                        source: syn(repr_to_ast(s)),
                        target: syn(repr_to_ast(t)),
                    })),
                    _ => None,
                },
            }))
        }
        InstrRepr::Attach { name, type_path, map } => {
            let address: ast::Address = type_path.into_iter().map(syn).collect();
            let clauses = map
                .into_iter()
                .map(|(gen_name, val)| {
                    syn(ast::PartialMapClause {
                        lhs: syn(repr_to_ast(DiagRepr::Cell(gen_name))),
                        rhs: syn(repr_to_ast(val)),
                    })
                })
                .collect();
            syn(ast::ComplexInstr::AttachStmt(ast::AttachStmt {
                name: syn(name),
                address: syn(address),
                along: Some(syn(ast::PartialMapDef::Ext(ast::PartialMapExt {
                    prefix: None,
                    clauses,
                }))),
            }))
        }
    }
}

fn instr_to_str(instr: &InstrRepr, indent: &str) -> String {
    match instr {
        InstrRepr::Gen { name, src: Some(s), tgt: Some(t) } => {
            format!("{}{} : {} -> {}", indent, name, repr_to_str(s), repr_to_str(t))
        }
        InstrRepr::Gen { name, .. } => format!("{}{}", indent, name),
        InstrRepr::Attach { name, type_path, map } => {
            let path = type_path.join(".");
            let clauses: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{} => {}", k, repr_to_str(v)))
                .collect();
            format!(
                "{}attach {} :: {} along [ {} ]",
                indent,
                name,
                path,
                clauses.join(", ")
            )
        }
    }
}

// ---------------------------------------------------------------------------
// TypeDef
// ---------------------------------------------------------------------------

/// A type block definition, built with a fluent builder API.
#[derive(Clone, Debug)]
pub struct TypeDef {
    name: String,
    body: Vec<InstrRepr>,
}

impl TypeDef {
    pub fn new(name: &str) -> Self {
        TypeDef { name: name.to_owned(), body: Vec::new() }
    }

    /// Declare a 0-dimensional generator (no boundary).
    pub fn cell(mut self, name: &str) -> Self {
        self.body.push(InstrRepr::Gen { name: name.to_owned(), src: None, tgt: None });
        self
    }

    /// Declare a generator with source and target diagrams.
    pub fn cell_bd(mut self, name: &str, src: Diag, tgt: Diag) -> Self {
        self.body.push(InstrRepr::Gen {
            name: name.to_owned(),
            src: Some(src.0),
            tgt: Some(tgt.0),
        });
        self
    }

    /// Attach a partial map: `attach name :: type_path along [ gen => diag, ... ]`.
    pub fn attach(mut self, name: &str, type_path: &[&str], map: Vec<(&str, Diag)>) -> Self {
        self.body.push(InstrRepr::Attach {
            name: name.to_owned(),
            type_path: type_path.iter().map(|s| s.to_string()).collect(),
            map: map.into_iter().map(|(k, v)| (k.to_owned(), v.0)).collect(),
        });
        self
    }

    fn into_ast(self) -> ast::Spanned<ast::TypeInst> {
        let body = self.body.into_iter().map(instr_to_ast).collect();
        syn(ast::TypeInst::Generator(ast::Generator {
            name: syn(ast::NameWithBoundary {
                name: syn(self.name),
                boundary: None,
            }),
            complex: syn(ast::Complex::Block { address: None, body }),
        }))
    }

    fn to_str(&self) -> String {
        let mut out = format!("{} <<= {{\n", self.name);
        for (i, instr) in self.body.iter().enumerate() {
            out.push_str(&instr_to_str(instr, "  "));
            if i + 1 < self.body.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push('}');
        out
    }
}

// ---------------------------------------------------------------------------
// Program
// ---------------------------------------------------------------------------

/// A complete alifib program, built from [`TypeDef`]s.
#[derive(Clone, Debug)]
pub struct Program {
    types: Vec<TypeDef>,
}

impl Program {
    pub fn new() -> Self {
        Program { types: Vec::new() }
    }

    /// Add a type block.
    pub fn type_def(mut self, t: TypeDef) -> Self {
        self.types.push(t);
        self
    }

    /// Interpret the program with the given context.
    pub fn interpret(self, context: Context) -> InterpResult {
        let ast = self.into_ast();
        interpret_program(context, &ast)
    }

    /// Serialise to `.ali` source text using the round-trip pretty-printer.
    ///
    /// Output is guaranteed to re-parse to an equivalent program.
    /// Prefer this over [`to_ali`](Self::to_ali) when the output will be read by humans or
    /// fed back into the parser.
    pub fn print_ali(&self) -> String {
        crate::language::print_program(&self.clone().into_ast())
    }

    /// Serialise to `.ali` source text (useful for debugging and snapshot tests).
    pub fn to_ali(&self) -> String {
        let mut out = String::from("@Type\n");
        for (i, td) in self.types.iter().enumerate() {
            if i > 0 {
                out.push_str(",\n\n");
            }
            out.push_str(&td.to_str());
        }
        out.push('\n');
        out
    }

    fn into_ast(self) -> ast::Program {
        let type_block = syn(ast::Block::TypeBlock(
            self.types.into_iter().map(TypeDef::into_ast).collect(),
        ));
        ast::Program { blocks: vec![type_block] }
    }
}

impl Default for Program {
    fn default() -> Self {
        Self::new()
    }
}
