---
kind: concept
status: draft
last-touched: 2026-06-10
---

# Encoding term rewriting systems

How do you run a term rewriting system in a language whose values are
[[string-diagram|string diagrams]], not trees? A term like $\mathrm{add}(x, x)$
is a tree; alifib has no trees, only [[diagram|diagrams]]. The answer is the
standard one for presenting an algebraic theory string-diagrammatically: make
the ambient category **cartesian**, so that its morphisms behave like functions
on terms — you may *share* a variable and *drop* an unused one — and then every
operation symbol is a node, every term is a string diagram, and every rewrite
rule is a directed 3-cell.

`examples/TRS.ali` is the reusable **structural layer** that makes a sort
cartesian and equips every operation with the bookkeeping it needs. It is *not
itself* a term rewriting system: it declares no rule of any actual theory, and
indeed every cell in it is marked `thin`. A concrete TRS — say
`examples/BinaryNat.ali` — `include`s it, `attach`es its operations, and adds
its own rules on top.

> **The tempting misreading.** "`TRS.ali` is a term rewriting system." It is the
> opposite: it is the *cartesian scaffolding shared by all* TRSs over this kind
> of signature — the part with no computational content of its own. The
> computation lives in the rules a concrete theory adds (below).

## Terms as string diagrams

The dual presentation ([[string-diagram]]) reads dimensions backwards: a
top-dimensional cell is a *node*, a codimension-1 cell a *wire*. In this
encoding a **term of sort $A$ is a 2-dimensional diagram**, so:

| ingredient | alifib cell | dimension | drawn as |
|---|---|---|---|
| a sort $A$ | `Sort.wire` | 1-cell | a wire |
| an operation $f$ | `node` | 2-cell | a node |
| a term $t : A_1 \cdots A_n \to B$ | a 2-diagram | 2 | a string diagram |
| a rewrite $t \Rightarrow t'$ | a 3-cell $\partial^- = t,\ \partial^+ = t'$ | 3 | a move |

A **sort** is brought in by `attach Nat :: TRS.Sort`; its `wire` (the term's
type) is the one piece left *non-thin*, so it is drawn solidly. An **operation**
is attached by arity — `TRS.Constant` (nullary), `TRS.Unary` ($1\to1$),
`TRS.Binary` ($2\to1$) — wiring the operation's sorts and naming its `node`,
again non-thin. So in `BinaryNat`:

```ali
attach Nat :: TRS.Sort along [ Unit => Unit ],
zero: unit -> nat,   attach Zero :: TRS.Constant along [ Cod => Nat, node => zero ],
0:    nat -> nat,    attach Bit0 :: TRS.Unary    along [ Dom => Nat, Cod => Nat, … ],
add:  nat nat -> nat, attach Add :: TRS.Binary   along [ LDom => Nat, RDom => Nat, … ],
```

A closed (ground) term is then a 2-diagram all of whose input wires are
**unit** wires — no free sort-variable wire dangles. The numeral
`n5 = zero 1 0 1` is the four operation-nodes `zero`, `1`, `0`, `1` pasted in
sequence (`#_1` along the shared `nat` wire), bottoming out at the nullary
`zero`.

## Why copy and discard: Fox's theorem

A bare symmetric monoidal category is *linear*: a wire is a resource, used
exactly once. Terms are not linear — $\mathrm{add}(x,x)$ uses $x$ twice and a
projection drops a variable. **Fox's theorem** (T. Fox, *Coalgebras and
Cartesian categories*, Communications in Algebra 4, 1976) says exactly what
upgrades a symmetric monoidal category to a **cartesian** one (where $\otimes$
is the categorical product and the unit is terminal): every object must carry a
cocommutative comonoid

$$\Delta_A : A \to A \otimes A \quad(\text{copy}), \qquad \varepsilon_A : A \to I \quad(\text{discard}),$$

**naturally** — every morphism is a comonoid homomorphism — and compatibly with
$\otimes$. That is the whole of why `Sort` declares

```ali
copy:    wire -> wire wire,    discard: wire -> unit,
```

and why each operation ships a naturality suite, the `*_Nat` cells. `copy`
($\Delta$) and `discard` ($\varepsilon$, landing in the unit $I$) are the
sharing and dropping of variables; the naturality cells are Fox's condition
that operations commute with them, oriented as directed rewrites that push
copy/discard *towards the leaves*:

- `Unary.Copy_Nat : node #1 Cod.copy ⟶ Dom.copy #1 (node #0 node)` — "$f$
  commutes with $\Delta$": copying $f$'s output is copying its input and applying
  $f$ to both.
- `Unary.Discard_Nat : node #1 Cod.discard ⟶ Dom.discard` — "$f$ commutes with
  $\varepsilon$": discarding $f$'s output discards its input.
- `Binary.Copy_Nat` carries the $\otimes$-compatibility visibly: copy both
  inputs, `LDom_RDom.swap` the middle pair to regroup, apply the node twice —
  i.e. $\Delta_{A\otimes B} = (\mathrm{id}\otimes\sigma\otimes\mathrm{id})\circ(\Delta_A\otimes\Delta_B)$.
- At the leaves, `Constant.Copy_Nat : node #1 copy ⟶ split (node #0 node)` and
  `Constant.Discard_Nat : node #1 discard ⟶ id` absorb the structure entirely —
  a constant is freely duplicated and freely deleted.

**The comonoid laws are deliberately absent.** `Sort` declares the comonoid
*generators* and their *naturality*, but **not** coassociativity,
cocommutativity (`copy #1 swap ⟶ copy`), or the counit law
(`copy #1 (discard ‖ wire) ⟶ wire`). This is by design, not an omission. alifib
mints a cell only for a *move it needs*; the comonoid laws are equalities that
hold on the nose in the intended semantics (sets and functions: $\Delta$ is the
diagonal, $\varepsilon$ the unique map to a point), not computational steps. And
since the language has no [[0001-no-identities|identity cells]] and no equational
quotient, an equality that is not a step is simply not represented. The wager —
that on *closed* terms the naturality cells suffice to drive every copy/discard
down to the constants without ever needing to reassociate or commute them on a
bare wire — is the substance of the convergence claim below, and is
[[trs-convergence|not yet proved]].

## The unit, unitors, and the Frobenius bookkeeping

Arities change under rewriting — copying a node turns one output into two,
discarding turns a term into nothing — so the encoding needs an algebra of
"empty" wires to absorb the slack. That is the **`Unit`** module: a wire `pt ->
pt` carrying a (special, commutative) Frobenius structure `merge`/`split` with
its laws `Assoc`/`Coassoc`/`LFrobenius`/`RFrobenius`/`Split_Merge`, where
`discard` lands and where a binary `Discard_Nat` merges two discarded units back
into one.

A **Frobenius algebra** is a single object that is at once a monoid (`merge`,
$\mu : A\otimes A \to A$) and a comonoid (`split`, $\delta : A \to A\otimes A$),
the two halves interlocked by the **Frobenius law**: sliding a split through a
merge lands in the same place whichever side you do it — `LFrobenius`/`RFrobenius`
are its two zig-zags, both equal to $\delta\circ\mu$ (the diagram `merge split`).
**Commutative** adds that `merge` ignores the order of its inputs; **special**
(separable) adds that split-then-merge is trivial,
`Split_Merge : split merge ⟶ id` ($\mu\circ\delta = \mathrm{id}$). Special kills
handles, so the normal-form theorem is maximally tight: every connected network
of merges and splits collapses to one canonical *spider* fixed solely by its
input/output count — a tangle of unit wires carries no information beyond *which
endpoints are joined*. (The classical backdrop: commutative Frobenius algebras
are exactly 2-dimensional TQFTs, surfaces classified by genus and boundary;
"special" forces genus $0$.) That connectivity-only bookkeeping is exactly what
lets the unit soak up the arity shifts `copy` and `discard` create. Note `Unit`
declares only this $\mu$/$\delta$ core and its laws — there is no separate unit
$\eta$ or counit $\varepsilon$ cell; those roles, where the encoding needs them,
fall to `Sort`'s unitors and `discard`.

`Sort` then adds **unitors** `lunit`/`runit` (and inverses) letting a
unit wire be introduced beside or absorbed into a sort wire. The unitors are
given in *both directions* as separate cells — that is how an isomorphism is
presented when there are no identities to express its inverse, not a
non-termination bug.

## Rules: structural ($E$) vs. user ($R$)

Every rewrite — structural or user-supplied — is a **3-cell** $r$ with input
$\partial^- r$ the pattern term and output $\partial^+ r$ the result;
[[rewriting]] matches $\partial^- r$ inside the current 2-diagram and substitutes
$\partial^+ r$ by a [[pushout]]. There is no distinction *in the engine* between
the two kinds of rule. The distinction is conceptual:

- **$E$ — the structural theory.** Everything in `TRS.ali`: the Frobenius
  bookkeeping, the comonoid generators, the unitors and swaps, and all the
  `*_Nat` naturality cells. This is the cartesian plumbing, shared by every TRS.
- **$R$ — the rewrite rules of a concrete theory.** Added by a downstream
  module as fresh, non-thin 3-cells. In `BinaryNat`:

  ```ali
  Succ_Bit0: 0 succ -> 1,              (* 2·x then +1, low bit 0 ⟶ low bit 1 *)
  Add_11: (1 #0 1) add -> cadd 0,      (* (2x+1)+(2y+1) ⟶ carry into 2(…)  *)
  Mul_Bit0: (0 nat) mul -> mul 0,      (* (2x)·y ⟶ 2·(x·y)                  *)
  ```

The intended reading of the header comment — that the encoding "preserves
confluence / termination / convergence on closed terms" — is the statement that
$R$'s behaviour as a term rewriting system is faithfully simulated by rewriting
$R \cup E$ on closed string-diagram terms. What exactly that means and what is
established is collected in [[trs-convergence]].

## `thin` has no semantic force

It is tempting to think `thin` is what makes $E$ "silent" so that only $R$
counts as real rewriting. It is not: `thin` is a **display annotation only**. It
greys and shrinks the cartesian plumbing on the [[string-diagram]] canvas so a
reader sees the term and its operations rather than the bureaucracy (`Sort.wire`
and operation `node`s stay solid; unit wires, copy/discard/swap, unitors, and
`*_Nat` squares fade). The rewriting engine never reads the `thin` index — to
the matcher, a `*_Nat` cell is an ordinary rule like any other. Whatever makes
the dynamics converge is therefore a property of the *cells and the rewriting
strategy*, not of `thin`.

## Implementation

This is an encoding *in the language*, so its "implementation" is the example
sources plus the engine that elaborates and runs them.

- **Source.** The structural layer is `examples/TRS.ali` (the `Unit`, `Sort`,
  `Pair`, `Constant`, `Unary`, `Binary` modules, plus the `*_Nat` variants for
  naturality against a third sort) and its `examples/TRS/Aux.ali` (the
  identity-bearing `Wire` and the `Node_n_to_m` node gadgets). A concrete TRS is
  `examples/BinaryNat.ali`.
- **Layering is pushout.** `include TRS` and each `attach _ :: TRS.Sort/Constant/
  Unary/Binary along [ … ]` are realised by [[module-system]] / [[interpreter]]:
  inclusion copies $E$ in, attachment glues a fresh operation onto the host along
  the [[partial-map]] in the `along` clause (a [[pushout]] computed cell-by-cell).
  This is why one `TRS.ali` serves every theory — each `attach` mints fresh,
  correctly-bounded copies of the naturality cells for the new operation.
- **Rewriting is uniform.** A rule is a 3-cell; [[rewriting]] / [[core-matching]]
  match its 2-diagram input and substitute its output. Nothing privileges $E$
  over $R$; the `start Unit 'split merge' id` REPL session rewrites the 2-diagram
  `split merge` to `id` via the structural 3-cell `Split_Merge` exactly as a user
  rule would fire.
- **`thin` is display-only.** Seeded from `Complex::find_index("thin")`,
  propagated along attach-maps, and emitted per type for the canvas
  ([[interactive-daemon-web]], [[string-diagram]]); the engine ignores it.
- **Convergence is unchecked.** No code verifies confluence, termination, or the
  faithfulness of the simulation — these are claims about the encoding, recorded
  in [[trs-convergence]], not invariants the interpreter enforces.

## Related

- [[string-diagram]] — the dual presentation a term is drawn in.
- [[rewriting]] · [[pushout]] · [[core-matching]] — how a rule (3-cell) rewrites
  a term (2-diagram).
- [[module-system]] · [[partial-map]] — `include` / `attach … along`, by which a
  concrete TRS layers $R$ over $E$.
- [[0001-no-identities]] — why unitors and other isos appear as opposing cell
  pairs rather than invertible cells.
- [[trs-convergence]] — the open question: does this preserve confluence /
  termination / convergence on closed terms?
