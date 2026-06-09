---
kind: question
status: draft
last-touched: 2026-06-10
code: [src/core/diagram.rs, src/core/ogposet.rs]
---

# Does `parallelism` enforce (Atom)'s sign-restriction?

The book's (Atom) constructor (Hadzihasanovic 2024, 3.2.1 and 3.3.2) requires
an isomorphism $\varphi : \partial U \cong \partial V$ that **restricts** to
$\varphi^\alpha : \partial^\alpha U \cong \partial^\alpha V$ for each sign —
inputs glued to inputs, outputs to outputs. `Diagram::parallelism`
*(internal)* never checks this. It compares the canonical forms of the two
*whole* boundaries (`ogposet::boundary_traverse` with `Sign::Both`) for
positional equality, labels included, then glues index-wise
(`pushout::pushout` → `build_cell_shape`). The boundary sub-ogposet itself
cannot witness the hemisphere split — the cofaces into the top cells are
stripped — so sign-respect rides entirely on the traversal *order*.

## Why it might still be sound

The `Both` traversal (`ogposet::build_stack_cell_n`) is *phase-separated*: it
marks all of $\partial^- U$ before any cell of
$\operatorname{int} \partial^+ U$. The positional gluing therefore respects
signs **iff** the two canonical forms put their phase boundary at the same
index — iff $|\Delta^-_{n-1} U| = |\Delta^-_{n-1} V|$ for $n := \dim U$. This
is *forced* by equality of the canonical forms when $n \le 2$: for $n = 2$
the boundary is a directed circle, and the first output-hemisphere edge
carries the face row $(\{0\}, \{p{+}1\})$ — a restart at the globally-first
vertex — which no input-hemisphere row can equal. For $n \ge 3$ (generators
of dimension $\ge 4$) no proof is known, and restart-shaped rows occur
*within* a single hemisphere too, so the $n = 2$ argument does not
generalise.

## What is at stake

If a sign-mismatched pair ever passes, the glued cell shape is not globular,
hence not a [[molecule]], hence not a [[regular-directed-complex]] — and by
Proposition 5.3.15's contrapositive, the `(shape, labels)` encoding of
[[diagram|diagrams]] over that shape loses uniqueness: two distinct pasting
diagrams with the same representation. Nothing downstream re-checks
regularity, so the failure would be silent. This is also why "alifib
represents RDCs" overstates: values are colimits, and even the shape-level
invariant is, today, an article of construction discipline rather than a
theorem about the code.

## Possible resolutions

- Prove the traversal lemma: equality of `build_stack_cell_n` canonical forms
  forces the hemisphere splits to align.
- Make soundness unconditional: after the canonical forms match, additionally
  check that the positional isomorphism maps $\Delta^-_{n-1} U$ onto
  $\Delta^-_{n-1} V$ (one `Ogposet::extremal` call per side), and add a test
  feeding `parallelism` a sign-mismatched pair. Note `is_round` has no unit
  tests and `core/diagram.rs` has exactly one
  (`boundary_normal_clamps_history_to_top_dim`), so the gate is currently
  untested either way.

## Related

[[regular-directed-complex]] · [[molecule]] · [[atom]] · [[diagram]] ·
[[core-diagram]] · [[core-ogposet]] · [[0002-round-boundaries]]
