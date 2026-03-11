# Files and Modules

- The main way of organising data is a _module_.
- Each file corresponds precisely to a module. For example, `Module.ali`
  corresponds to `Module`.
- Each module holds a number of _complexes_. 
- You can include a module into another using
  ```
    include Module as m
  ```
  where `m` is any identifier. Then a complex `X` of `Module` is available as
  `m.X`.
- Modules consist of two types of blocks:
  - `@Type` blocks, which define new complexes.
  - `@C` blocks, which create local definitions in existing complexes.

# Complexes

- A complex is a finite cell complex.

## Generators

- The most primitive way to define a complex is a _generator_, which assigns a
  new complex to a name.

  The simplest kind of generator lists a sequence of cells, in increasing
  dimension. Later cells may be mention previous cells. For example, a complex
  representing a _walking object_ equipped with identity 1-cell may be given by
  ```
  Ob <<= {
    ob,
    id : ob -> ob
    idem : id id -> id
  }
  ```
  `ob` is a 0-cell. `id` is a 1-cell on `ob`. `idem` is a 2-cell that "rewrites"
  the composite of two `id`s to one. Notice that there is no 2-cell `id -> id
  id` allowing a rewrite in the opposite direction.

## Attachment

- Within a generator is it possible to _attach_ a copy of an already existing
  complex. For example, we can define a complex representing a _walking
  morphism_ by
  ```
  Mor <<= {
    attach dom :: Ob,
    attach cod :: Ob,
    mor : dom.ob -> cod.ob
  }
  ```
  The `attach` declaration attaches two copies of a walking object `Ob`, locally
  named `dom` and `cod`. The cells of these complexes can be accessed using name
  qualification. For example, the third line declares a 1-cell from the 0-cell
  of the object representing the domain, `dom.ob`, to the 0-cell of the object
  representing the codomain, `cod.ob`.

- Attachment may also happen _along_ a partial map. This enables us to identify
  cells of the complex that is being attached to cells of the complex that is
  being declared in this generator. The notation `attach c :: C along [ ... ]`
  adds a 'copy' `c` of `C` along the map defined in `[...]`. For example, we can 
  define a complex representing a _pair_ of composable morphisms by the
  generator

  The idea is
  that you end up with the pushout

  ```
     dom [ ... ]  -------->  C
         |                   |
  [...]  |                   |
         |                   |
         v                   v
         D      ---------->  D'
  ```
  where `[...]` defines a partial map from (part of) `C`, the complex we are
  attaching, `D` is the complex we have defined so far, and `D'` is the complex
  obtained after this attachment declaration.

  For example, we can define a _pair_ of composable morphisms by
  ```
  Pair <<= {
    attach x :: Ob,
    attach y :: Ob,
    attach z :: Ob,
    attach f :: Mor along [ dom => x, cod => y ],
    attach g :: Mor along [ dom => y, cod => z ]
  }
  ```
