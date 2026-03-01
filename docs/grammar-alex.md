-- the basic identifiers are made of alphanumeric characters

<Nat> ::= 0 | [1-9][0-9]*
<Name> ::= [A-Za-z0-9_][A-Za-z0-9_]*

-- addresses name complexes
-- they are dot-qualified series of identifiers

<Address> ::= <Name> { "." <Name> }

-- a program is a series of blocks
<Program> ::= { <Block> }

-- a block is either 
-- * a declaration of a type block, followed by an optional type block body
-- * a declaration of a complex block, followed by an optional local block body
<Block> ::= "@" "Type" [ <TypeBlock> ] | "@" <Complex> [ <LocalBlock> ]

-- a type block is a series of type instructions, i.e.
-- * a generator instruction, assigning a complex to a name
-- * a local definition of a diagram
-- * a local definition of a morphism of diagrams
-- * an inclusion of a module

<TypeBlock> ::= <TypeInst> { "," <TypeInst> }
<TypeInst> ::= <Generator> | <LetDiag> | <LetMor> | <IncludeModule>
<Generator> ::= <NameWithBoundary> "<<=" <Complex>
<NameWithBoundary> ::= <Name> [ ":" <Boundary> ]
<IncludeModule> ::= "include" <Name> [ "as" <Name> ]

-- a complex is either
-- * just an address (naming an existing complex)
-- * optionally an address, followed by a complex block

<Complex> ::= <Address> | [ <Address> ] "{" [ <CBlock> ] "}"

-- a complex block is a series of complex instructions, i.e.
-- * a name with a boundary
-- * a local definition of a diagram
-- * a local definition of a morphism
-- * an include statement
-- * an attach statement

<CBlock> ::= <CInstr> { "," <CInstr> }
<CInstr> ::= <NameWithBoundary> | <LetDiag> | <LetMor> | <IncludeStmt> | <AttachStmt>
<IncludeStmt> ::= "include" <Address> [ "as" <Name> ]
<AttachStmt> ::= "attach" <Name> "::" <Address> [ "along" <MDef> ]

-- a local block is a series of local instructions
-- a local instruction is either
-- * a local definition of a diagram
-- * a local definition of a morphism
-- * an assertion that two pastings are equal

<LocalBlock> ::= <LocalInst> { "," <LocalInst> } 
<LocalInst> ::= <LetDiag> | <LetMor> | <AssertStmt>
<LetDiag> ::= "let" <Name> [ ":" <Boundary> ] "=" <Diagram>
<AssertStmt> ::= "assert" <Pasting> "=" <Pasting>

-- a diagram is either
-- * a concatenation of expressions
-- * a diagram composed along a dimension with a concatenation of expressions
<Diagram> ::= <DConcat> | <Diagram> "#" <Nat> <DConcat>
<DConcat> ::= <DExpr> | <DConcat> <DExpr>
<DExpr> ::= <DComp> | <DExpr> "." <DComp>
<DComp> ::= <MTerm> | <DTerm> | <Name> | <Bd> | "?"
<DTerm> ::= "(" <Diagram> "#" <Nat> <DConcat> ")" | "(" <DConcat> <DExpr> ")"
<Bd> ::= "in" | "out"

<Pasting> ::= <Concat> | <Pasting> "#" <Nat> <Concat>
<Concat> ::= <DExpr> | <Concat> <DExpr>

<Boundary> ::= <Diagram> "->" <Diagram>


-- a morphism is either
-- * 
<Morphism> ::= <MComp> | <Morphism> "." <MComp>
<MComp> ::= <MTerm> | <Name>
<MTerm> ::= "(" "map" <MExt> "::" <Complex> ")"
<MExt> ::= [ <Morphism> ] "[" [ <MBlock> ] "]"
<MBlock> ::= <MInstr> { "," <MInstr> }
<MInstr> ::= <Pasting> "=>" <Pasting>

<LetMor> ::= "let" <Name> "::" <Address> "=" <MDef>
<MDef> ::= <Morphism> | <MExt>

