---
kind: question
status: draft
last-touched: 2026-06-09
---

# What does `open` bring into scope vs `include`?

## The question

`include M` is **eager and total**: it copies every generator of $M$ into the
ambient complex under an alias prefix and records an identity inclusion map
([[module-system]]). `open M` should instead bind a **name**: make $M$
addressable — enough to define a [[partial-map]] out of part of $M$ and attach
along it — *without* copying $M$'s generators, and ideally without loading the
parts of $M$ the map never touches. Precisely: what does `open M` expose, and
what does `attach B :: M along F` compute when $F$'s domain is an opened
module?

## Sketch (from the design notes)

```
# Module.ali
@Type
  X <<= { x, y, f : x -> y }
  Y <<= X { g : x -> y, p : f -> g }

# Main.ali
A <<= { x, e : x -> x, let y = x, let f = e }
open Module
let F :: Module = [ X => A ]
attach B :: Module along F
```

Intended outcome: a new complex `B.Y` equal to `A { g : x -> x, p : e -> g }` —
the part of `Module` not covered by `F` is transported along it, and `Y`, `Z`,
… are never loaded unless reached.

## Current state

Nothing of `open` is built. It is **not even a token**: the lexer keyword set
(`ident_or_nat_or_kw` in `src/language/lexer.rs`) is `include`, `attach`,
`along`, `assert`, `in`, `out`, `Type`, `let`, `total`, `map`, `as`, `index`,
`for`, `bar`, `run` — and no parser production consumes an `open` form, so
`open M` lexes as two identifiers and is a parse error.

How far the sketch gets today:

- **Maps out of modules already exist.** In a `@Type` block,
  `let F :: Module = […]` resolves its domain as a module
  (`interpret_def_pmap_module` → `resolve_module_domain`,
  `src/interpreter/{partial_map,resolve}.rs`), storing a
  `MapDomain::Module` map — the `let F` line needs no new machinery.
- **Attachment over module domains does not.** `attach B :: Module` fails:
  `interpret_address` requires the final address segment to name a *type*
  generator, and `interpret_attach_instr` (`src/interpreter/include.rs`)
  rejects `MapDomain::Module` outright (*"Unexpected module domain in
  attach"*). Only types can be attached.
- **Laziness has no hook.** Loading is whole-graph and eager:
  `InterpretedFile::load` interprets every dependency leaves-first before the
  root ([[interpreter]]), so "don't load `Y`" contradicts the current pipeline.

## Open points

- **Scoping rule.** Does `open M` bind only the module alias — usable as a
  `:: M` map domain and as a qualified-address prefix — with *no* generator
  import? If so, how do `M`'s names appear in diagrams without classifiers in
  the ambient complex?
- **Module attachment.** Extend `attach … along F` to `MapDomain::Module`
  domains, deriving `B.Y` from $F$ by the same pushout as type attachment
  ([[module-system]]) but transporting *type definitions*, not just cells.
- **Laziness vs the store.** Loading only the reached part of `M` conflicts
  with the eager topological pipeline and the canonical-path module keying in
  [[interpreter]] — per-type demand loading would need the store to admit
  partially-interpreted modules.

## Related

[[module-system]] · [[partial-map]] · [[interpreter]]
