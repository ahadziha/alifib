---
kind: impl
status: stable
last-touched: 2026-06-09
code: [src/language/mod.rs, src/language/lexer.rs, src/language/token.rs, src/language/parser.rs, src/language/ast.rs, src/language/ast_fmt.rs, src/language/ast_print.rs, src/language/error.rs]
---

# language-parser — lexer, parser, AST

The front end. Turns `.ali` source text into a `Program` of typed AST nodes
(`ast.rs`), which the [[interpreter]] then evaluates. The pipeline is two
[chumsky](https://github.com/zesterer/chumsky) passes — `&str` → tokens
(`lexer.rs`) → AST (`parser.rs`) — bracketed by two pretty-printers (`ast_fmt`,
`ast_print`) and a diagnostics module (`error.rs`).

## Pipeline & public entry points (`src/language/mod.rs`)

Every entry point follows the same shape: lex with `lexer::lexer()`, feed the
token slice (re-spanned via `split_token_span` with an end-of-input span) into a
parser from `parser.rs`, then collect lex + parse errors.

- `parse(&str) -> Result<Program, Vec<Error>>` — the whole-program parser. On
  success it runs `resolve_for_bodies_program` (see *for-blocks* below) before
  returning. Lex and parse errors are merged and `dedup_by`'d on `(message, span)`.
- `parse_diagram(&str) -> Result<Spanned<Diagram>, String>` — one diagram
  expression, terminated by `end()`. Two [[interactive-engine|engine]] callers:
  the slow path of `eval_diagram_expr` (a prompt expression that isn't a stored
  diagram name), and the proof round-trip check (`output::render_diagram` →
  `parse_diagram` → `interpret_diagram`).
- `parse_complex_instrs`, `parse_type_instrs`, `parse_local_instrs`,
  `parse_pmap_clauses` — parse a *comma-separated instruction list* (no enclosing
  block). These exist to re-parse the textual body of an expanded `for`-block
  back into AST nodes at the right grammatical level. Each runs the matching
  `resolve_for_bodies_*` afterwards so nested `for`s survive a second round.
- `collect_includes(&Program) -> Vec<String>` *(internal, `pub(crate)`)* —
  gathers `include`d module names from `@Type` blocks **only**; `@Local`-block
  includes name types already in scope, not external files (see comment at
  `collect_includes`).

## The grammar (`src/language/parser.rs`)

A `.ali` file is a sequence of **blocks** (`program_parser`). Block bodies are
**not brace-delimited** — instructions simply follow the header, comma-separated
(trailing comma allowed, possibly empty), until the next `@` or end of input:

- `@Type inst, …` → `Block::TypeBlock` of `TypeInst`s.
- `@ <Complex> inst, …` → `Block::LocalBlock` over an existing complex, of
  `LocalInst`s. The `@` is consumed by the block rule; the `Complex` parser
  itself never sees it. (Braces, when present, belong to an inline
  `Complex::Block`, not to the local block.)

The three instruction levels share most sub-grammars but differ in what they
admit:

| Level | Distinctive forms |
|-------|-------------------|
| `TypeInst` | `Generator` (`name : input -> output <<= Complex`), `IncludeModule` |
| `ComplexInstr` | `NameWithBoundary`, `AttachStmt` (`attach n :: addr along …`), `IncludeStmt` |
| `LocalInst` | `AssertStmt` (`assert lhs = rhs`) |

All three also admit `let`/`let total … ::` bindings (`build_let_or_def`),
`index` declarations, and `for`-blocks.

### Generators and the `<<=` arrow

A generator is `NameWithBoundary <<= Complex` (`Token::LArrow`, lexed from
`<<=`). `NameWithBoundary` is a name with an optional `: input -> output`
[[boundary]] (`build_name_with_boundary`, `build_boundary`).

### Diagrams (`build_diagram`)

The diagram grammar is the heart of the parser and is genuinely recursive
(through parentheses and anonymous maps). Levels, outermost first:

1. **Explicit paste** — `lhs #n rhs…` folds left (via `foldl` over `Token::Hash`)
   into `Diagram::Paste { lhs, dim, rhs }`, where `rhs` is a principal run of
   `DExpr`s. The `#n` names the **paste dimension**: `lhs` and `rhs` are pasted
   along their shared $n$-[[boundary]], i.e. $\#_n$.
2. **Implicit (principal) pasting** — a juxtaposed run of `DExpr`s becomes
   `Diagram::PrincipalPaste`; the interpreter pastes them at the *principal*
   dimension $\min(\dim) - 1$ (it is a paste, not a separate composition — see
   [[interpreter]]).
3. **Dotted access** — `a.b.c` folds left into nested `DExpr::Dot`.
4. **Components** (`DComponent`): `in` / `out` keywords,
   `(d)` parenthesised, `(map … :: Complex)` anonymous [[partial-map]], `(run auto
   on d)` rewrite request, a name, or a **string literal**. (A `?` hole is *not* a
   diagram component — it is only the right-hand side of a partial-map clause; see
   below.)

`run`'s `auto` and `on` are *not* keywords — they're matched as bare
`Token::Ident("auto")` / `Token::Ident("on")` inside `build_diagram`, so they
stay usable as ordinary identifiers elsewhere (lexer tests `test_on_is_ident`).

### String literals expand to pastes

`expand_string_literal` turns `"abc"` into a parenthesised
`PrincipalPaste` of one generator name per character, mapping each char through
`char_to_generator_name` (`a`→`a`, `(`→`LPAREN`, ` `→`SPACE`, …). An empty
string becomes the single name `here`. Escapes (`\n`, `\t`, `\\`, …) are decoded
to their logical char first.

### Partial maps

`PartialMapDef` is either a `PartialMap` (a dotted chain of
`PartialMapBasic` — name, parenthesised, or nested `(map …)`) or an `Ext`
(`PartialMapExt`): an optional prefix map plus a bracketed list of `lhs => rhs`
rewrite clauses — `prefix? [ clause, … ]`; `[clauses]` alone is a bare `Ext`
with no prefix. A clause's right-hand side is a `ClauseRhs`: either a diagram
or a bare `?` ([[hole]]). The `?` is legal **only** here — it carries just the
span of its token (`ClauseRhs::Hole`) and is not a diagram component.

### For-blocks and indices (string templating)

`index N = [a, b, c]` declares a named value list (`IndexDecl`).
`for v in (N | [vals]) [bar (N | [vals])] { … }` is a `ForBlock`: it iterates
`v` over the index, optionally excluding the `bar` set.

The body is **not parsed at grammar time**. `for_body` matches *balanced braces*
(over arbitrary tokens, so `<var>` instances inside lex fine) and records only the
body's `Span`; `ForBlock.body_text` is left empty. After parsing,
`resolve_for_bodies_*` (in `mod.rs`, via `resolve_fb`) walk the AST and splice the
raw source text `source[body_span]` into `body_text`. The interpreter later expands
it: `eval::expand_body` replaces the literal text `<v>` in `body_text` with each
index value (comma-joining the copies), and re-parses each expansion via the
instruction-list parsers above (`parse_complex_instrs` / `parse_type_instrs` /
`parse_local_instrs` / `parse_pmap_clauses`). This deferred, text-level expansion
is why `for`-blocks can appear at every instruction level and inside partial-map
clause lists (`PMapEntry::For`).

### Parser-reuse pattern

The mutually-recursive sub-grammars are built by `build_*` helpers that take and
return *type-erased* `recursive()` handles (the `R<…>` alias) — this both breaks
construction-time recursion and stops the generic types exploding into giant
symbol names. The public surface is small: `program_parser`, `diagram_parser`,
and the four `*_instrs`/`pmap_clauses` parsers. Each
re-assembles whatever slice of the `build_diagram → build_partial_map →
build_partial_map_def → build_complex` chain it needs (`pmap_clauses_parser` only
needs `build_diagram`). To expose a new sub-grammar, follow this chain and erase
with `recursive()`; see the `end()` gotcha below for which entry points
self-terminate.

## The AST (`src/language/ast.rs`)

Every node is wrapped in `Spanned<T> { inner: T, span: Span }` (a `{start, end}`
byte range; `Span::synthetic()` gives `0..0` for programmatically built nodes).
The whole file is `#![allow(dead_code)]` — many fields are read only by some
consumers. The shape mirrors the grammar:

| Type | Role |
|------|------|
| `Program` | top level: `Vec<Spanned<Block>>` |
| `Block` | `TypeBlock(Vec<TypeInst>)` or `LocalBlock { complex, body: Vec<LocalInst> }` |
| `TypeInst` / `ComplexInstr` / `LocalInst` | the three instruction levels (see the grammar table) |
| `Complex` | `Address(Address)` or `Block { address, body }` — an inline complex |
| `Diagram` | `PrincipalPaste(Vec<DExpr>)` (implicit) or `Paste { lhs, dim, rhs }` (explicit `#n`) |
| `DExpr` | `Component(DComponent)` or `Dot { base, field }` |
| `DComponent` | leaf: `Name`, `In`, `Out`, `Paren`, `AnonMap`, `Run` (no `Hole`) |
| `PartialMapDef` | `PartialMap(PartialMap)` or `Ext(PartialMapExt)` |
| `PartialMap` / `PartialMapBasic` | dotted chain of name / paren / `AnonMap` |
| `PMapEntry` | a partial-map clause `Clause(lhs => rhs)` or a nested `For` |
| `ClauseRhs` | a clause RHS: `Diagram(…)` or `Hole(Span)` — the only place `?` is legal |
| `ForBlock` / `IndexDecl` / `ForIndex` | the string-templating machinery |

`Address` is a type alias for `Vec<Spanned<String>>` (a dotted name path).
`DExpr::dotted_name` extracts the qualified name (`Sub.arr`) when the expression
is exactly one — the canonical key generators are stored and rendered by.

## Lexer (`src/language/lexer.rs`, `token.rs`)

`token.rs` defines the single `Token<'src>` enum: fifteen keyword variants
(`Include`, `Attach`, `Along`, `Assert`, `In`, `Out`, `Type`, `Let`, `Total`,
`Map`, `As`, `Index`, `For`, `Bar`, `Run` — the exact set `ident_or_nat_or_kw`
matches, with **no `open`**), the punctuation/symbol variants, and the three
data-carrying variants `Ident(&str)`, `Nat(&str)`, `Str(&str)` that borrow from
the source. Its `Display` impl renders each token back to its surface lexeme —
used in chumsky's `Rich` error messages, not for source reconstruction.

**`LAngle`/`RAngle` (`<`/`>`) are not dead.** No *named* production consumes them,
but they are load-bearing for `for`-block bodies: a body is scanned as raw tokens
(`for_body` matches balanced braces over the wildcard `any()`), and the variable
instance syntax `<var>` substituted in that body — e.g. `<ctx>` — must lex, which
it could not without `<`/`>` (see the clarifying comment at the lexer's `symbol`
rules). The for-body lexes, then `eval::expand_body` does the textual `<var>` →
value replacement (see [[interpreter]]). The boundary arrow is still `->`
(`Token::Arrow`), not angle brackets.

`lexer()` is a chumsky parser over `&str` producing `Vec<(Token, SimpleSpan)>`
(here `Spanned<T>` is the lexer-local tuple alias `(T, Span)`, distinct from
`ast::Spanned<T>`, which is a `{ inner, span }` struct). Notable behaviour, all
backed by the in-file test module:

- **Keywords** lex to dedicated `Token` variants (`Token::Type`, `Token::Let`, …)
  via exact match *after* maximal-munch identifier scan, so `barring` is an ident,
  not `bar` + `ring` (`test_bar_keyword_prefix`).
- **Identifiers** are `is_alphanumeric() || '_'`, so Unicode letters lex as idents
  (`héllo`, `变量`, `αβγ`). A run of *only* ASCII digits becomes `Token::Nat` —
  leading zeros included (`007`, `test_nat_multi_digit`); Unicode digits (`٣`) do
  **not** (`test_unicode_digit_not_nat`).
- **Comments** are nestable `(* … *)`, handled by a `recursive` parser and
  stripped as padding (`test_nested_comment`).
- **Strings** accept `"…"` or `'…'`, keep escapes *raw* in the token (decoding
  happens later in `expand_string_literal`), and error if unclosed.
- **Spans are byte offsets**, not char counts — `αβ` spans `(0, 4)`
  (`test_unicode_ident_span`). `error.rs` converts to (line, col) when reporting.
- `<<=`, `::`, `=>`, `->` are matched before their single-char prefixes so the
  longer symbol wins.

## Output: two pretty-printers

These are distinct on purpose; don't conflate them. The CLI exposes both as
separate modes: `--ast` runs `ast_fmt`, `--print` runs `ast_print`
(`cli/src/main.rs::run_ast` / `run_print`).

- `ast_fmt.rs` — the `Display` impls, in two halves the file itself separates:
  *value/leaf* types (`Diagram`, `DComponent`, `Complex`, …) render **compact and
  lossy** on one line (e.g. `AnonMap` collapses to `(map ...)`, a `Complex::Block`
  to `{...N items}`, a `for`-block body to `{ ... }`); *structural* types
  (`Program`, `Block`, the three instruction enums) render as an **indented tree**
  via `pp_*` helpers and the `pad` indenter. It is not a faithful source printer.
  Consumed by the CLI `--ast` dump and inline in REPL/error messages.
- `ast_print.rs` — `print_program`, the **round-trip** printer: its output
  re-parses to an equivalent AST (comments and whitespace are not preserved) —
  *except `for`-blocks*; see the gotcha below. Consumed by [[codegen]]
  (`src/codegen.rs::Program::print_ali` → `into_ast` → `print_program`) and the
  CLI `--print` mode (`cli/src/main.rs::run_print`). It is **not** used by the
  module loader.

## Errors & diagnostics (`src/language/error.rs`)

`Error` is `Syntax { message, span }` or `Runtime { message, span, notes }`
(parsing only ever produces `Syntax`). Two presentation paths:

- `report_errors` — pretty terminal output via
  [ariadne](https://github.com/zesterer/ariadne), converting byte spans to char
  offsets.
- `Error::to_diagnostic` → `Diagnostic` (a `Serialize` struct with
  one-indexed (line, col) `Position`s and a pre-rendered caret `snippet`). This
  is what crosses the web boundary to the [[interactive-daemon-web|web frontend]].

`Position::from_byte` counts columns in *Unicode scalar values*, matching editor
conventions, and clamps out-of-range offsets to source length
(`position_uses_char_count_for_column`, `position_clamped_to_source_len`).

## Gotchas

- Byte vs char offsets: tokens carry byte spans; only `error.rs` translates to
  char/line/col. Mixing the two silently mis-points carets.
- `for`-block bodies are raw text until `resolve_for_bodies_*` runs — a `Program`
  obtained without that pass has empty `body_text`. `parse()` and the
  `*_instrs` entry points run it; building a `Program` by hand does not.
- **`print_program` loses `for`-block bodies.** `ast_print::Printer::for_block`
  emits the body as the literal `{ ... }` (and `ast::ForBlock` retains no parsed
  body to print). The output still re-parses — `...` lexes as three `Dot` tokens,
  which `for_body` happily swallows — but the result is *not* equivalent, and
  expanding it later fails. The module doc's blanket round-trip guarantee
  overclaims; it holds only for `for`-free programs (all [[codegen]] output
  qualifies).
- **`let total x = diagram` silently drops `total`.** `build_let_or_def` parses
  the optional `Total` before committing to either arm, and the `LetOrDef::Let`
  arm discards the flag — only the `:: address =` map form records it
  (`DefPartialMap.total`). docs/GRAMMAR.md grants `total` to the map form only;
  the parser accepts a superset.
- `end()` placement is inconsistent: `program_parser` and the four
  `*_instrs`/`pmap_clauses` parsers append `then_ignore(end())` themselves, but
  `diagram_parser` does **not** — its `mod.rs` caller `parse_diagram` tacks on
  `end()` at the call site. Reusing `diagram_parser` at a boundary without your
  own `end()` will silently accept trailing tokens.
- **docs/GRAMMAR.md drift** (the external grammar reference, not fixed here): its
  `<ForBlock>` omits the optional `bar` exclusion; its `<DComponent>` omits
  string literals; its lexical classes are ASCII-only (`<Name>`) and forbid
  leading zeros (`<Nat>`), both of which the lexer accepts.

## Mathematics

The parser is the surface syntax of the [[module-system]]: `@Type` blocks and
`@ <Complex>` local blocks, `<<=` generators, `include`/`attach … along`, and
`let total … ::` [[partial-map|partial maps]]. Diagram expressions denote
[[diagram|diagrams]] built by pasting $\#_n$ ([[boundary]]) over [[atom|atoms]] and
[[molecule|molecules]]; `?` holes and `(run auto on …)` feed [[rewriting]].
