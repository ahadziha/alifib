use crate::helper::positions::Span;
use crate::core::diagram::Sign as DiagramSign;

/// A located value: wraps a value together with an optional source span.
#[derive(Debug, Clone)]
pub struct Node<T> {
    pub span: Option<Span>,
    pub value: T,
}

impl<T> Node<T> {
    pub fn new(value: T) -> Self {
        Self { span: None, value }
    }
    pub fn with_span(value: T, span: Span) -> Self {
        Self { span: Some(span), value }
    }
}

pub type Name   = Node<String>;
pub type Nat    = Node<usize>;
pub type Bd     = Node<DiagramSign>;

pub type Program     = Node<ProgramDesc>;
pub type Block       = Node<BlockDesc>;
pub type Complex     = Node<ComplexDesc>;
pub type CBlockType  = Node<Vec<CInstrType>>;
pub type CBlock      = Node<Vec<CInstr>>;
pub type CBlockLocal = Node<Vec<CInstrLocal>>;
pub type CInstrType  = Node<CInstrTypeDesc>;
pub type CInstr      = Node<CInstrDesc>;
pub type CInstrLocal = Node<CInstrLocalDesc>;

pub type GeneratorType    = Node<GeneratorTypeDesc>;
pub type Generator        = Node<GeneratorDesc>;
pub type Boundaries       = Node<BoundariesDesc>;
pub type Address          = Node<Vec<Name>>;
pub type Morphism         = Node<MorphismDesc>;
pub type MComp            = Node<MCompDesc>;
pub type MTerm            = Node<MTermDesc>;
pub type MExt             = Node<MExtDesc>;
pub type MDef             = Node<MDefDesc>;
pub type MBlock           = Node<Vec<MInstr>>;
pub type MInstr           = Node<MInstrDesc>;
pub type Mnamer           = Node<MnamerDesc>;
pub type Dnamer           = Node<DnamerDesc>;
pub type IncludeStatement = Node<IncludeStatementDesc>;
pub type IncludeModule    = Node<IncludeModuleDesc>;
pub type AttachStatement  = Node<AttachStatementDesc>;
pub type AssertStatement  = Node<AssertStatementDesc>;
pub type Diagram          = Node<DiagramDesc>;
pub type DConcat          = Node<DConcatDesc>;
pub type DExpr            = Node<DExprDesc>;
pub type DComp            = Node<DCompDesc>;
pub type DTerm            = Node<DTermDesc>;
pub type Pasting          = Node<PastingDesc>;
pub type Concat           = Node<ConcatDesc>;

#[derive(Debug, Clone)]
pub struct ProgramDesc {
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub enum BlockDesc {
    Type { body: Option<CBlockType> },
    Complex { complex: Complex, local: Option<CBlockLocal> },
}

#[derive(Debug, Clone)]
pub struct ComplexDesc {
    pub address: Option<Address>,
    pub block: Option<CBlock>,
}

#[derive(Debug, Clone)]
pub enum CInstrTypeDesc {
    Generator(GeneratorType),
    Dnamer(Dnamer),
    Mnamer(Mnamer),
    IncludeModule(IncludeModule),
}

#[derive(Debug, Clone)]
pub enum CInstrDesc {
    Generator(Generator),
    Dnamer(Dnamer),
    Mnamer(Mnamer),
    Include(IncludeStatement),
    Attach(AttachStatement),
}

#[derive(Debug, Clone)]
pub enum CInstrLocalDesc {
    Dnamer(Dnamer),
    Mnamer(Mnamer),
    Assert(AssertStatement),
}

#[derive(Debug, Clone)]
pub struct GeneratorTypeDesc {
    pub generator: Generator,
    pub definition: Complex,
}

#[derive(Debug, Clone)]
pub struct GeneratorDesc {
    pub name: Name,
    pub boundaries: Option<Boundaries>,
}

#[derive(Debug, Clone)]
pub struct BoundariesDesc {
    pub source: Diagram,
    pub target: Diagram,
}

#[derive(Debug, Clone)]
pub enum MorphismDesc {
    Single(MComp),
    Concat { left: Box<Morphism>, right: MComp },
}

#[derive(Debug, Clone)]
pub enum MCompDesc {
    Term(MTerm),
    Name(Name),
}

#[derive(Debug, Clone)]
pub struct MTermDesc {
    pub ext: MExt,
    pub target: Complex,
}

#[derive(Debug, Clone)]
pub struct MExtDesc {
    pub prefix: Option<Box<Morphism>>,
    pub block: Option<MBlock>,
}

#[derive(Debug, Clone)]
pub enum MDefDesc {
    Morphism(Morphism),
    Ext(MExt),
}

#[derive(Debug, Clone)]
pub struct MInstrDesc {
    pub source: Pasting,
    pub target: Pasting,
}

#[derive(Debug, Clone)]
pub struct MnamerDesc {
    pub name: Name,
    pub address: Address,
    pub definition: MDef,
}

#[derive(Debug, Clone)]
pub struct DnamerDesc {
    pub name: Name,
    pub boundaries: Option<Boundaries>,
    pub body: Diagram,
}

#[derive(Debug, Clone)]
pub struct IncludeStatementDesc {
    pub address: Address,
    pub alias: Option<Name>,
}

#[derive(Debug, Clone)]
pub struct IncludeModuleDesc {
    pub name: Name,
    pub alias: Option<Name>,
}

#[derive(Debug, Clone)]
pub struct AttachStatementDesc {
    pub name: Name,
    pub address: Address,
    pub along: Option<MDef>,
}

#[derive(Debug, Clone)]
pub struct AssertStatementDesc {
    pub left: Pasting,
    pub right: Pasting,
}

#[derive(Debug, Clone)]
pub enum DiagramDesc {
    Single(DConcat),
    Paste { left: Box<Diagram>, nat: Nat, right: DConcat },
}

#[derive(Debug, Clone)]
pub enum DConcatDesc {
    Single(DExpr),
    Concat { left: Box<DConcat>, right: DExpr },
}

#[derive(Debug, Clone)]
pub enum DExprDesc {
    Single(DComp),
    Dot { left: Box<DExpr>, right: DComp },
}

#[derive(Debug, Clone)]
pub enum DCompDesc {
    Mterm(MTerm),
    Dterm(Box<DTerm>),
    Name(Name),
    Bd(Bd),
    Hole,
}

#[derive(Debug, Clone)]
pub enum DTermDesc {
    Indexed { diagram: Box<Diagram>, nat: Nat, tail: DConcat },
    Pair { concat: DConcat, expr: DExpr },
}

#[derive(Debug, Clone)]
pub enum PastingDesc {
    Single(Concat),
    Paste { left: Box<Pasting>, nat: Nat, right: Concat },
}

#[derive(Debug, Clone)]
pub enum ConcatDesc {
    Single(DExpr),
    Concat { left: Box<Concat>, right: DExpr },
}

/// Empty program
pub fn empty_program() -> Program {
    Node::new(ProgramDesc { blocks: vec![] })
}
