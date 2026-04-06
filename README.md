# alifib

An interpreter for a language of pasting diagrams in regular directed complexes,
based on Amar Hadzihasanovic's theory of molecules.

## Overview

`alifib` lets you define algebraic structures using higher-dimensional
generators and diagram composition, check that they are well-formed, and assert
equations between them. A *regular directed complex* is the shape of a pasting
diagram; a *molecule* is a shape built inductively by gluing atoms. The
interpreter elaborates type definitions, checks boundaries, resolves included
modules, and verifies that partial maps are structure-preserving.

## Language

Source files use the `.ali` extension. The core constructs are:

**Type blocks** — declare algebraic structures with generators:

```ali
@Type
Ob <<= {
  pt,
  ob : pt -> pt
},

Magma <<= {
  attach Ob :: Ob,
  m : Ob.ob Ob.ob -> Ob.ob
}
```

A bare name like `pt` declares a 0-cell. `name : src -> tgt` declares a cell
with a source and target diagram. `attach T :: S` imports a copy of type `S`
under the name `T`.

**Partial maps** — structure-preserving assignments between types, written with
`along [ gen => diagram, ... ]`. These are used by `attach` to identify
generators across types.

**Diagram expressions** — vertical composition is written by juxtaposition
(`f g`), horizontal composition by `f #0 g` (pasting along a 0-cell boundary),
and grouping with parentheses.

**Modules** — `.ali` files can include other files. The interpreter resolves the
full dependency graph before elaboration. Set `ALIFIB_PATH` to a
colon-separated list of directories to search for included files.

See `examples/` for more, and `docs/grammar.md` for the full grammar.

## Building

```
cargo build --release
```

## Usage

```
alifib <input.ali> [-o <output.ali>] [--ast] [--bench N]
```

- `-o / --output` — write output to a file instead of stdout
- `--ast` — print the parsed AST instead of interpreting
- `--bench N` — run N times and print average wall time in milliseconds

## Examples

```
cargo run --release -- examples/Frobenius.ali
cargo run --release -- examples/YangBaxter.ali
```

## Repository layout

```
src/           Interpreter source (language/, core/, interpreter/, output/)
examples/      Example .ali files
docs/          Grammar, interpreter description, formal semantics (LaTeX)
trs/           Plugin: convert term rewriting systems to alifib (see trs/README.md)
```
