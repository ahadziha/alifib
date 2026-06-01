---
kind: question
status: draft
last-touched: 2026-06-01
---

# What does `open` bring into scope vs `include`?

> Draft, captured from `docs/new-notes.md`. Design not yet settled.

## The question

`open Module` should bring a **name** into scope without pulling in the module's
entire universe. We want to reference part of a module and define a map out of it
without forcing `Y`, `Z`, … to load. What exactly does `open` expose, and how does
it differ from `include`?

## Sketch from the notes

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

Intended outcome: a new complex `B.Y` equal to `A { g : x -> x, p : e -> g }`.
`open` brings the name in but does **not** include the entire universe of
`Module`; ideally `Y`, `Z` are not loaded.

## Current state

Nothing of `open` is built. `open` is **not even a token**: the lexer keyword set
(`ident_or_nat_or_kw` in `src/language/lexer.rs`) is `include`, `attach`, `along`,
`assert`, `in`, `out`, `Type`, `let`, `total`, `map`, `as`, `index`, `for`,
`bar`, `run`, and no parser production consumes an `open` form. So `open M` lexes
today as two bare identifiers and is a parse error. The only realised module
import is `include` (`interpret_include_module_instr` / `interpret_include_instr`
in `src/interpreter/include.rs`), which is **eager and total** — it copies *every*
generator of the source under the alias prefix and records an `identity_map`. The
lazy single-name binding this page asks for has no implementation; see
[[module-system]] for the import operations that do exist.

## Open points

- Precise scoping rule for `open` (lazy name binding) vs `include` (eager
  inclusion of contents).
- How `attach … along F` derives `B.Y` from the [[partial-map]] `F`.
- Interaction with the canonical-path module keying in [[interpreter]].

## Related

[[module-system]] · [[partial-map]] · [[interpreter]]
