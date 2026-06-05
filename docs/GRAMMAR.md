# The alifib grammar

The concrete grammar of the `.ali` language, in EBNF. This is the reference for
the surface syntax; for what the constructs *mean*, see the
[README](../README.md) and [`CONCEPTS.md`](CONCEPTS.md).

**Notation.** `::=` defines a nonterminal; `<X>` is a nonterminal; `"x"` is a
literal token; `{ x }` is zero or more repetitions; `[ x ]` is optional; `|` is
alternation; `( … )` groups; and regex-style character classes (e.g.
`[A-Za-z0-9_]`) describe lexical tokens.

## Lexical tokens

Identifiers are made of alphanumeric characters and underscores; naturals are
unsigned. Comments are delimited by `(* … *)` and may be nested.

```ebnf
<Nat>  ::= 0 | [1-9][0-9]*
<Name> ::= [A-Za-z0-9_][A-Za-z0-9_]*
```

## Addresses and boundaries

**Addresses** name complexes; they are dot-qualified series of identifiers. A
**boundary** is of the form `(diagram) -> (diagram)` (the `<Diagram>` nonterminal
is defined [below](#diagrams)). Names sometimes come with a boundary.

```ebnf
<Address>          ::= <Name> { "." <Name> }
<Boundary>         ::= <Diagram> "->" <Diagram>
<NameWithBoundary> ::= <Name> [ ":" <Boundary> ]
```

## Local definitions

We often make use of local definitions of *diagrams* and *maps*. A local
definition of a map *must* come with a partial-map definition: a partial-map
*extension* is only allowed when its domain is explicitly requested — e.g. in a
`let` with `::`, or in an anonymous map. The optional `total` keyword asserts
that the map is total.

```ebnf
<LetDiag> ::= "let" <Name> "=" <Diagram>
<DefPMap> ::= "let" ["total"] <Name> "::" <Address> "=" <PMapDef>
<PMapDef> ::= <PMap> | <PMapExt>
```

## Programs and blocks

A **program** is a (possibly empty) series of blocks. A **block** is either a
*type block* (introduced by `@Type`) or a *local block* at a complex (introduced
by `@<Complex>`), each optionally followed by a body.

```ebnf
<Program> ::= { <Block> }
<Block>   ::= "@" "Type" [ <TypeBlock> ] | "@" <Complex> [ <LocalBlock> ]
```

## Type blocks

A **type block** is a series of type instructions. A **type instruction** adds
global complexes; it can be a *generator* (declaring a name, optionally with a
boundary, to stand for a complex), a local definition of a diagram or a map, an
inclusion of a module, an index declaration, or a for-block.

```ebnf
<TypeBlock>     ::= <TypeInst> { "," <TypeInst> } [ "," ]
<TypeInst>      ::= <Generator> | <LetDiag> | <DefPMap> | <IncludeModule>
                  | <IndexDecl> | <ForBlock>
<Generator>     ::= <NameWithBoundary> "<<=" <Complex>
<IncludeModule> ::= "include" <Name> [ "as" <Name> ]
```

## Complexes

A **complex** is either just an address (naming an existing complex) or an
optional address followed by a *complex block* that defines a new complex. A
**complex instruction** can be a name with a boundary, a local definition of a
diagram or map, an `attach` statement (attaching a copy of a previously existing
complex), an `include` statement (making another complex a subcomplex of this
one), an index declaration, or a for-block.

```ebnf
<Complex>      ::= <Address> | [ <Address> ] "{" [ <ComplexBlock> ] "}"
<ComplexBlock> ::= <CInstr> { "," <CInstr> } [ "," ]
<CInstr>       ::= <NameWithBoundary> | <LetDiag> | <DefPMap> | <AttachStmt>
                 | <IncludeStmt> | <IndexDecl> | <ForBlock>
<IncludeStmt>  ::= "include" <Address> [ "as" <Name> ]
<AttachStmt>   ::= "attach" <Name> "::" <Address> [ "along" <PMapDef> ]
```

## Local blocks

A **local block** is a series of local instructions: a local definition of a
diagram or a partial map, an assertion that two pastings are equal, an index
declaration, or a for-block.

```ebnf
<LocalBlock> ::= <LocalInst> { "," <LocalInst> } [ "," ]
<LocalInst>  ::= <LetDiag> | <DefPMap> | <AssertStmt> | <IndexDecl> | <ForBlock>
<AssertStmt> ::= "assert" <Diagram> "=" <Diagram>
```

## Indices and for-blocks

Index declarations and for-blocks provide string templating. An **index** is a
named list of strings; a **for-block** expands its body once per index value,
substituting `<var>` with each value.

```ebnf
<IndexValue> ::= <Name>
<IndexList>  ::= "[" <IndexValue> { "," <IndexValue> } [ "," ] "]"
<IndexDecl>  ::= "index" <Name> "=" <IndexList>
<ForBlock>   ::= "for" <Name> "in" ( <Name> | <IndexList> ) "{" <ForBody> "}"
```

`<ForBody>` is raw source text (with balanced braces); occurrences of a `<Name>`
delimited by `<` `>` are replaced with the current index value. For-blocks and
index declarations may appear in type, complex, local, and partial-map blocks.

## Diagrams

A **diagram** is a `#`-separated series of *explicit pastings* — `… # k …`
pastes along a shared `k`-boundary — each of which is a juxtaposition of
*principal pastings*, each in turn a dotted series of components. A component is
a name, an anonymous map, a `run` expression, a boundary destructor (`in` /
`out`), or a parenthesised diagram.

A hole `?` is **not** a component: it may appear only as the whole right-hand
side of a partial-map clause (see [`<PMapClause>`](#maps) below).

```ebnf
<Diagram>    ::= <DPrincipal> | <Diagram> "#" <Nat> <DPrincipal>
<DPrincipal> ::= <DExpr> | <DPrincipal> <DExpr>
<DExpr>      ::= <DComponent> | <DExpr> "." <DComponent>
<DComponent> ::= <Name> | <AnonMap> | <RunExpr> | <Bd> | "(" <Diagram> ")"
<Bd>         ::= "in" | "out"
<RunExpr>    ::= "(" "run" <Strategy> "on" <Diagram> ")"
<Strategy>   ::= "auto"
```

## Maps

A general **partial map** is a dotted sequence of basic partial maps; a basic
partial map is a name, a parenthesised partial map, or an anonymous map. An
**anonymous map** is a partial-map definition with an explicit target type,
enclosed in parentheses.

```ebnf
<PMap>      ::= <PMapBasic> | <PMap> "." <PMapBasic>
<PMapBasic> ::= <Name> | <AnonMap> | "(" <PMap> ")"
<AnonMap>   ::= "(" "map" <PMapDef> "::" <Complex> ")"
```

An **extension** is an optional partial map followed by a number of clauses that
extend it. A **clause** names the image of a diagram, or leaves it open with a
bare hole `?`.

```ebnf
<PMapExt>     ::= [ <PMap> ] "[" <PMapEntries> "]"
<PMapEntries> ::= <PMapEntry> { "," <PMapEntry> } [ "," ]
<PMapEntry>   ::= <PMapClause> | <ForBlock>
<PMapClause>  ::= <Diagram> "=>" <Diagram> | <Diagram> "=>" "?"
```
