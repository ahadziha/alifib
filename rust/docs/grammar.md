-- The basic identifiers are made of alphanumeric characters

<Nat> ::= 0 | [1-9][0-9]*
<Name> ::= [A-Za-z0-9_][A-Za-z0-9_]*

-- Addresses name complexes. They are dot-qualified series of identifiers.

<Address> ::= <Name> { "." <Name> }

-- A boundary is of the form (diagram) -> (diagram), to be defined later.
-- Sometimes names come with boundaries.

<Boundary> ::= <Diagram> "->" <Diagram>
<NameWithBoundary> ::= <Name> [ ":" <Boundary> ]

-- We will often make use of local definitions of _diagrams_ and _maps_.
-- A local definition of a map _must_ come with a partial map definition. The reason
-- is that a _partial map extension_ should only be allowed when its domain is
-- explicitly requested, e.g. in a let with "::" or in an anonymous map.

<LetDiag> ::= "let" <Name> "=" <Diagram>
<DefPMap> ::= "let" <Name> "::" <Address> "=" <PMapDef>
<PMapDef> ::= <PMap> | <PMapExt>

-- A program is a (potentially empty) series of blocks

<Program> ::= { <Block> }

-- A block is either 
-- * A _type block_, followed by a body (optionally)
-- * A _local block_ (at a complex), followed by its body

<Block> ::= "@" "Type" [ <TypeBlock> ] | "@" <Complex> [ <LocalBlock> ]

-- A _type block_ is a series of type instructions:

<TypeBlock> ::= <TypeInst> { "," <TypeInst> } [ "," ]

-- A _type instruction_ adds global complexes. It can be:
-- * a generator instruction, declaring a name (optionally with a boundary) to stand for a complex
-- * a local definition of a diagram
-- * a local definition of a map
-- * an inclusion of a module

<TypeInst> ::= <Generator> | <LetDiag> | <DefPMap> | <IncludeModule>
<Generator> ::= <NameWithBoundary> "<<=" <Complex>
<IncludeModule> ::= "include" <Name> [ "as" <Name> ]

-- A _complex_ is either
-- * just an address (naming an existing complex)
-- * optionally an address, followed by a _complex block_

<Complex> ::= <Address> | [ <Address> ] "{" [ <ComplexBlock> ] "}"

-- A _complex block_ defines a new complex.
-- It consists of a series of is a series of complex instructions:

<ComplexBlock> ::= <CInstr> { "," <CInstr> }

-- A _complex instruction_ alters a block. It can be either:
-- * A name with a boundary
-- * A local definition of a diagram
-- * A local definition of a Map
-- * An attach statement (attaching a copy of a previously existing block)
-- * An include statement (making another complex a subcomplex of this one)

<CInstr> ::= <NameWithBoundary> | <LetDiag> | <DefPMap> | <AttachStmt> | <IncludeStmt>  
<IncludeStmt> ::= "include" <Address> [ "as" <Name> ]
<AttachStmt> ::= "attach" <Name> "::" <Address> [ "along" <PMapDef> ]

-- A _local block_ is a series of local instructions

<LocalBlock> ::= <LocalInst> { "," <LocalInst> } [ "," ]

-- A _local instruction_ is either
-- * a local definition of a diagram
-- * a local definition of a partial map
-- * an assertion that two pastings are equal

<LocalInst> ::= <LetDiag> | <DefPMap> | <AssertStmt>
<AssertStmt> ::= "assert" <Diagram> "=" <Diagram>

-- a diagram is either
-- * a concatenation of explicit pastings
-- * each of which is a concatenation of implicit pastings
-- * each part of which is a dotted series of components
-- * which are either names, partial maps, boundaries (+ or -), (diagrams), or holes
<Diagram> ::= <DPrincipal> | <Diagram> "#" <Nat> <DPrincipal>
<DPrincipal> ::= <DExpr> | <DPrincipal> <DExpr>
<DExpr> ::= <DComponent> | <DExpr> "." <DComponent>
<DComponent> ::= <PMapBasic> | <Bd> | "(" <Diagram> ")" | "?"
<Bd> ::= "in" | "out"

-- A general _partial map_ is a dotted sequence of basic partial maps.
-- A basic partial map is either
-- * a name
-- * a parenthesized partial map
-- * an anonymous map, which is necessarily an extension

<PMap> ::= <PMapBasic> | <PMap> "." <PMapBasic>
<PMapBasic> ::= <Name> | <AnonMap> | "(" <PMap> ")"

-- An extension is (optionally) a partial map followed by a number of clauses
-- that extend it.

<PMapExt> ::= [ <PMap> ] "[" <PMapClauses> "]"
<PMapClauses> ::= <PMapClause> { "," <PMapClause> }
<PMapClause> ::= <Diagram> "=>" <Diagram>