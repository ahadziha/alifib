---
kind: impl
status: stable
last-touched: 2026-06-03
code: [src/interactive/engine.rs]
---

# interactive-engine — the rewrite session

> A session is a cursor walking through a sequence of rewrites. The engine holds
> the loaded context, the current $n$-[[diagram]], and a list of applied
> $(n+1)$-steps. There is **no disk format and no move log**: a session's durable
> form is the *proof diagram itself* (an `.ali` term), and `resume` rebuilds a
> session by taking that diagram apart. The engine never re-runs [[core-matching]]
> from scratch — it precomputes [[rewriting|rule patterns]] once and reuses them
> every step.

`RewriteEngine` drives [[rewriting]] interactively: you start from an initial
diagram, apply rules one at a time (or auto, random, parallel), undo/redo along
the line, and finally assemble the recorded steps into one $(n+1)$-proof diagram.
It is the in-memory state the shared [[interactive-session|`Session`]] wraps — the
REPL, daemon, and web all drive it through `Session::apply`, never directly — and
an interactive [[hole|hole]]-fill of dimension $\ge 1$ drives one too (see
[[interactive-session]]). The engine has no command-handling method of its own;
it exposes `step`/`undo`/`auto`/`assemble_proof`/… and the `Session` sequences
them.

## What it owns

`engine.rs` owns the *mutable session*: which rewrites are currently applicable,
the cursor into the step history, the undo/redo buffer, and the on-demand
assembly of the proof. It is a thin orchestration layer over the core pipeline —
all the real matching, pushout, and reconstruction lives in [[core-matching]].
Persistence is no longer a module of its own (`session.rs` was retired): the only
durable artefact is the proof term, rendered by `proof_expr` and re-ingested by
`resume`.

## Key public types

| Type / fn | Role |
|---|---|
| `RewriteEngine` | the whole session; immutable context + mutable cursor state |
| `HistoryEntry` *(crate)* | `{ rule_name, choice: Option<Vec<usize>> }` — one applied step, **display only** (`choice` is the picked rewrite indices, `None` for an `auto`/`resume` step). There is no replay |
| `ProofCache` | `{ snapshot: Diagram, at_step }` — incrementally-extended assembled proof, active only under proof view |
| `load_file_context` / `reevaluate` / `resolve_type` / `load_type_context` | free fns that load (or re-evaluate) a `.ali` file into a `GlobalStore` and resolve a type [[core-complex|complex]] |
| `eval_diagram_expr` | resolve a diagram by *name* (fast path) or by *parsing+interpreting* an expression (slow path) |

## Data flow

### Construction

Three live constructors, all taking an *already-loaded* store (the `Session`
loads the file before building, via `load_file_context` / `reevaluate`):

```
from_store   (start)        from_diagrams  (a hole-fill)        resume   (from a proof diagram)
     │                          │                                    │
resolve_type (TypeName → Arc<Complex>)   given initial/target        eval proof d (dim n+1);
     │                          Diagrams directly                    pseudo_normalise its paste tree
from_store:                                                          flatten_at(n) ⇒ each subtree a step
  eval initial (+target?)   from_diagrams:                           (realise_tree); label by top_generators;
  check_parallel              check_parallel(initial, target)        reverse if backward
     │                          │                                    │  initial = ∂(initial_sign) of d
     │                          │                                    │  current = ∂(step_sign) of last step
     ▼                          ▼                                    ▼  target = the supplied goal, not from d
  build_rule_patterns(type_complex, n, backward)        ← per-rule RulePattern, ONCE
  collect_confirmed_matches(current)                    ← applicable rewrites
     ▼
  RewriteEngine { current_diagram, steps, history, active_len, rewrites, rule_patterns, … }
```

- **`from_store`** is the *start* path (`Session::start_rewrite`): begin from an
  `initial_diagram` (a name or a diagram expression) resolved by
  `eval_diagram_expr`, with an optional `target` and an explicit `backward` flag;
  `steps` and `history` start empty.
- **`from_diagrams`** is the same, but takes the initial and target as *already-built*
  `Diagram`s plus display names. Its caller is `fill.rs::start_fill`
  ([[interactive-session]]): a hole's realised boundary diagrams become a rewrite
  from `F(x.in)` to `F(x.out)`, named `?x.in`/`?x.out`.
- **`resume`** begins from a finished **proof diagram** `d` of dimension $n+1$. It
  pseudo-normalises `d`'s paste tree ([[core-paste-tree]]), `flatten_at(n)`s the
  outermost $\#_n$ chain into the individual rewrite steps, `realise_tree`s each,
  and labels it by its top `(n+1)`-generators (`top_generators`). The session then
  *is* that proof, every step already applied: the initial diagram is `d`'s input
  boundary (forward) or output boundary (backward), the current diagram is the
  opposite boundary, and `assemble_proof` reproduces `d`. The `target` is the
  supplied goal — the *original* session's target — never read off `d`.

A fourth constructor, `init` (load from a path, then build), survives in the
source but is **currently unused** — the daemon's old pre-load path now goes
through `Session::from_disk` instead. Treat it as legacy until rewired or removed.

### One manual step

`step(choice)` indexes the precomputed `rewrites: Vec<MatchResult>`, clones the
stored $(n+1)$-step, derives the new current $n$-diagram from it, and refreshes
the applicable rewrites:

```
step(choice)
   │ pr = rewrites[choice]                       (a confirmed MatchResult)
   │ step = pr.step.clone()                        (the (n+1)-diagram)
   │ current = Diagram::boundary(step_sign, n, step)
   │           step_sign = Output (fwd) | Input (bwd)
   │ truncate_redo(); push step + HistoryEntry; active_len = steps.len()
   ▼ refresh_rewrites()  →  collect_confirmed_matches(current)
```

The crucial move is `Diagram::boundary(step_sign, n, step)`: the next $n$-diagram
is *read off the boundary of the just-applied $(n+1)$-step*, not recomputed from
the rule. Forward rewriting takes the output boundary $\partial^+_n$
(`Sign::Output`); backward takes the input boundary $\partial^-_n$ (`Sign::Input`).
This is the one rule in `step_sign` (internal).

### Auto, random, parallel

- `auto(max_steps)` loops: stop if `target_reached()` or budget exhausted,
  otherwise take *one* step. In parallel mode that step is
  `greedy_parallel_auto_step` (a whole compatible family glued at once via
  multi-pushout); otherwise the first confirmed singleton, found lazily. Returns
  `(applied, stop_reason)`.
- `random(max_steps)` picks a uniform index into the current `rewrites` via the
  session's seeded `Xoshiro256PlusPlus` and steps.
- `step_multi(choices)` is *manual* parallel: it checks the chosen matches are
  pairwise disjoint by `image_positions`, then glues them through the family path
  in [[core-matching]].

`set_parallel(on)` toggles parallel mode but affects **only** `auto` —
`refresh_rewrites` always lists individual matches for manual selection.

### Cursor: undo / redo / seek

All cursor motion sets `active_len` and restores `current_diagram` (the initial
diagram at 0, else the `step_sign` boundary after `steps[active_len-1]`), then
refreshes. `undo`/`undo_all`/`undo_to`/`redo`/`redo_to` are thin wrappers. Undone
steps stay in `steps`/`history` beyond `active_len` as a *redo buffer*; only a
genuinely new `step`/`step_multi`/`auto` calls `truncate_redo` to discard it.

### Assembling and persisting the proof

`steps` are stored **un-pasted** — each is a single rewrite $(n+1)$-diagram.
`assemble_proof` folds the active prefix with `Diagram::paste(n, …)` into the full
$(n+1)$-proof, composing $\#_n$ along the rewriting dimension; backward sessions
paste in reverse order. This is deferred until genuinely needed — storing,
typechecking, or rendering a proof banner. `ProofCache` makes the proof-view case
incremental: advancing the cursor extends the cached snapshot by pasting only the
new steps rather than re-folding from the start.

Two renderings of the proof, for two purposes:

- **`proof_expr`** — the durable, step-structured **source** form: one step per
  line, `d₁`, then `#ₙ d₂`, … (reversed for backward). This is what `store` writes
  into the `.ali` and what `resume` consumes. The step layout *is* the recipe.
- **`proof_label`** — the same proof flattened to a single line, for a status
  banner. Both denote the same diagram.

`register_proof(name)` commits the assembled proof as a named let-binding in the
type complex: it `Arc::make_mut`s the store, `modify_type_complex`s in a fresh
generator-free `add_diagram`, and returns fresh `Arc`s so the caller
(`Session::store_proof`) can resync `self.store`. The store key is the session's
`source_file`, which **is the loader's canonical path** — `Session` passes its
`root_path` into the constructor, never the raw CLI argument (the fix in
`463898c`; see [[interactive-session]]).

## Non-obvious invariants & gotchas

- **Steps are stored un-pasted; the current diagram is a boundary.** `steps[i]` is
  one rewrite, and `current_diagram` is *always* the `step_sign` boundary of the
  last applied step (or the initial diagram at step 0). The proof is only
  materialised by `assemble_proof`. Mixing these up is the easy mistake.
- **History is display-only — there is no replay.** `HistoryEntry` records a
  rule name and the chosen indices for the UI; nothing reconstructs a session by
  re-running them. A session is reconstructed by `resume` *from the proof diagram*,
  whose paste tree already encodes the step decomposition. This is why the old
  `SessionFile`/move-log was retired: a move log stored rule names but replayed on
  enumeration indices, so it was fragile and binary-version-bound, and carried
  nothing the proof diagram doesn't.
- **Rule patterns are built once.** `build_rule_patterns` runs at construction
  only; every `step`/`auto`/`refresh_rewrites` reuses the same `rule_patterns`
  map. Re-slicing rule boundaries per step was a hot spot — see [[core-matching]]
  §`RulePattern`.
- **`backward` flips three things in lockstep.** It chooses the pattern boundary
  inside `RulePattern`, the `step_sign` used to read the next diagram (`Output` vs
  `Input`), *and* the paste order in `assemble_proof`. They must agree; a mismatch
  silently builds the wrong proof.
- **`target_reached` is just `current ≅ target`.** It holds at step 0 too: an
  initial diagram already isomorphic to the target *is* a (zero-step, identity)
  proof — the unit of $\#_n$ on the initial diagram, valid because composition is
  unital (see [[0001-no-identities]]). An earlier version gated this on
  `active_len > 0` to reject zero-step proofs; that guard was wrong and has been
  removed.
- **`resume` needs `dim > 0`.** A proof diagram must be an $(n+1)$-cell with $n+1 >
  0$; a bare $0$-diagram has no $\#_n$ chain to decompose and is rejected.
- **`typecheck_proof` is a self-audit, not a user error path.** It checks the
  proof's initial-side boundary $\cong$ the initial diagram and that the proof
  round-trips through the sourcefier + interpreter. Both failures are phrased as
  engine/sourcefier bugs, because for a well-formed engine they cannot fire.
- **The engine has no `handle` method.** Command dispatch is no longer the
  engine's job — the old shared `handle` was retired when the
  [[interactive-session|`Session`]] became the single command surface. The engine
  exposes only the operations (`step`, `undo`, `auto`, `assemble_proof`, the
  accessors); `Session::apply` is what turns a `Request` into one of them.

## Mathematics

This module is the *driver* of [[rewriting]]: it does not itself realise new
mathematics, but it sequences the core operation into a session. A single `step`
is exactly one rewrite — locate a rule's pattern $U$ inside the current
[[diagram]] $V$ and substitute — performed by [[core-matching]]; the engine's job
is to chain such steps, reading each successive $V$ off the $\partial^\pm_n$
[[boundary]] of the previous $(n+1)$-step and finally composing them all with
$\#_n$ in `assemble_proof`. Backward rewriting swaps $\partial^-$ and $\partial^+$
throughout. `resume` runs this in reverse: it takes a finished proof
$(n+1)$-[[molecule]] apart into its steps by pseudo-normalising and flattening its
paste tree — see [[core-paste-tree]]. The proof a session builds is the
$(n+1)$-molecule whose input/output boundaries are the initial and final
$n$-diagrams; its type-correctness is what `typecheck_proof` audits. See
[[core-matching]] for the matching/pushout/reconstruct pipeline each step invokes,
[[interactive-session]] for the `Session` that sequences these methods, and
[[interactive-repl]] / [[interactive-daemon-web]] for the front ends that drive it.
