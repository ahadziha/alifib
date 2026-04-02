# Interpreter Data Summary (Q/A)

This is a short, practical summary of what data the interpreter carries, how it is represented, and why.

## What is a diagram in this interpreter?
A diagram is represented as:
- `shape` (`Ogposet`): connectivity/incidence structure
- `labels`: `labels[dim][pos] = Tag`
- `paste_history`: composition history per dimension and side

Why this split:
- `shape` captures structure
- `labels` capture identity of each cell
- `paste_history` captures how composites were built

## Is a diagram just shape + labels?
Almost. In code it is `shape + labels + paste_history`.

Why not just shape + labels:
- boundary/paste operations also need composition history, not only final incidence.

## What is a tag?
`Tag` is cell identity:
- `Tag::Global(GlobalId)`
- `Tag::Local(String)`

Why not just strings:
- global IDs are stable across modules/scopes
- names can collide, alias, or change
- maps/state can key by stable identity

## What does CellData record?
`CellData` is boundary specification for a generator:
- `Zero`
- `Boundary { boundary_in, boundary_out }`

Why:
- this is the minimal semantic payload needed to construct/check higher cells.

## What is in a Complex?
`Complex` is one scoped mathematical environment:
- generators (name <-> tag, with dimension)
- generator classifier diagrams
- named diagrams
- named partial maps
- local cells
- name-usage bookkeeping

Why:
- it is the place where local definitions and lookups live.

## What is in State? (planned rename: GlobalStore)
Global registry across interpretation:
- global cells (`GlobalId -> CellEntry`)
- cells grouped by dimension
- global types (`GlobalId -> TypeEntry { data, complex }`)
- modules (`ModuleId -> Complex`)

Why:
- cross-module/type consistency needs one canonical store.

## Why do types have GlobalIds?
Because types are referenced from many places (addresses, attach/include, map domains).

Why:
- stable identity independent of local naming/aliasing.

## What is Namespace? (planned rename: TypeScope)
Current working scope during local interpretation:
- `root` (planned rename: `owner_type_id`)
- `location: Complex`

Why keep `root`:
- local edits happen on `location`
- successful changes must be written back to the owning type in global store via `owner_type_id`.

## Is this “everything has global IDs” pattern known?
Yes. Common names:
- object/entity identity
- symbol IDs / symbol table keys
- surrogate keys (DB terminology)

Why this pattern here:
- semantic operations need stable references, not fragile text names.

## Why paste history per dimension and side, not per cell?
`paste_history[dim][side]` is one decomposition tree over the boundary slice at that dimension/side.

Why:
- boundaries are whole subdiagrams, not independent per-cell trees
- operations (`boundary`, `paste`) consume/produce history at boundary slices.

## What rename decisions have we made?
Accepted:
- `State` -> `GlobalStore`
- `Diagram::trees` -> `Diagram::paste_history` (done)
- `Sign::Input` -> `Sign::Source`
- `Sign::Output` -> `Sign::Target`
- `Namespace` -> `TypeScope`
- `Namespace.root` -> `owner_type_id`

Kept:
- `Complex`
- `shape`

## What have we not covered yet?
- full `Ogposet` invariants
- pushout/embedding mechanics in detail
- full `PMap` internals
- complete include/attach worked examples
- strict invariant hardening plan (e.g. tighter constructors/validation)
