---
kind: concept
status: stable
last-touched: 2026-06-01
---

# Module system

A program in alifib is a forest of named [[core-complex|complexes]] organised at
two scales. A **type** is a single [[diagram|diagram]]-generator together with
the whole complex grown inside its body — an [[atom|atom]] equipped with its own
ambient universe of cells, maps, and lower atoms. A **module** is a `.ali` file:
the complex accumulated by elaborating all of its top-level declarations, keyed
by its canonical path. The module system is the calculus by which one such
complex is carried into another — by **inclusion** (`include`), by
**attachment along a [[partial-map]]** (`attach … along`), and (aspirationally,
see [[module-open-semantics]]) by **opening** a name without importing its
universe.

Mathematically these are not bookkeeping conveniences. They are the two
canonical ways one [[regular-directed-complex]] embeds in another: as a
*sub-complex* (an inclusion, the identity on shared generators) or as a *gluing*
(a pushout-style colimit that freely adjoins the un-identified cells of one
complex along a map). The whole system is the syntax of these colimits.

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
`include S as p` realises the **inclusion** $\iota \colon \mathcal{C}_S
\hookrightarrow \mathcal{C}$. Concretely it copies every generator of $S$ into
$\mathcal{C}$ under the prefix $p$, and records the identity [[partial-map]]
$\mathrm{id}_{\mathcal{C}_S}$ as the witnessing map. Inclusion is *eager and
total*: nothing of $S$ is left behind, and no cell is renamed up to its prefix.
Re-including an already-present generator is idempotent (the second copy is
skipped). The top-level form `include M` does the same for a whole module
$M$, dropping $M$'s unnamed root generator.

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
already defined. This is exactly the universal property of a pushout

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

### Opening (aspirational)

`open M` is intended to bring a *name* from module $M$ into scope **without**
importing $M$'s universe — a lazy, single-name binding that lets a map out of
part of $M$ be defined without forcing the rest of $M$ to load. As of the
current source there is **no `open` keyword**: the lexer's keyword set
(`src/language/lexer.rs`, `ident_or_nat_or_kw`) is `include`, `attach`, `along`,
`assert`, `in`, `out`, `Type`, `let`, `total`, `map`, `as`, `index`, `for`,
`bar`, `run` — no `open` token exists. The distinction between this lazy `open`
and the eager `include` is the substance of the open question
[[module-open-semantics]].

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
  `Complex::add_map`. The
  skip-if-present test in `insert_generators_by_tag` is the idempotence above.

- **Attachment.** `interpret_attach_instr` →
  `extend_scope_with_attached_generators` (`include.rs`) is the pushout: it walks
  `sorted_generators`, skips those `map.is_defined_at`, applies $F$ to the
  boundary via `mapped_cell_data` (which calls `PartialMap::apply`), mints a
  fresh image cell — `GlobalId::fresh()` + `set_cell` in `Mode::Global`, or a
  `Tag::Local` via `add_local_cell` in `Mode::Local` (an `@Local` body) — and
  grows the map with `PartialMap::insert_raw`. The grading check is the
  `boundary_in.dim() != expected` guard.

- **The $0$-cell-only law** for `@Type` blocks is enforced in
  `src/interpreter/eval.rs` — `interpret_type_generator` rejects a
  `CellData::Boundary` with *"Higher cells in @Type blocks are not supported"*.

- **The type lookup chain** (no `find_type_by_name`): module scope →
  `Complex::find_diagram` → `Diagram::top_label` → unwrap `Tag::Global` →
  `find_type`. Lives in `src/interpreter/resolve.rs` (`interpret_address` →
  `type_id_of_named_diagram` (internal) for the id, `resolve_type_complex` for
  the `find_type` step). See [[interpreter]] for the full walk.

- **Surface syntax.** `IncludeModule` / `IncludeStmt` / `AttachStmt` in
  `src/language/ast.rs`; their statement productions (`include_stmt`,
  `attach_stmt`) in `src/language/parser.rs`, the keyword set in
  `src/language/lexer.rs`. The grammar is `docs/grammar.md`. No `open` production
  exists.

## Related

[[partial-map]] · [[core-complex]] · [[diagram]] · [[atom]] ·
[[regular-directed-complex]] · [[module-open-semantics]] · [[interpreter]] ·
[[language-parser]]
