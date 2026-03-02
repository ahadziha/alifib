- You always need an empty complex. The reason is that the notation
  ```
  @ X { ...}
  ```
  locally extends the complex `X` with some additional generators. So it is
  possible to write
  ```
  @ { ... }
  ```
  and obtain a 'local' complex. That should extend the empty one.

- (0-dim) complex of modules: 0-cells are modules
  a module itself is a complex whose cells are (small) complexes
  a complex is a "purely syntactic" cell complex

- In a block `@X { ... }` any local definitions _will_ be added to `X`, but
  _only_ if, when unfolded, they use _just_ generators of `X`, no local
  generators found in `{ ... }`

- In a module, complex cells are subcomplexes of the 'global' complex.

- Named generators = name + reference to global cell

- In local blocks, local defns do NOT get added to the global complex

- An `include` statement in a complex block genuinely makes the complex a
  subcomplex of the current one.

- Reassigning to a name that's taken is illegal.

- Include shares, attach copies. For example
  ```
  @Type
  C <<= { c },
  D <<= { include C as E },
  E <<= { e, let f :: C = [ c => e ] } 
  @E
  let g :: D = f
  ```
  is perfectly fine, but
  ```
    @Type
    C <<= { c },
    D <<= { attach E :: C },
    E <<= { e, let f :: C = [ c => e ] } 
    @E
    let g :: D = f
  ```
  fails.

- `D <<= { include C as E }` includes the generators of `C` but qualified as
  `E`, wheras `D <<= C` does the same thing but without qualification

- The main reason for `include` in a complex block is that it allows definition
  of a maps by extension: define it on the subcomplex first, then extend it to
  a bigger thing.

- A _diagram_ `d` in a complex `X` is a morphism from a molecule `shape(d) ->
  X`.

- A _morphism_ is a map of complexes. The category DCplx is presheaves on atoms
  and their embeddings. A morphism in DCplx has to send generators to generators
  and cells to cells. There's a merge monad M on this category. If X is a
  directed complex then MX is the complex in which cells of shape U (an atom)
  are pairs (U ~> V, V -> X) where V is a round molecule and V -> X is a
  V-shaped diagram in X. Kleisli morphisms of M can send a cell to a round
  diagram (in the base cat: only generators to generators!). U ~> V is a
  _subdivision_ (in the paper with Clemence), a formal dual of a comap U <- V.

- When defining partial maps, the boundaries are inferred in two cases: if the
  input boundary is 'trivial' (e.g. just a single cell of some dimension), or if
  the shapes of the input and output perfectly match. Otherwise it can just
  fail.

- Equality is decidable for both diagrams and maps
