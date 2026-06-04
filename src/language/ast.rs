#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    /// A synthetic span with no source location, for programmatically constructed AST nodes.
    pub fn synthetic() -> Self {
        Self { start: 0, end: 0 }
    }
}

#[derive(Clone, Debug)]
pub struct Spanned<T> {
    pub inner: T,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Program & Blocks
// ---------------------------------------------------------------------------

pub struct Program {
    pub blocks: Vec<Spanned<Block>>,
}

pub enum Block {
    TypeBlock(Vec<Spanned<TypeInst>>),
    LocalBlock {
        complex: Spanned<Complex>,
        body: Vec<Spanned<LocalInst>>,
    },
}

// ---------------------------------------------------------------------------
// Type-level instructions
// ---------------------------------------------------------------------------

pub enum TypeInst {
    Generator(Generator),
    LetDiag(LetDiag),
    DefPartialMap(DefPartialMap),
    IncludeModule(IncludeModule),
    Index(IndexDecl),
    For(ForBlock),
}

pub struct Generator {
    pub name: Spanned<NameWithBoundary>,
    pub complex: Spanned<Complex>,
}

pub struct IncludeModule {
    pub name: Spanned<String>,
    pub alias: Option<Spanned<String>>,
}

// ---------------------------------------------------------------------------
// Complex
// ---------------------------------------------------------------------------

pub enum Complex {
    Address(Address),
    Block {
        address: Option<Address>,
        body: Vec<Spanned<ComplexInstr>>,
    },
}

// ---------------------------------------------------------------------------
// Complex instructions
// ---------------------------------------------------------------------------

pub enum ComplexInstr {
    NameWithBoundary(NameWithBoundary),
    LetDiag(LetDiag),
    DefPartialMap(DefPartialMap),
    AttachStmt(AttachStmt),
    IncludeStmt(IncludeStmt),
    Index(IndexDecl),
    For(ForBlock),
}

pub struct AttachStmt {
    pub name: Spanned<String>,
    pub address: Spanned<Address>,
    pub along: Option<Spanned<PartialMapDef>>,
}

pub struct IncludeStmt {
    pub address: Spanned<Address>,
    pub alias: Option<Spanned<String>>,
}

// ---------------------------------------------------------------------------
// Local instructions
// ---------------------------------------------------------------------------

pub enum LocalInst {
    LetDiag(LetDiag),
    DefPartialMap(DefPartialMap),
    AssertStmt(AssertStmt),
    Index(IndexDecl),
    For(ForBlock),
}

pub struct AssertStmt {
    pub lhs: Spanned<Diagram>,
    pub rhs: Spanned<Diagram>,
}

// ---------------------------------------------------------------------------
// Shared: names, boundaries, let/def
// ---------------------------------------------------------------------------

pub type Address = Vec<Spanned<String>>;

pub struct NameWithBoundary {
    pub name: Spanned<String>,
    pub boundary: Option<Spanned<Boundary>>,
}

pub struct Boundary {
    pub input: Spanned<Diagram>,
    pub output: Spanned<Diagram>,
}

pub struct LetDiag {
    pub name: Spanned<String>,
    pub value: Spanned<Diagram>,
}

pub struct DefPartialMap {
    pub total: bool,
    pub name: Spanned<String>,
    pub address: Spanned<Address>,
    pub value: Spanned<PartialMapDef>,
}

// ---------------------------------------------------------------------------
// Diagrams
// ---------------------------------------------------------------------------

pub enum Strategy {
    Auto,
}

pub enum Diagram {
    /// Implicit pasting: a principal sequence of diagram expressions (no explicit #n).
    PrincipalPaste(Vec<Spanned<DExpr>>),
    /// Explicit pasting: lhs #n rhs
    Paste {
        lhs: Box<Spanned<Diagram>>,
        dim: Spanned<String>,
        rhs: Vec<Spanned<DExpr>>,
    },
}

pub enum DExpr {
    /// A single component
    Component(DComponent),
    /// Dotted access: expr.component
    Dot {
        base: Box<Spanned<DExpr>>,
        field: Spanned<DComponent>,
    },
}

pub enum DComponent {
    Name(String),
    AnonMap {
        def: Box<Spanned<PartialMapDef>>,
        target: Spanned<Complex>,
    },
    In,
    Out,
    Paren(Box<Spanned<Diagram>>),
    Run {
        strategy: Spanned<Strategy>,
        diagram: Box<Spanned<Diagram>>,
    },
}

impl DExpr {
    /// The dotted generator name this expression denotes, if it is exactly a
    /// (possibly qualified) name — `r`, `Sub.arr` — and nothing more.  This is
    /// the canonical form a generator is keyed and rendered by, and so the
    /// left-hand side a `done` assignment writes.
    pub fn dotted_name(&self) -> Option<String> {
        match self {
            DExpr::Component(DComponent::Name(s)) => Some(s.clone()),
            DExpr::Dot { base, field } => {
                let prefix = base.inner.dotted_name()?;
                match &field.inner {
                    DComponent::Name(s) => Some(format!("{}.{}", prefix, s)),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Partial maps
// ---------------------------------------------------------------------------

pub enum PartialMapDef {
    PartialMap(PartialMap),
    Ext(PartialMapExt),
}

pub struct PartialMapExt {
    pub prefix: Option<Box<Spanned<PartialMap>>>,
    pub clauses: Vec<Spanned<PMapEntry>>,
}

pub enum PartialMap {
    Basic(PartialMapBasic),
    Dot {
        base: PartialMapBasic,
        rest: Box<Spanned<PartialMap>>,
    },
}

pub enum PartialMapBasic {
    Name(String),
    AnonMap {
        def: Box<Spanned<PartialMapDef>>,
        target: Spanned<Complex>,
    },
    Paren(Box<Spanned<PartialMap>>),
}

pub struct PartialMapClause {
    pub lhs: Spanned<Diagram>,
    pub rhs: ClauseRhs,
}

/// The right-hand side of a partial-map clause: either a diagram naming the
/// image, or a bare hole `?` declining to.  A hole is *only* legal here — it is
/// not a diagram component — so it carries just the span of its `?` token.
pub enum ClauseRhs {
    Diagram(Spanned<Diagram>),
    Hole(Span),
}

pub enum PMapEntry {
    Clause(PartialMapClause),
    For(ForBlock),
}

// ---------------------------------------------------------------------------
// Index & For (string templating)
// ---------------------------------------------------------------------------

pub struct IndexDecl {
    pub name: Spanned<String>,
    pub values: Vec<Spanned<String>>,
}

pub struct ForBlock {
    pub variable: Spanned<String>,
    pub index: ForIndex,
    pub exclude: Option<ForIndex>,
    pub body_span: Span,
    pub body_text: String,
}

pub enum ForIndex {
    Named(Spanned<String>),
    Inline(Vec<Spanned<String>>),
}
