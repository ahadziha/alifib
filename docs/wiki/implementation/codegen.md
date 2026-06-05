---
kind: impl
status: stable
last-touched: 2026-06-05
code: [src/codegen.rs]
---

# codegen — fluent builders for programmatic ASTs

> A plugin wants to *emit* alifib, not parse it. `codegen` is the back door: a
> handful of opaque builder types that assemble an `ast::Program` from Rust,
> then either interpret it directly or pretty-print it to `.ali` source. It
> hides every internal AST type behind three structs and a clutch of free
> functions.

The only consumer in-tree is the `trs` plugin
(`plugins/trs/src/{generate,encode}.rs`), which compiles term-rewriting systems
into alifib type blocks. Nothing in the core library depends on `codegen`; it is
a one-way bridge *out* of Rust *into* the language.

## What it owns

The construction of well-formed `ast::Program` values without exposing the AST.
Callers never see `Spanned`, `DExpr`, `ComplexInstr`, or spans — they manipulate
`Diag`, `TypeDef`, and `Program`, whose internal reprs (`DiagRepr`, `InstrRepr`)
are private. The module also owns the *emission* of those reprs both as AST
(`repr_to_ast`, `instr_to_ast`, `into_ast`) and as `.ali` text
(`repr_to_str`, `instr_to_str`, `to_str`, `to_ali`).

Everything synthetic is spanless: `codegen::syn` (internal) wraps any value in
`ast::Spanned` with `ast::Span::synthetic()`. Generated programs carry no source
positions, so diagnostics from interpreting them point nowhere — acceptable,
because the program was machine-built.

## Key public types

| Type / fn | Role |
|---|---|
| `Diag` | opaque diagram expression; wraps a private `DiagRepr` |
| `Diag::cell` / `then` / `par` / `is_cell` | single generator, vertical paste, horizontal paste $\#_0$, name probe |
| `seq` / `seq_flat` / `par_seq` / `obs` / `compose_or_single` | free combinators over `Diag` |
| `TypeDef` | one type block, built by chaining `cell` / `cell_bd` / `attach` |
| `Program` | a whole program; `type_def` adds blocks; `interpret` / `print_ali` / `to_ali` are the exits |

`Diag` carries the three shapes of `DiagRepr` (internal): `Cell(name)` a bare
generator reference; `Seq(parts)` vertical/principal paste (elements space-joined,
compound ones parenthesised); `Par(lhs, rhs)` horizontal paste emitted as
`lhs #0 rhs`.

`TypeDef`'s body is a `Vec<InstrRepr>` (internal) with two variants:
`Gen { name, input, output }` — a generator, with an optional input/output
[[boundary]] pair — and `Attach { name, type_path, map }` — an `attach … along`
[[partial-map]] clause set.

## Data flow

```
Rust calls                 internal repr            exit
──────────                 ─────────────            ────
Diag::cell("ob")           DiagRepr::Cell
  .then(..).par(..)        DiagRepr::Seq / Par

TypeDef::new("M")          InstrRepr::Gen
  .cell / .cell_bd         InstrRepr::Attach
  .attach

Program::new()             Vec<TypeDef>
  .type_def(td)
        │
        ├── interpret(ctx) ─▶ into_ast ─▶ ast::Program ─▶ interpret_program
        ├── print_ali()    ─▶ into_ast ─▶ language::print_program  (round-trip)
        └── to_ali()       ─▶ to_str    ─▶ hand-rolled @Type text  (debug)
```

Two independent emission paths converge on the same reprs:

- **AST path.** `Program::into_ast` (internal) wraps the `TypeDef`s in a single
  `ast::Block::TypeBlock`. Each `TypeDef::into_ast` becomes an
  `ast::TypeInst::Generator` whose `complex` is an `ast::Complex::Block` holding
  the body. `instr_to_ast` turns a `Gen` into `ComplexInstr::NameWithBoundary`
  (boundary present only when *both* `input` and `output` are `Some`) and an `Attach`
  into `ComplexInstr::AttachStmt` with a `PartialMapDef::Ext` of `PMapEntry::Clause`s.
  `repr_to_ast` lowers a `DiagRepr`: `Cell`/`Seq` become `Diagram::PrincipalPaste`,
  `Par` becomes `Diagram::Paste { dim: "0", … }`.
- **Text path.** `to_str` / `instr_to_str` / `repr_to_str` print the same reprs
  as `.ali` directly, never touching the AST. `Program::to_ali` opens with a
  literal `@Type` header and joins type blocks with `,\n\n`; each block is
  `name <<= { … }`.

`Program::print_ali` is the *third* exit and the recommended one: it routes
through `into_ast` and then `language::print_program` (`src/language/ast_print.rs`),
the round-trip pretty-printer whose output is guaranteed to re-parse to an
equivalent program. `to_ali` is the hand-rolled debug printer and carries no such
guarantee.

## Non-obvious invariants and gotchas

- **Boundary is all-or-nothing.** `instr_to_ast` attaches an `ast::Boundary`
  only on `(Some(_), Some(_))`; any other combination emits a bare name with
  `boundary: None`. `TypeDef::cell` always produces `(None, None)` (a
  0-dimensional generator), `cell_bd` always `(Some, Some)`. There is no way to
  build a one-sided boundary through this API, which matches the language: a
  generator either is a point or has both an input and output [[boundary]].
- **`then` flattens left, parenthesises right.** If `self` is already a
  `Seq`, `other` is *pushed onto* it (no nesting); otherwise a two-element `Seq`
  is formed. So `a.then(b).then(c)` is one flat `Seq[a,b,c]`, but the right
  argument of a single `then` is emitted parenthesised when compound
  (`repr_to_dexpr` / `repr_to_dexpr_str`). `seq` folds with `then`, giving
  `a b (c d) e`; `seq_flat` instead splices every element's `Seq` together so
  peers stay unparenthesised. Choose `seq_flat` when the pieces are already
  individually correct.
- **`par` is binary and left-associative.** `par_seq` folds with `par`, so
  `par_seq([a,b,c])` is `(a #0 b) #0 c`. Horizontal paste is fixed at dimension
  $0$ everywhere in this module (`dim: "0"` in the AST, `#0` in text).
- **The free combinators panic on empty input.** `seq` and `par_seq` `reduce`
  and `.expect(...)` — an empty iterator is a programming error, not a runtime
  case. `obs(0)` therefore panics (it calls `seq` over an empty range).
- **`attach` clause LHS is always a single cell.** `instr_to_ast` builds each
  clause's `lhs` from `repr_to_ast(DiagRepr::Cell(gen_name))` — the left side of
  an `attach … along [ g => d ]` clause is a bare generator name by construction,
  never a compound diagram.
- **Spans are synthetic.** Every node is wrapped by `syn`, so error spans on a
  generated program are meaningless. This is the deliberate cost of building
  ASTs out of band.
- **No unit tests in the module.** Behavioural coverage lives downstream: the
  `trs` plugin exercises `cell`, `cell_bd`, `obs`, `seq`, `seq_flat`, `par_seq`,
  and `compose_or_single` while compiling rewriting systems
  (`plugins/trs/src/generate.rs`, `plugins/trs/src/encode.rs`). When changing
  emission, treat the plugin's golden output as the regression surface.

## Mathematics

This module is **infrastructure, not realisation**: it builds and serialises
syntax, it does not compute with [[diagram|diagrams]] or carry any categorical
content. The bridge is therefore a *support* relationship, stated plainly.

`codegen` supports the [[module-system]]: a built `Program` is exactly a
`@Type` block of generator and `attach` declarations — the same surface syntax
the parser accepts and the interpreter populates into a `Complex`. `TypeDef`
mirrors a type block; `attach` mirrors `attach … along`, the language's
[[partial-map]] form.

The `Diag` builders are a Rust-side surface for [[diagram]] *expressions* — the
`PrincipalPaste` (vertical $\#$) and `Paste` ($\#_0$) shapes that the
interpreter elaborates into an actual labelled molecule. `codegen` never
performs that elaboration; it only produces the syntax that, once
[[interpreter|interpreted]], becomes a diagram. See [[language-parser]] for the
AST types these builders target and [[interpreter]] for what `Program::interpret`
hands the program to.
