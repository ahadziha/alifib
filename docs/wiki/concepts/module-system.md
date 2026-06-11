---
kind: concept
status: stable
last-touched: 2026-06-09
---

# Module system

A program in alifib is a forest of named [[core-complex|complexes]] organised at
two scales. A **type** is a single [[diagram|diagram]]-generator together with
the whole complex grown inside its body — an [[atom|atom]] equipped with its own
ambient universe of cells, maps, and lower atoms, all
[[regular-directed-complex|regular]] shapes of a [[directed-complex]]. A
**module** is a `.ali` file: the complex accumulated by elaborating all of its
top-level declarations, keyed by its canonical path. The module system is the
calculus by which one such complex is carried into another, and these are
exactly the two canonical ways one [[directed-complex]] embeds in another:

- **inclusion** (`include`) — a *sub-complex* embedding, the identity on shared
  generators;
- **attachment** (`attach … along`) — a *gluing*, a [[pushout]] that freely
  adjoins the un-identified cells of one complex along a [[partial-map]].

A third surface form, orthogonal to both, is **extension**: the parent address
of an inline type body, `X <<= Y { … }`, which clones `Y`'s complex as `X`'s
starting scope (flat names, inherited maps, no recorded inclusion). It is plain
inheritance, not an embedding. [[extension-inclusion-attachment]] compares all
three axis by axis.

A third axis, orthogonal to both, is **naming**: how a source name reaches
across module boundaries, by *qualified names* (dotted addresses walked through
the maps in scope) and *scoped include resolution* (the file search behind a
bare `include <Name>`). A lazy `open` that would bind a name *without*
importing its universe is unimplemented and unsettled — see
[[module-open-semantics]].

## Definition

### Types and modules as complexes

Fix the algebra of [[diagram|diagrams]] over a [[regular-directed-complex]]. A
**generator** is a named cell $a$ with a boundary specification
$\partial^-(a) \to \partial^+(a)$ (a $0$-cell has none). A **type** $T$ is a
generator together with a complex $\mathcal{C}_T$ — the cells, maps and lower
types declared inside its `{ … }` body. A type is therefore *self-contained*: it
names an atom and supplies the entire context in which that atom's boundary
lives. A **module** $M$ is the complex obtained by elaborating one source file;
its generators are the types and let-bound diagrams declared at top level.

The surface syntax (`@Type` and `@Local` blocks; see [[language-parser]]) keeps
the two scales apart by a structural law: **a `@Type` block introduces only
$0$-dimensional generators** (objects). Every higher cell $\dim \ge 1$ must be
declared inside a type body, where it has an ambient complex to be a cell *of*.
This is the syntactic shadow of the fact that an $n$-cell is meaningless without
its $(n{-}1)$-boundary already present.

### Inclusion

For a type $S$ with complex $\mathcal{C}_S$ and an ambient complex $\mathcal{C}$,
`include S` (inside a complex body) realises the **inclusion** $\iota \colon
\mathcal{C}_S \hookrightarrow \mathcal{C}$. Concretely it copies every generator
of $S$ into $\mathcal{C}$ under an alias prefix, and records the identity
[[partial-map]] $\mathrm{id}_{\mathcal{C}_S}$ as the witnessing map. Inclusion
is *eager and total*: nothing of $S$ is left behind, and no cell is renamed
beyond its prefix. Re-including an already-present generator is idempotent (the
second copy is skipped). The alias defaults to $S$'s own name; including a
*non-local* type — one addressed through a dotted path — requires an explicit
`as` alias. The `@Type`-block form `include M [as A]` does the same for a whole
module $M$, dropping $M$'s unnamed root generator.

### Attachment as a pushout along a partial map

`attach B :: S along F` is the gluing operation. Given

- an ambient complex $\mathcal{C}$ (the type or `@Local` body under
  construction),
- an *attachment* type $S$ with complex $\mathcal{C}_S$,
- a [[partial-map]] $F \colon \mathcal{C}_S \rightharpoonup \mathcal{C}$
  (the `along` clause; empty when omitted),

attachment forms the universe in which the cells of $S$ *already identified by
$F$* are shared with $\mathcal{C}$, while the cells *not in the domain of $F$*
are adjoined freshly. For each unmapped generator $a \in \mathcal{C}_S$ a new
generator $b$ is minted in $\mathcal{C}$ whose boundary is the $F$-image of $a$'s
boundary:
$$ \partial^\pm(b) \;=\; F\!\left(\partial^\pm(a)\right). $$
The map is grown in lockstep ($F \mathrel{+}= \{a \mapsto b\}$) so that a later
generator of $S$ — whose boundary may mention an earlier one — finds its image
already defined. This is exactly the universal property of a [[pushout]]

$$
\begin{array}{ccc}
\mathrm{dom}(F) & \longrightarrow & \mathcal{C}\phantom{_S} \\
\downarrow & & \downarrow \\
\mathcal{C}_S & \longrightarrow & \mathcal{C}'
\end{array}
$$

computed cell-by-cell along $F$, with $F$'s domain as the shared sub-complex. A
dimension check rejects the gluing when an image boundary lands in the wrong
$\dim$ — the colimit must respect grading.

The example `attach Ob :: Ob` (no `along`) adjoins a *fresh copy* of `Ob`: with
empty $F$, every generator is unmapped and reborn under the prefix. With a
non-trivial $F$ (e.g. `along [ id => 2id ]`) some cells are identified with the
ambient ones and only the remainder is freely added.

### Naming across modules

Once a module is in the store, two distinct mechanisms reach a name across the
boundary it was declared behind.

**Qualified names.** A dotted address `p₁.p₂.….a` is *not* a string key into a
flat namespace; it is a **walk through the maps in scope**. Resolution starts in
the current module's complex and consumes the prefix segment by segment: each
$p_i$ must name a [[partial-map]] whose domain is a *module* — and `include M`
records exactly such a map under the alias `M`. Naming a map whose domain is a
*type*, or a name that is no map at all, is an error: only module-valued maps
may be traversed. After the prefix is walked, the scope has advanced into the
named submodule's complex, and the final segment `a` is looked up there as a
type generator. So `A.Aux.Ob` means: from `A` (a module included into me), step
into its `Aux` (a module `A` itself included), and take the type `Ob` there.
Because each module carries its own inclusion maps, **the same dotted name
resolves relative to where it is written** — two modules each importing a
module called `Aux` see *their own* `Aux`. The two scoped-resolution tests
below exercise precisely this.

**Scoped include resolution.** A bare `include <Name>` does not name a file
directly; the loader searches for `<Name>.ali` in a precedence order owned by
the [[aux]] layer (`Loader::with_parent_dir` + `find_file`): (1) the including
file's own directory, (2) a *same-named subdirectory* (so `Foo.ali` may keep
private submodules in a `Foo/` directory and include them by bare name), then
(3) the inherited search paths (the working directory, `ALIFIB_PATH`, any extra
paths). **The closest directory wins**, so two files in different directories
that both `include Aux` may resolve to *different* files. The loader then
interprets the whole dependency graph **leaves-first** in topological order into
one shared store, with cycle detection — the semantic side of this (the
three-phase `InterpretedFile::load` pipeline, the parent/name → canonical-path
resolution map) is documented in [[interpreter]].

The upshot: *resolution is lexical, not global*. A module exports names; an
importer chooses local aliases for them; and a qualified name is read against the
importer's own map of aliases. There is no single global symbol table that all
modules share.

## Implementation

Realised by [[interpreter]]; parsed by [[language-parser]].

- **Storage.** A module is an `Arc<Complex>` in `GlobalStore::modules`, an
  insertion-ordered `IndexMap<ModuleId, Arc<Complex>>` (dependencies precede
  dependents); a type is a `TypeEntry { data, complex }` in `GlobalStore::types`,
  keyed by an opaque `GlobalId`. Both keys are ids, never names —
  `global_store.rs`. A `ModuleId` is the file's canonical path; the side table
  `module_names` maps a short stem to it via `resolve_module_by_name`.

- **The type-vs-module split** is the enum `MapDomain::{Type(GlobalId),
  Module(ModuleId)}` (`src/core/complex.rs`), the domain recorded with every map
  by `Complex::add_map`. It is precisely the two scales of this concept.

- **Inclusion.** `interpret_include_instr` and `interpret_include_module_instr`
  (`src/interpreter/include.rs`): `prefixed_generators` + `insert_generators_by_tag`
  copy the cells, `identity_map` (`src/interpreter/types.rs`) supplies the
  witnessing $\mathrm{id}$, registered under `MapDomain::{Type,Module}` via
  `Complex::add_map`. The skip-if-present test in `insert_generators_by_tag` is
  the idempotence above; the alias default and the *"Inclusion of non-local
  types requires an alias"* rule live in `resolve_include` (internal).

- **Attachment.** `interpret_attach_instr` →
  `extend_scope_with_attached_generators` (`include.rs`) is the pushout: it walks
  `sorted_generators`, skips those `map.is_defined_at`, applies $F$ to the
  boundary via `mapped_cell_data` (which calls `PartialMap::apply`), mints a
  fresh image cell — `GlobalId::fresh()` + `set_cell` in `Mode::Global`, or a
  `Tag::Local` via `add_local_cell` in `Mode::Local` (an `@Local` body) — and
  grows the map with `PartialMap::insert_raw`. The grading check is the
  `boundary_in.dim() != expected` guard. Only type domains may be attached:
  `interpret_attach_instr` rejects `MapDomain::Module`.

- **The $0$-cell-only law** for `@Type` blocks is enforced in
  `src/interpreter/eval.rs` — `interpret_type_generator` rejects a
  `CellData::Boundary` with *"Higher cells in @Type blocks are not supported"*.

- **The type lookup chain** (no `find_type_by_name`): module scope →
  `Complex::find_diagram` → `Diagram::top_label` → unwrap `Tag::Global` →
  `find_type`. Lives in `src/interpreter/resolve.rs` (`interpret_address` →
  `type_id_of_named_diagram` (internal) for the id, `resolve_type_complex` for
  the `find_type` step). See [[interpreter]] for the full walk.

- **Qualified names** are walked by `interpret_address` →
  `resolve_address_prefix_scope` (internal, `resolve.rs`): each prefix segment is
  looked up with `Complex::find_map`; its `MapDomain` must be `Module(id)`, and
  the scope advances to `find_module_arc(id)`. A `MapDomain::Type` prefix or a
  missing map is rejected. The final segment is resolved by
  `type_id_of_named_diagram` in the advanced scope.

- **Module-domain maps.** In a `@Type` block, `let F :: M = […]` resolves `M`
  as a *module*: `interpret_def_pmap_module` → `resolve_module_domain` →
  `GlobalStore::resolve_module_by_name` (single-segment short name →
  `(path, Arc<Complex>)`). In complex and `@Local` bodies the same syntax
  resolves a *type* domain (`interpret_def_pmap`).

- **Scoped include resolution** is owned by [[aux]] (`Loader::with_parent_dir`,
  `find_file`, `resolve_recursive`); the interpreter consumes the resulting
  `ModuleResolutions` map in `interpret_include_module_instr`
  (`context.resolutions.resolve(module_id, module_name)` → canonical path) and
  reads the pre-interpreted complex out of the store. The leaves-first
  topological interpretation is `InterpretedFile::load` ([[interpreter]]).
  Behaviour is pinned by `submodule_in_same_named_directory` and
  `virtual_loader_subdirectory_resolution` (`tests/interpreter.rs`): the latter
  has `A.ali` and `B.ali` each `include Aux`, resolving to the *distinct* files
  `A/Aux.ali` and `B/Aux.ali`, then addressing them as `A.Aux.Ob` / `B.Aux.Ob`.

- **Surface syntax.** `IncludeModule` (a `@Type`-block instruction) /
  `IncludeStmt` / `AttachStmt` in `src/language/ast.rs`; their productions
  (`include_module`, `include_stmt`, `attach_stmt`) in
  `src/language/parser.rs`, the keyword set in `src/language/lexer.rs`. The
  grammar is `docs/GRAMMAR.md`. No `open` token or production exists
  ([[module-open-semantics]]).

## Related

[[extension-inclusion-attachment]] · [[partial-map]] · [[pushout]] ·
[[core-complex]] · [[diagram]] · [[atom]] · [[directed-complex]] ·
[[regular-directed-complex]] · [[module-open-semantics]] · [[interpreter]] ·
[[language-parser]] · [[aux]]
