---
kind: concept
status: draft
last-touched: 2026-06-11
---

# Extension, inclusion, attachment — three ways to build on a complex

## The question this page answers

You have a type `Y` — a generator together with the [[core-complex|complex]]
grown in its body — and you want to define a new type `X` that *builds on* `Y`.
alifib's surface gives you three ways to write that, and they look almost alike:

```
X <<= Y { … }              -- extension
X <<= { include Y, … }     -- inclusion
X <<= { attach B :: Y, … } -- attachment
```

They are **not stylistic variants of one operation.** They differ on three
independent axes — *what names `Y` arrives under*, *whether `X` shares `Y`'s
cells or gets fresh copies of them*, and *what map (if any) records the
relationship*. Pick the wrong one and your boundaries refer to names that aren't
there, or you silently get two disjoint copies of `Y` where you wanted one. This
page works one small example through all three so the differences are concrete.

The right-hand side of `<<=` is always a `Complex` ([[language-parser]]); the
three forms are three shapes of that complex. Extension is the *parent address*
of an inline block; inclusion and attachment are *instructions inside the
block*. [[module-system]] introduces inclusion and attachment as "the two
canonical ways one [[directed-complex]] embeds in another"; extension is a third,
orthogonal thing — not an embedding but plain inheritance.

## The running example

Take a single object with one endomorphism:

```
Ob <<= {
  pt,
  ob : pt -> pt
}
```

`Ob`'s complex holds a $0$-cell `pt`, a $1$-cell `ob : pt → pt`, and `Ob`'s own
identity self-map (named `Ob`). We now build a *magma* — `Ob` plus a
multiplication $m \colon ob \#_0 ob \Rightarrow ob$ — three ways.

### 1. Extension — `Magma <<= Ob { … }`

```
Magma <<= Ob {
  m : ob ob -> ob
}
```

The parent address `Ob` is resolved and **its whole complex is cloned as the
starting scope** (`open_type_scope`: `working_complex: (*Y).clone()`,
`src/interpreter/resolve.rs`). Then the body runs on top. So:

- `Ob`'s cells keep their **original, flat names**: `pt` and `ob`, *no prefix*.
  That is why the body can write `m : ob ob -> ob` referring to `ob` bare.
- `Ob`'s **maps are inherited wholesale** — including `Ob`'s own self-map `Ob`.
- **No new map is recorded** for the parent relationship. `Magma` simply *is* an
  extended copy of `Ob`; nothing marks "Magma extends Ob".

This is inheritance. It is the most common form in the example library —
`Parser <<= Term { … }` (`examples/SKI_Term.ali`), `AnBn <<= TM { … }`
(`examples/TM.ali`), `Dunce <<= S1 { … }` (`examples/Delta_complexes.ali`) — and
the golden snapshots `golden_ski` / `golden_tm` / `golden_delta_complexes`
(`tests/golden_examples.rs`) pin that these elaborate.

One gotcha falls straight out of "maps are inherited": if the new type's own
name collides with a map inherited from the parent, elaboration errors with
*"Type name '…' collides with an inherited map of the same name"*
(`interpret_type_generator`, `src/interpreter/eval.rs`).

### 2. Inclusion — `Magma <<= { include Ob, … }`

```
Magma <<= {
  include Ob,
  m : Ob.ob Ob.ob -> Ob.ob
}
```

Now the block has **no parent address**, so the starting scope is the module
*root* — an empty complex (`resolve_root_owner_type_id`). The `include`
instruction (`interpret_include_instr`, `src/interpreter/include.rs`) then:

- copies `Ob`'s generators in **under the alias as a prefix**: `Ob.pt`, `Ob.ob`
  (`prefixed_generators` + `qualify_name("Ob", "ob") = "Ob.ob"`). That is why the
  body must now write `Ob.ob`, not `ob`.
- copies each generator **keeping its `GlobalId`** — so `Magma`'s `Ob.ob` *is*
  the very same cell as `Ob`'s `ob`, shared by identity, not a copy. Re-including
  the same type is therefore idempotent (`insert_generators_by_tag` skips a tag
  already present).
- records an **identity inclusion map** named `Ob`, with domain
  `MapDomain::Type` — a first-class [[partial-map]] witnessing the sub-complex
  embedding $Ob \hookrightarrow Magma$, usable as a qualified-address prefix.

`include` also works for a *whole module* (`include M [as A]`, the `@Type`-block
form), dropping `M`'s unnamed root; `attach` does not. Including a non-local
(dotted) type requires an explicit `as` alias — e.g.
`include TRS.Unit as Unit` (`examples/BinaryNat.ali`).

### 3. Attachment — `Magma <<= { attach Ob :: Ob, … }`

This is the form the real fixture uses (`tests/fixtures/Magma.ali`, pinned by
`magma_interpretation` in `tests/interpreter.rs`):

```
Magma <<= {
  attach Ob :: Ob,
  m : Ob.ob Ob.ob -> Ob.ob
}
```

`attach B :: Y [along F]` is a [[pushout]] (`interpret_attach_instr` →
`extend_scope_with_attached_generators`, `src/interpreter/include.rs`). For every
generator of `Y` **not** in the domain of `F` it **mints a fresh cell** —
`GlobalId::fresh()` — whose boundary is the `F`-image of `Y`'s boundary;
generators *in* dom(F) are identified with the cells `F` sends them to. Names are
prefixed exactly as for inclusion (`Ob.pt`, `Ob.ob`), and a map named `B` is
recorded.

The decisive difference from inclusion is **freshness**. With the empty `along`
(as above) *every* cell is reborn: `attach Ob :: Ob` adjoins a **fresh, disjoint
copy** of `Ob`. Attach the same type twice and you get two independent copies;
include it twice and you get one shared sub-complex.

Why freshness earns its keep is visible one type later, in the fixture's
Frobenius magma:

```
FrobeniusMagma <<= {
  attach Ob :: Ob,
  attach Magma   :: Magma   along [ Ob => Ob ],
  attach Comagma :: Comagma along [ Ob => Ob ]
}
```

A fresh `Ob` is laid down, then `Magma` and `Comagma` are each glued on **along a
map that identifies their `Ob` with the one already present**. The result
(`magma_interpretation` checks it cell-for-cell) is a single shared object
carrying both `Magma.m` and `Comagma.c` — a genuine colimit. Inclusion cannot
express this: it can share one `Ob`, but it has no `along` clause with which to
*identify* `Magma`'s copy of `Ob` with the ambient one.

## The three at a glance

| | extension `X <<= Y { … }` | inclusion `… { include Y }` | attachment `… { attach B :: Y along F }` |
|---|---|---|---|
| Starting scope | clone of `Y`'s complex | empty (module root) | empty (module root) |
| `Y`'s names in `X` | flat (`ob`) | prefixed (`Y.ob`) | prefixed (`B.ob`) |
| `Y`'s cell identity | **shared** (clone keeps ids) | **shared** (same `GlobalId`) | **fresh** (`GlobalId::fresh`) |
| Map recorded | none (inherits `Y`'s maps) | identity, `MapDomain::Type` | the pushout map `B` |
| Can identify/glue cells | no | identity only | yes, via `along F` |
| Domain admitted | type only | type **or** module | type only (rejects `MapDomain::Module`) |
| Idempotent on repeat | — | yes (skip-if-present) | no (fresh copy each time) |

The misreading to resist: *"all three just bring `Y` into `X`."* They do, but
along different axes. Two of them (extension, inclusion) **share** `Y`'s cells;
only attachment **copies** them. Two of them (inclusion, attachment) **namespace**
`Y` under a prefix and record a map; extension keeps `Y` flat and records
nothing. If you remember one sentence: *extension inherits, inclusion embeds,
attachment glues.*

## Implementation

All three flow through `interpret_complex` (`src/interpreter/eval.rs`), which
splits on the parsed `ast::Complex`:

- **Extension** is `Complex::Block { address: Some(Y), … }`. The address is
  resolved by `resolve_type_scope` → `open_type_scope`
  (`src/interpreter/resolve.rs`), whose `working_complex` is `(*Y).clone()` — the
  clone *is* the inheritance. The body is then folded over that scope by
  `interpret_complex_body`. No `add_map` happens for the parent; the only map
  `interpret_type_generator` adds is `X`'s own self-identity, and its collision
  check against inherited maps is the *"collides with an inherited map"* guard.
  *(Construction discipline / direct code fact — not a theorem.)*

- **Inclusion** is `Complex::Block { address: None, … }` with an `IncludeStmt` in
  the body. `interpret_include_instr` calls `prefixed_generators` (prefix =
  alias, names via `qualify_name`) and `insert_generators_by_tag` (tags
  preserved → shared identity; skip-if-present → idempotence), then
  `add_map(alias, MapDomain::Type(id), identity_map(…), …)`. The "non-local
  include needs an alias" rule lives in `resolve_include` (internal).

- **Attachment** is an `AttachStmt` in the body.
  `extend_scope_with_attached_generators` walks `sorted_generators`, skips those
  `map.is_defined_at`, transports the boundary through `F` via `mapped_cell_data`
  (`PartialMap::apply`), and mints `GlobalId::fresh()` for the rest — the
  cell-by-cell [[pushout]] of [[module-system]]. The grading guard
  (`boundary_in.dim() != expected`) rejects an ill-graded gluing. Only a type
  domain is accepted: `interpret_attach_instr` rejects `MapDomain::Module`
  outright.

What is checked vs assumed: the *names*, *identity-sharing*, *prefixing*, and
*recorded maps* above are exactly what these functions do — read them, or read
the cells/maps `magma_interpretation` asserts. That attachment computes the
**pushout's universal property** is the mathematical reading of the cell-by-cell
construction ([[pushout]], [[module-system]]); it is a framing of the algorithm,
not a separately-checked invariant.

Behavioural pins: `magma_interpretation` (`tests/interpreter.rs`) — attach with
and without `along`, prefixing, and the FrobeniusMagma gluing, asserted
cell-for-cell; the `golden_*` snapshots (`tests/golden_examples.rs`) — the
extension form across the real library.

## Related

[[module-system]] — the parent concept: types/modules, and inclusion/attachment
as the two canonical embeddings · [[partial-map]] — the maps `include`/`attach`
record and the `along F` clause · [[pushout]] — the colimit attachment computes ·
[[core-complex]] — the scope each form threads · [[trs-encoding]] — `include TRS`
+ `attach` per operation, the pattern in the wild · [[language-parser]] — the
`<<=` head and `Complex::Block` it parses to.
