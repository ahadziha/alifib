# alifib — Interpreter Description

alifib is an interpreter for the `.ali` language, a notation for defining and
reasoning about algebraic structures using higher-dimensional diagrams. It is
based on the theory of *oriented graded posets* (ogposets), which provide the
combinatorial shapes that diagrams are built from.

## Core ideas

The central objects are **generators** and **diagrams**.

A generator is an atomic cell of some dimension. Every generator above dimension
zero has a *source* and a *target*, which are diagrams of one dimension lower.
A **diagram** is a pasting of generators: generators glued end-to-end along
matching boundaries. Two generators can be pasted whenever the target of the
first equals the source of the second.

For example, given 0-cells `A`, `B`, `C` and 1-cells `f : A → B` and
`g : B → C`, the juxtaposition `f g` is a 1-dimensional diagram with source `A`
and target `C`. Given 2-cells `α : f → f'` and `β : g → g'`, the paste `α β`
is a 2-dimensional diagram of type `f g → f' g'`.

The interpreter enforces that all pastings are *well-typed*: boundary dimensions
must match and source/target diagrams must be equal.

## Types and complexes

A **type** is a named algebraic structure declared in a `@Type` block. Its body
(`<<= { ... }`) lists the generators and derived constructs that make up that
structure.

```
@Type
Magma <<= {
  pt,
  ob  : pt -> pt,
  mul : ob ob -> ob
}
```

Here `pt` is a 0-cell, `ob` is a 1-cell (an endomorphism of `pt`), and `mul`
is a 2-cell whose source is the paste `ob ob` and whose target is `ob`.

A type body can also contain:

- **`let x = diagram`** — a named diagram binding, for giving a name to a
  composite.
- **`def f :: T = [ ... ]`** — a named partial map from type `T` into the
  current context.
- **`include T`** — imports all generators of type `T` into the current scope.
- **`attach S :: T along [ ... ]`** — attaches a copy of structure `T` via an
  explicit map that connects `T`'s generators to things already in scope. This
  is the main mechanism for expressing relationships between structures.

## Partial maps

A **partial map** (`PMap`) is a structure-preserving assignment: it sends
generators of a source type to diagrams of matching boundary in a target type.
The `along [ ... ]` clause in an `attach` statement is a partial map, as are
`def` definitions and anonymous maps written inline.

Maps compose: if `f : A → B` and `g : B → C` are maps, `f.g` is their
composite. The interpreter verifies boundary compatibility at each step.

## Modules and inclusion

Each `.ali` file is a **module**. A module can include other modules with
`include ModuleName` at the top-level `@Type` block. Included modules are
located via a search path (the `ALIFIB_PATH` environment variable plus the
directory of the file being interpreted).

## Local blocks and assertions

A `@T` block (where `T` names a type already in scope) opens a **local block**
in the context of that type's structure. Inside a local block, diagram bindings
can reference the type's generators by name, and `= lhs rhs` assertions check
that two diagrams are equal up to isomorphism.

## Holes

A `?` in a diagram expression is a **hole**: a position whose value is unknown.
The interpreter reports holes with their inferred boundary (source and target)
derived from the surrounding context, as a form of type-directed feedback.

## What the interpreter produces

After successfully interpreting a file the interpreter reports:

- Every module, with its types listed by name.
- For each type, its generators grouped by dimension, any named diagram
  bindings, and any named map bindings.
- Any holes, annotated with their inferred boundary.
- Any errors (type mismatches, undefined names, boundary shape failures).

The interpreter does not execute programs in a traditional sense: there is no
runtime evaluation or reduction. Instead it *elaborates* the structure —
checking that all declared generators and maps are well-formed and that all
asserted equations hold — and records the resulting global state.
