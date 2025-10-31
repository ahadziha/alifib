<Program> ::= { <Block> }
<Block> ::= "@" "Type" [ <CBlockType> ] | "@" <Complex> [ <CBlockLocal> ]

<Complex> ::= <Address> | [ <Address> ] "{" [ <CBlock> ] "}"

<CBlockType> ::= <CInstrType> { "," <CInstrType> }
<CInstrType> ::= <GeneratorType> | <DNamer> | <MNamer> | <IncludeStatement>

<CBlock> ::= <CInstr> { "," <CInstr> }
<CInstr> ::= <Generator> | <DNamer> | <MNamer> | <IncludeStatement> | <AttachStatement>

<CBlockLocal> ::= <CInstrLocal> { "," <CInstrLocal> } 
<CInstrLocal> ::= <DNamer> | <MNamer> | <AssertStatement>

<GeneratorType> ::= <Generator> "<<=" <Complex>
<Generator> ::= <Name> [ ":" <Boundaries> ]

<Address> ::= <Name> { "." <Name> }

<Morphism> ::= <MComp> | <Morphism> "." <MComp>
<MComp> ::= <MTerm> | <Name>
<MTerm> ::= "(" "map" <MExt> "::" <Complex> ")"
<MExt> ::= [ <Morphism> ] "[" [ <MBlock> ] "]"

<MDef> ::= <Morphism> | <MExt>
<MNamer> ::= "let" <Name> "::" <Address> "=" <MDef>
<MBlock> ::= <MInstr> { "," <MInstr> }
<MInstr> ::= <Pasting> "=>" <Pasting>

<Diagram> ::= <DConcat> | <Diagram> "#" <Nat> <DConcat>
<DConcat> ::= <DExpr> | <DConcat> <DExpr>
<DExpr> ::= <DComp> | <DExpr> "." <DComp>
<DComp> ::= <MTerm> | <DTerm> | <Name> | <Bd> | "?"
<DTerm> ::= "(" <Diagram> "#" <Nat> <DConcat> ")" | "(" <DConcat> <DExpr> ")"
<Bd> ::= "in" | "out"

<Pasting> ::= <Concat> | <Pasting> "#" <Nat> <Concat>
<Concat> ::= <Expr> | <Concat> <Expr>
<Expr> ::= <DComp> | <Expr> "." <DComp>

<DNamer> ::= "let" <Name> [ ":" <Boundaries> ] "=" <Diagram>
<Boundaries> ::= <Diagram> "->" <Diagram>

<IncludeStatement> ::= "include" <Address> [ "as" <Name> ]
<AttachStatement> ::= "attach" <Name> "::" <Address> [ "along" <MDef> ]
<AssertStatement> ::= "assert" <Pasting> "=" <Pasting>

<Nat> ::= 0 | [1-9][0-9]*
<Name> ::= [A-Za-z0-9_][A-Za-z0-9_]*