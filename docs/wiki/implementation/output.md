---
kind: impl
status: stable
last-touched: 2026-06-05
code: [src/output/mod.rs, src/output/normalize.rs, src/output/types.rs]
---

# output — normalisation to a name-keyed render tree

> The interpreter computes with opaque IDs; humans read names. `output`
> *normalises* a `GlobalStore` into a plain, ID-free tree keyed by names, then
> renders that tree as text. Two halves: `types.rs` holds the inert data + its
> `Display`, `normalize.rs` walks the store and resolves every tag back to a
> generator name against the [[core-complex|Complex]] that owns it.

This layer is downstream of everything. By the time control reaches it, a file
has been parsed, type-checked, and elaborated into a [[interpreter|GlobalStore]]
full of `Tag::Global(GlobalId)` references into side tables. None of that is
legible. `normalize` produces a `Store` in which every cell, type, diagram, and
map is named, sorted deterministically, and free of IDs — fit for
`assert_eq!` in tests and for the `Display` impls that print a `.alifib` file's
contents.

## What it owns

The render boundary of the whole interpreter. `mod.rs` re-exports the data
types (`Cell`, `Dim`, `Map`, `Module`, `Store`, `Type`) and the one public
rendering helper `render_diagram`; the map-[[hole]] listing helpers
`render_hole_boundary`, `render_hole_constraints`, and `domain_complex` are
`pub(crate)` in `normalize.rs`, consumed by the interactive hole listings
([[interactive-session]]), while `render_hole_line` / `render_map_holes` are
module-private and drive the type-detail render.
`Display for GlobalStore` (in `normalize.rs`) routes through
`GlobalStore::normalize`. The module owns no semantics — it never decides what a
diagram *means*, only how to spell one.

## Key public types

The hierarchy mirrors the interpreter's scoping: `Store` → `Module` → `Type` →
`Dim` / `Cell` / `Map` (`output::types`).

| Type | Role |
|---|---|
| `Store` | top: `cells_count`, `types_count`, and `modules` in load order. `#[derive(PartialEq)]` — the unit of structural test assertions |
| `Module` | one source file: canonical `path` + its `types` in definition order |
| `Type` | one named type (or the unnamed root, shown `<empty>`): generators grouped into `dims`, plus `diagrams` and `maps` sorted by name |
| `Dim` | generators at one dimension, `cells` sorted by name |
| `Cell` | a named generator *or* diagram with its boundary rendered as two strings `input`, `output` (both empty ⇒ a 0-cell) |
| `Map` | a named map, rendered `name :: domain`, plus `holes` — its open [[hole|holes]] pre-rendered as `?name : in → out` lines |

Every `Display` impl substitutes `<empty>` for the empty name (the unnamed root
type / anonymous generator), and a `Cell` with empty `input`/`output` prints as a
bare name rather than `name : ->`.

## Data flow

```
GlobalStore::normalize
   │  modules_iter()                       (load order)
   ▼
normalize_module(store, path, mc)
   │  generators_iter → sort by generator_order   (insertion order, not name)
   │  each module generator's Tag::Global(gid)
   │     → store.find_type(gid).complex        the type's own Complex (tc)
   ▼
normalize_type(store, name, module_complex, tc)
   ├─ generators of tc, grouped by dim, sorted by name
   │     cell_data_for_tag(tc, tag) → cell_from_data       ← reads CellData
   ├─ diagrams_iter, sorted by name
   │     cell_from_diagram(n, d, tc)                       ← extracts boundary
   └─ maps_iter, sorted by name
         render_domain(dom, module_complex)
   ▼
Store  ──Display──▶  text
```

Resolution is two-layered. Module-level generators are *types*; each carries its
own `Complex` (`type_entry.complex`), and the type's contents are resolved
against *that* inner complex, while a map's domain is resolved against the
*outer* `module_complex`. Getting the scope wrong here yields raw `Tag` strings
in the output — see the fallbacks below.

### The two ways a cell gets its boundary

This is the crux. A `Cell`'s `input`/`output` come from one of two routines, and they
are *mirror images*:

- **`cell_from_data`** — a *generator*. Its boundary is already stored: the
  interpreter keeps `CellData::Boundary { boundary_in, boundary_out }`, so the
  renderer just reads the two sub-[[diagram]]s off and renders them.
  `CellData::Zero` ⇒ empty `input`/`output`.
- **`cell_from_diagram`** — a *named diagram* (a let-binding). No boundary is
  stored; it must be *recomputed* from the diagram's shape. With $n =$
  `top_dim()`, the relevant boundary dimension is $k = n - 1$ via
  `top_dim().checked_sub(1)`; the input/output are then
  `Diagram::boundary(Sign::Input, k, diag)` and `…Sign::Output…`. A
  $0$-dimensional diagram (`checked_sub(1)` underflows to `None`) or a failing
  boundary computation falls back to an empty `input`/`output`, i.e. a bare name.

So `cell_from_diagram` is the **boundary-extraction mirror** of [[core-diagram]]:
where the interpreter *stored* a generator's $\partial^\pm_{n-1}$ as `CellData`,
the renderer *re-derives* a diagram's $\partial^\pm_{n-1}$ on demand, calling the
same [[boundary]] machinery. Both then funnel into `render_diagram_tree`.

### Rendering a diagram to a term — `render_diagram_tree`

A boundary sub-diagram is rendered not as a flat list of labels but as a
structured *term* recovered from its paste history. `render_diagram_tree`
fetches the input [[core-paste-tree|`PasteTree`]] at the top dimension
(`Diagram::tree(Sign::Input, n)`):

- `PasteTree::Leaf(tag)` → the generator name via
  `Complex::find_generator_by_tag` (empty names and missing tags fall back to the
  raw `Tag` `Display`).
- `PasteTree::Node { dim: k, .. }` → a $\#_k$ chain, **flattened**:
  `collect_chain` walks all same-dimension nodes so that nested
  `paste(k, paste(k, a, b), c)` prints `(a #k b #k c)`, not `((a #k b) #k c)`.

If there is no paste history (`tree` returns `None`), it falls back to
`diagram_labels` — the top-dimension labels joined by spaces, or `"?"` if even
those are absent. `render_diagram` is the public wrapper, also used by the REPL
([[interactive-repl]]).

### Hole listing — `render_map_holes`

A map with unfilled [[hole|holes]] renders them inline, one line each
(`render_map_holes`). `hole_names` first assigns every metavariable a display
name after the generator it images; `render_hole_line` then prints a hole as
`?name` (a 0-cell) or `?name : in → out`, with `render_hole_boundary` walking the
boundary paste trees via `render_paste_tree_with_holes` — so a leaf that is
itself a metavariable shows as another `?name`, and pure holes and conditionals
render uniformly. `render_hole_constraints` renders the equations a *conditional*
pending assignment imposes (`F(x.side) = a.side`). These feed both the
`Display`/type-detail render and the interactive `holes` command
([[interactive-session]]).

Holes are not solved-then-reported; they are listed where they live — on the map.

## Non-obvious invariants and gotchas

- **Determinism is by design, but not uniform.** Module generators (types) sort
  by `generator_order` (*insertion* order), while a type's generators, diagrams,
  and maps sort by *name*. The mix is deliberate: types keep authoring order;
  their contents are alphabetised for stable diffs.
- **`normalize` panics on broken interpreter invariants.** A module generator
  with a `Tag::Local`, a missing `find_type`, or a generator with no
  `cell_data_for_tag` all `panic!` — these are interpreter bugs, never caller
  errors, and the panic message says so.
- **Stored vs. recomputed boundary.** A generator's boundary is *read*
  (`cell_from_data`); a named diagram's is *recomputed* (`cell_from_diagram`).
  Confusing the two paths is the easy mistake — the diagram path is the only one
  that can underflow on a 0-cell or fail the boundary call.
- **Raw `Tag` leakage signals a scope bug.** Every name resolution
  (`find_generator_by_tag`, `render_domain`) falls back to printing the `Tag`/ID
  if the tag isn't found in the supplied `Complex`. A stray ID in the output
  almost always means the wrong scope was threaded in (module-vs-type complex).
- **`<empty>` is a real, visible name.** The unnamed root type and anonymous
  generators are not skipped — they render as the literal string `<empty>` via
  `name_or_empty` (in resolution) and the inline checks in the `Display` impls.
- **Term, not list.** Boundaries print as flattened $\#_k$ terms reconstructed
  from the [[diagram|paste history]], which is why a lost history degrades to a
  space-joined label list rather than erroring.
- **Holes render where they live.** A map's open [[hole|holes]] are listed inline
  by `render_map_holes`; there is no separate solve-then-report pass. A hole leaf
  inside a boundary prints as a `?name` metavariable, so pure holes and
  conditionals read uniformly.

## Mathematics

This module realises no mathematics of its own — it is the *presentation* layer
that makes the interpreter's diagrams legible. Its one genuine mathematical
operation is borrowed: `cell_from_diagram` extracts the input/output
$\partial^\pm_{n-1}$ of a named [[diagram]] by calling `Diagram::boundary`, the
same [[boundary]] operation documented in [[core-diagram]]. That call is the
"boundary-extraction mirror": where a generator's $\partial^\pm_{n-1}$ is *stored*
as `CellData::Boundary`, a let-bound diagram's is *recomputed* here. Everything
else — sorting, name resolution, term flattening — is faithful transcription of
the [[diagram|diagram]] and its paste structure, not new theory. The bridge to
[[diagram]] and [[boundary]] is therefore a *presentation-of* relationship, with
exactly one true realisation (the boundary call). See [[interpreter]] for the
`GlobalStore` being normalised and [[interactive-repl]] for the `render_*`
consumers.
