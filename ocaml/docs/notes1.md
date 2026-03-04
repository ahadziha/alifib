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

- [THIS IS BAD DON'T DO IT] In a block `@X { ... }` any local definitions _will_
  be added to `X`, but _only_ if, when unfolded, they use _just_ generators of
  `X`, no local generators found in `{ ... }`

- In a module, complex cells are subcomplexes of the 'global' complex.

- Named generators `X <<= ...` assign a complex to a name. Reassigning to a name
  that's taken is illegal.

- In local blocks, local defns do NOT get added to the global complex

- An `include` statement in a complex block genuinely makes the complex a
  subcomplex of the current one.


- `include` shares, `attach` copies. For example
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

- `D <<= { include C as E }` includes the generators of `C` but qualified by
  `E`, whereas `D <<= C` does the same thing but without qualification

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

- `[ F => G ]` is valid where `F` and `G` are maps with the same domain. This
  expands to `forall x : dom(F). F.x => G.x`.

- `.` is always composition.

  Example: `C.x` is always a composition of maps. 
  - If we are `@C` the `C` is the identity map. 
  - If we have attached `C` then `C` is the inclusion map. 

  Example: `C.x.in` is the composition
  `(C --inc--> D) o (shape(x) -x-> C) o (d^{-} shape(x) --inc--> shape(x))`

  Dots are always used like this:
  `(prefix (= map).("simple" diagram).(suffix (e.g. boundaries))`

  Another example: if I have
  ```
  @E  ... F :: D ..
  @D  ... G :: C ..
  ```
  then I have
  ```
  @E  ... F.G :: C
  ```
  At `E` we have `F : D -> E`. Then the dot makes us enter the namespace of `D`,
  where we can find `G`. We could have also named `G` as `F` with `F.F` being
  perfectly valid!

- A diagram is a map whose domain is a molecule. In the implementation it's
  slightly different though! A map is implemented as a table that maps
  generators (or rather, ids of generators) to diagrams. A diagram is
  implemented as an ogposet plus a labelling in ids, plus some information about
  how it was built. The only way to interact with maps is to compose them. When
  you have diagrams you can paste them, or apply a map to them, or take their
  boundary. A diagram is a map whose domain is implicit (a molecule).

- Ideally we'd like
  - Gray products
  - `attach` for modules. For example, in `Module.ali`:
    ```
    @Type
    X <<= { x, a : x -> x }
    Y <<= X { m : a a -> a }
    ```
    While in `Test.ali`:
    ```
      @Type
      A <<= { x, b : x -> x, let a = b b },
      attach F :: Module along [ X => A ]
    ```
    This will _automatically_ add `F.Y` which should be `A { m : a a -> a }`
  -  `@Type` is also a complex so eventually we should have
      ```
      @Type
        A <<= { ...},
        B <<= { ...},
        F : A -> B <<= { ... }
      ```
      where one should think of `F` as the collage of a profunctor. Rewriting-wise
      these should be something like partial, nondeterministic transducers.

- Implementation notes
  - For every generator of a named complex there's a global unique id.
  - There's a table (global unique ids) -> (molecule + labelling in ids). In fact
    the only thing that matters is the boundaries.
  - Every named complex is an isomorphism (named generators) <-> (unique ids).
  - The domain of a map is a list of ids.
  - Diagrams are stored as they are constructed. So for example interchange is
    NOT strict equality. To check equality you need to traverse to normalize.
    When stored there's a flag that records whether something has been
    normalized before. Rewalt normalized every time and it was a huge
    bottleneck.
