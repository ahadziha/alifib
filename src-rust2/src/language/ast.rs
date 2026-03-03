#![allow(dead_code)]

pub use chumsky::span::{SimpleSpan, Spanned};

pub type Span = SimpleSpan;

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
    DefPMap(DefPMap),
    IncludeModule(IncludeModule),
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
        body: Vec<Spanned<CInstr>>,
    },
}

// ---------------------------------------------------------------------------
// Complex instructions
// ---------------------------------------------------------------------------

pub enum CInstr {
    NameWithBoundary(NameWithBoundary),
    LetDiag(LetDiag),
    DefPMap(DefPMap),
    AttachStmt(AttachStmt),
    IncludeStmt(IncludeStmt),
}

pub struct AttachStmt {
    pub name: Spanned<String>,
    pub address: Spanned<Address>,
    pub along: Option<Spanned<PMap>>,
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
    DefPMap(DefPMap),
    AssertStmt(AssertStmt),
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
    pub source: Spanned<Diagram>,
    pub target: Spanned<Diagram>,
}

pub struct LetDiag {
    pub name: Spanned<String>,
    pub boundary: Option<Spanned<Boundary>>,
    pub value: Spanned<Diagram>,
}

pub struct DefPMap {
    pub name: Spanned<String>,
    pub address: Spanned<Address>,
    pub value: Spanned<PMap>,
}

// ---------------------------------------------------------------------------
// Diagrams
// ---------------------------------------------------------------------------

pub enum Diagram {
    /// Implicit pasting (juxtaposition)
    Principal(Vec<Spanned<DExpr>>),
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
    In,
    Out,
    Paren(Box<Spanned<Diagram>>),
    Hole,
}

// ---------------------------------------------------------------------------
// Partial maps
// ---------------------------------------------------------------------------

pub enum PMap {
    Basic(PMapBasic),
    Dot {
        base: PMapBasic,
        rest: Box<Spanned<PMap>>,
    },
}

pub enum PMapBasic {
    Name(String),
    System(PMSystem),
}

pub struct PMSystem {
    pub extend: Option<Box<Spanned<PMap>>>,
    pub clauses: Vec<Spanned<PMapClause>>,
}

pub struct PMapClause {
    pub lhs: Spanned<Diagram>,
    pub rhs: Spanned<Diagram>,
}
