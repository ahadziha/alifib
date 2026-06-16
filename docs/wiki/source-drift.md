---
kind: note
status: stable
last-touched: 2026-06-13
---

# Source-side drift — deferred maintenance

Code-side rot surfaced while review-passing the wiki against `src/` (2026-05-29)
and verified item-by-item against current source (2026-05-30). These are **source
bugs, not wiki bugs** — every line/symbol below was confirmed present. The user
directed: record what to do here, do not touch the code yet. Pick these up
deliberately; tick them off in `log.md` when done.

Re-verified 2026-06-01 after the source/target → input/output rename (`03757c0`)
and the session-layer overhaul (`eefecda`/`59297b9`): all nine items are still
present. The `engine.rs` rewrite drifted some line numbers — trust the symbol
names below, not the line refs.

## Stale doc-comments (no behaviour change — safe when scheduled)

| # | Location | Wrong claim | Fix |
|---|----------|-------------|-----|
| 1a ✓ | `src/core/ogposet.rs` — module doc (L8–9), `normalisation` (L532), `boundary_traverse` (L576) | Says results are "memoised" / "memoised by pointer identity / by (pointer, sign, k)". There is **no cache** (no field, no `OnceCell`, no `lazy_static`); both fns call `traverse(…)` on every invocation. Only short-circuit is `is_normal()`. | **DONE 2026-06-04.** Deleted the four "memoised" mentions; `normalisation`'s `is_normal()` short-circuit now documented as idempotence. The `Arc`-keyed cache was *not* added — deferred as a separate perf decision. |
| 1b ✓ | `src/interactive/engine.rs` (doc on `rule_patterns`) | Doc-links `find_matches`, which now exists **only** as a `#[cfg(test)]` helper in `core/matching.rs`. Production path is `for_each_rule_candidate` + `confirm_candidate`. | **DONE 2026-06-04.** Re-pointed the `rule_patterns` doc (engine.rs:73) to `[`for_each_rule_candidate`]`; link resolves under `cargo doc`. |
| 1c ✓ | `src/interactive/web.rs:3` (also L48) | Says `WebRepl` is used by "both web backends" (server + wasm). There are **three**: the MCP backend also uses it (`web/mcp/src/lib.rs:2,54`). | **DONE 2026-06-04.** Reworded to count-free phrasing listing all three (server/wasm/mcp). The secondary "L48" mention was already gone (removed in the web.rs rewrite). |
| 1d | `src/interactive/mod.rs:47` (submodule table) | Lists `render_match_highlight` under `render` — **no such symbol**. Public render entry point is `render_step` (`render.rs:22`); also `print_state`, `print_history`. | Replace `render_match_highlight` → `render_step`. |
| 1e ✓ | `src/interpreter/partial_map.rs:616` (doc on `interpret_partial_map_ext`) | Says extension grammar is `{ prefix? clause* }` (curly). Real grammar uses `LBrack`/`RBrack` → `[ … ]` (`language/parser.rs:172–199`). | **DONE 2026-06-04.** Changed to `prefix? [ clause, … ]` — *not* the note's `[ prefix? clause* ]`, which was itself wrong: the prefix sits before the bracket (form `F [ … ]`) and clauses are comma-separated, not whitespace-repeated. |

## Dead code

- **`src/aux/intset.rs:72` — `pub fn intersection`** (`#[allow(dead_code)]`, L71).
  **Fully dead**: whole-repo `rg --no-ignore` finds the definition and *zero*
  call sites — not even in tests. → Safe outright delete. *(Not selected for
  action by the user; recorded for completeness.)*

- **`src/core/matching.rs` — `find_compatible_families`** (`pub(crate)`,
  `#[allow(dead_code)]`) **+ helpers** `max_independent_set_size`, `max_is_dfs`,
  `enumerate_independent_sets_of_size`. ~110 LOC implementing a
  maximal-independent-set exhaustive parallel-rewrite strategy. Only callers are
  in `#[cfg(test)] mod tests`; the engine's live parallel path is greedy
  (`greedy_parallel_auto_step` → `build_greedy_family` → `try_or_shrink`).
  **RESOLVED 2026-06-04 — KEEP, intentionally.** Per the user: this is the
  *deterministic exhaustive* enumeration of *all* maximal compatible families — a
  genuinely different problem from greedy auto-rewrite (which grabs one family for
  speed and may miss the maximal ones). Worst-case exponential in match count, so
  deliberately kept out of the engine hot path, but retained as a backend
  capability. Action taken: replaced the bare `#[allow(dead_code)]` with a
  documented retention rationale on the function and "supports
  `find_compatible_families`" markers on the three helpers, so it no longer reads
  as accidental rot. Tests (`idem_parallel_in_four_chain`,
  `idem_no_parallel_in_three_chain`) kept; greedy has its own disjointness
  coverage (`greedy_parallel_in_four_chain`). The L7 module doc-link is correct as
  is. Not deleted, not engine-wired.

## WET code

- **`src/analysis/homology.rs` — Smith Normal Form, tracked vs untracked.**
  Two parallel families:
  - Untracked: `smith_normal_form` (L334) + `find_and_move_pivot` (L708),
    `eliminate_column` (L733), `eliminate_row` (L763), `enforce_divisibility` (L799).
    Caller: `matrix_rank` (L409).
  - Tracked: `smith_normal_form_with_basis` (L429) + `find_and_move_pivot_tracked`
    (L537), `eliminate_column_tracked` (L690), `eliminate_row_tracked` (L564),
    `enforce_divisibility_tracked` (L607), `sort_diag_tracked` (L658), and the
    elementary-op carriers `row_swap/add/negate/gcd_tracked` (L487–533).
    Caller: `compute_homology` (L244).

  The driver loops are near line-for-line identical (L346–389 vs L442–471) and the
  elimination helpers duplicate the same integer 2×2 row arithmetic. ~150 LOC of
  structural overlap. **Correctness risk:** two copies of subtle integer-SNF logic
  must stay in lockstep.

  **RESOLVED 2026-06-04 — refactored.** Introduced a `Tracker` trait with seven
  elementary mirror ops (`row_swap`/`row_add`/`row_negate`/`row_gcd` +
  `col_swap`/`col_add`/`col_gcd`). `NoTrack` is a zero-cost no-op; `FullTrack`
  mirrors each row op *inverted* onto `u_inv` and each column op directly onto
  `v`, preserving `U·M·V = diag`. A single `snf_reduce<T: Tracker>` drives the
  shared pivot/eliminate loop once, with generic `find_and_move_pivot`/
  `eliminate_column`/`eliminate_row`. `smith_normal_form` uses `NoTrack`,
  `smith_normal_form_with_basis` uses `FullTrack`. The two *tails* were kept
  separate as planned — the plain path normalises the diagonal directly
  (`enforce_divisibility` + sort); the tracked path returns the raw positional
  diagonal and `compute_homology` runs `enforce_divisibility_tracked` /
  `sort_diag_tracked` so the basis stays valid. Deleted all the `*_tracked`
  elementary helpers and the untracked duplicates: net −88 LOC. All 34 homology
  tests (and the full 143-test lib suite) green before and after.

  *Follow-on (same session):* surfacing the tracked path's output. The torsion
  witnesses `compute_homology` produces had **no user-facing consumer** —
  `build_homology_data` dropped them, so the `homology` command (CLI + web + MCP,
  all via the shared `richtext::homology` renderer) showed only groups + χ. Now
  `HomologyGroupInfo` carries a `Vec<TorsionWitnessInfo>` (order + formatted cycle
  + preimage, via new `TorsionWitness::cycle_str`/`preimage_str`), populated from
  `h.torsion_witnesses` and rendered as an indented sub-line under each `H_d`.
  Verified end-to-end (`homology RP2` → `Z/2 cycle: c.t  (preimage: L.t + U.t)`;
  free spaces show none). Full 201-test workspace suite green.

## Notes

Items 1b and the matching.rs strategy compile away in release (`#[cfg(test)]` /
`#[allow(dead_code)]`), so the stale doc-links are invisible at runtime but mislead
readers of the source.

## [2026-06-01] Second pass — drift found verifying the full wiki against `src/`

A page-by-page re-verification of every wiki page (plus new coverage of `cli/`
and `web/`) surfaced further source-side rot. Same rule as above: **recorded, not
fixed.**

### A. ~~`PartialMap::extend` does not enforce no-dimension-lowering~~ — not a bug (2026-06-03)

Dimension-*lowering* maps are legitimate: a 1-cell whose endpoints collapse maps
to the 0-cell itself, and collapse inference produces such images on purpose.
`extend`'s only dimension guard, no-*raising*, is the correct one — there is
nothing to add. The genuine structural constraint on cells is roundness of
boundaries, [[0002-round-boundaries]]. (The premise behind this item — that
no-identities forbids lowering — does not hold; see [[0001-no-identities]].)

### B. Stale doc-comments — the `03757c0` source/target→input/output rename left comments behind

The rename swept symbols and the whole wiki but not all doc-comments. Behaviour is
correct (code uses `Sign::Input`/`Sign::Output`); only the comments mislead.

| Location | Wrong text | Fix |
|---|---|---|
| ✓ `core/diagram.rs` — `Diagram::is_round` doc | "True if … input and output boundaries are **equal**." Wrong twice: it delegates to `ogposet::is_round`, the directed-sphere **disjointness** test, not equality. | **DONE 2026-06-04.** Reworded to mirror `Ogposet::is_round` (disjoint input/output interiors at every dimension); dropped both "equal" and the misleading "prerequisite for pasting" gloss. |
| ✓ `output/normalize.rs` — `sign_superscript`, `cell_from_diagram`, `render_solved_hole` | "⁻ for Source / ⁺ for Target", "source and target boundaries", "principal boundary `src -> tgt`" | **DONE 2026-06-04.** Only `cell_from_diagram` survived — fixed its "source and target boundaries" doc → input/output and renamed locals `src_diag`/`tgt_diag` → `in_diag`/`out_diag`. `sign_superscript` and `render_solved_hole` no longer exist (deleted), so the other two are obsolete. |
| ~~`interpreter/diagram.rs` — `push_parallel_constraints` doc~~ — **OBSOLETE 2026-06-04.** `push_parallel_constraints` was deleted with the inference layer; the whole `src/interpreter/inference.rs` is gone. No `globular_propagate`/`globular_sub_boundaries`, no `(Source/Target, n-1)` slots, no "source/target boundaries" remain anywhere in the file. `interpret_assert`/`interpret_paste` survive but carry none of the flagged comments. Nothing to fix. | | |
| ✓ `interactive/engine.rs` — `step_sign` doc | "Forward: `Target` (output). Backward: `Source` (input)." | **DONE 2026-06-04.** → "Forward: `Output`. Backward: `Input`." (matches the body `if backward { Input } else { Output }`). |
| ✓ `interactive/cli.rs` — `ServeArgs` doc | "waits for an `Init` request" — there is no `Init` variant; it is `Start`. | **DONE 2026-06-04.** "Init" → "Start". |
| ~~`interactive/display.rs` — palette comments (`C_SRC`, `C_TGT`)~~ | "matched source pattern", "rewrite target" | **OBSOLETE 2026-06-04.** Already corrected — the comments now read "input side" / "output side"; nothing to fix. |
| ~~`interpreter/types.rs` — `InterpResult` doc~~ | "merged with `combine`" | **OBSOLETE 2026-06-04.** Already correct — the doc says "merged with `merge`" and the method is `InterpResult::merge`; no `combine` remains. |

### C. Dead code

- **`core/ogposet.rs::closure`** carries `#[allow(dead_code)]` but has **live**
  production callers (`matching::check_match_isomorphism`, `reconstruct`). The
  attribute is stale — remove it (the opposite of the others: it masks a *live*
  symbol). **DONE 2026-06-04.** Removed the attribute; build has no `dead_code`
  warning, confirming the symbol is live.
- **`core/diagram.rs::whisker_rewrite`** (`pub`): fully dead — whole-repo
  `rg --no-ignore` finds zero callers, not even tests. Production rewrite steps go
  through `matching::construct_parallel_step` → `pushout::multi_pushout`. It is
  *not* behind `#[allow(dead_code)]`; only its `pub` visibility dodges the warning.
  **DONE 2026-06-04 — deleted.** Unlike `find_compatible_families` it solves no
  distinct problem: `construct_parallel_step` already builds the same step as a
  1-member family. Removed the ~60-LOC fn and its now-orphaned private helper
  `fold_trees` (the build confirmed the orphan via `dead_code`). Net −122 LOC;
  build clean, diagram tests green.
- ~~**`language` — `Token::LAngle` / `Token::RAngle` (`<` / `>`)**: lexed but
  consumed by no grammar production anywhere in the repo. Dead tokens.~~
  **NOT A BUG — RETRACTED 2026-06-04.** False positive (caught by the user). The
  angle brackets are the surface syntax for **for-block variable instances**
  (`<ctx>`, `<k>`, …): `parser::for_body` scans the block body as raw tokens with
  a wildcard `any()` (so it *does* consume `LAngle`/`RAngle`), captures the body
  source span, and `eval::expand_body` substitutes `<var>` textually
  (`var_pattern = format!("<{}>", …)`). Deleting the tokens would make `<` a lex
  error and break every `for`-block — verified: `examples/LambdaSigma_Term.ali`
  (91 generators, full of `<ctx>`/`<char>`) loads fine. Added a clarifying comment
  at the lexer rules so this isn't re-flagged. The "no named production" basis was
  literally true but operationally wrong.
- **`language::parse_complex` + `parser::complex_parser`** (`pub`): fully dead (no
  caller repo-wide). Their doc-comments still claim "Used by the interactive REPL
  to parse `@ <expr>` commands" — the `@`-prompt grammar was removed in the
  session-layer overhaul. Dead public API + stale doc-comment.
  **DONE 2026-06-04 — deleted both.** Confirmed distinct from the live for-block
  re-parse path, which uses the *separate* `parse_complex_instrs` →
  `complex_instrs_parser` (eval.rs:583), not this pair. `complex_parser` only
  composed shared builders (`build_diagram`/`build_complex`/…) that other live
  parsers still use, so nothing downstream was orphaned. No external consumers.
  Build + 57 language tests green.

### D. Notes (outside `src/`)

- `docs/interp/interp.tex` repeats the same phantom-"cached by pointer identity"
  claim as `ogposet.rs` (item 1a) and mislabels input-extremality as "no input
  cofaces" (source uses no *output* coface). A paper, not code — but it seeds the
  same false claims. **DONE 2026-06-04.** Fixed both (with the user, it being their
  paper): deleted the "cached by pointer identity" sentence; corrected "no input
  cofaces" → "no output cofaces" (matches `ogposet::extremal`: `Sign::Input` ⇒
  `cofaces_out.is_empty()`). LaTeX structure preserved.
- ~~`web/EXAMPLES.md` describes a build-time-generated `dist/` manifest / deploy
  workflow that `web/frontend/package.json` … does not implement.~~ **NOT A BUG —
  RETRACTED 2026-06-04.** False positive: the workflow *is* implemented, just not
  in `package.json` (which only bundles via esbuild). The manifest is generated by
  `scripts/build_examples_manifest.py`, invoked at deploy time by
  `.github/workflows/deploy.yml:57` (`python3 scripts/build_examples_manifest.py
  examples dist/examples`). `EXAMPLES.md` is accurate.
- `docs/HOMOLOGY.md` cites `src/core/homology.rs`; the code is at
  `src/analysis/homology.rs`. Stale path. (Moved here from the `analysis` wiki
  page, which should describe current code, not flag external-doc drift.)
  **DONE 2026-06-04.** Updated both refs (text + relative links, L39 & L138) to
  `src/analysis/homology.rs`.

## [2026-06-03] Refactors landed — some items now stale

The **maps-with-holes** rewrite and the **shared interactive `Session`** (see
`log.md`) touched several files this backlog points at. Re-verify before picking
any item up — line refs in particular have moved. Known status changes:

- **Item 1d — RESOLVED.** `src/interactive/mod.rs` no longer lists
  `render_match_highlight`; the new submodule table names `render_step`. Nothing
  to fix.
- **Item B, `render_solved_hole` row — OBSOLETE.** That function was *deleted*
  with `inference.rs` and the solved-hole reporting path; there is no longer a
  doc-comment to correct. (The `sign_superscript` / `cell_from_diagram` rows of
  the same item still need checking against current `output/normalize.rs`.)
- **Items 1b, 1c, 1e** point at `engine.rs`, `web.rs`, `partial_map.rs` — all
  heavily rewritten. The *symbols* they name may be gone or renamed (`engine.rs`
  no longer has a `handle`; `partial_map.rs` no longer has `enrich_holes`). Treat
  these as needing a fresh pass, not as verified.
- **Items C/D** (dead `whisker_rewrite`, `parse_complex`, `Token::LAngle/RAngle`;
  WET SNF in `homology.rs`) were re-checked and **still stand** — those symbols
  remain present. (The old "missing dimension-lowering guard" is **not** among
  them — see item A above, retracted: lowering is legitimate, and collapse
  inference relies on it.)

### New dead code from the refactor

- **`src/interactive/engine.rs::init`** (`pub`): now **dead** — the only mention
  repo-wide outside its own definition is the doc-link at `engine.rs:44`; there
  are zero call sites in `src/`, `cli/`, `web/`, or tests. It was the daemon's
  load-file-and-build-in-one-step constructor; the daemon now goes through
  `Session::from_disk` instead, leaving `init` orphaned. Like `whisker_rewrite`
  (item C) it is *not* behind `#[allow(dead_code)]` — only its `pub` visibility
  dodges the warning. **DONE 2026-06-04 — deleted.** Removed `init`, its
  orphaned-only-caller helper `load_context`, and the then-orphaned type alias
  `LoadedRewriteContext` (build confirmed each orphan in turn). Re-pointed the
  `engine.rs:44` doc-link to the live `from_store`/`resume`. Build clean, full
  201-test workspace green.

## [2026-06-05] note | `docs/interp/interp.tex` no longer on disk

The full update pass found the `[[oriented-graded-poset]]` citation "interp.tex
§Oriented Graded Posets" stale: `docs/interp/` now holds only `interp.pdf` plus
LaTeX build artifacts (`.aux`/`.toc`/`.fls`/`.fdb_latexmk`/`.log`), no `interp.tex`,
and the whole directory is untracked (`git status` shows `??`). The wiki citation
was re-pointed to `docs/interp/interp.pdf §3.1`. Note §D above and the 2026-06-04
log entry record *editing* `interp.tex` (the author's paper); those records now
point at an absent source file — the edits presumably live in the author's working
copy of the `.tex`. No source/code action; the re-pointed citation is the only
change. The two long-standing `src/`-side items remain as recorded:
`aux::intset::intersection` is still fully dead (zero callers, kept per user), and
`ogposet::closure` is confirmed live (its stale `#[allow(dead_code)]` was removed
2026-06-04). No new source rot surfaced.

## [2026-06-09] Third pass — full audit/rewrite of every wiki page

Every page was audited and rewritten against current source by a parallel agent
batch. Same rule as always: **recorded, not fixed.** Each item below was verified
by the auditing agent against current code (several independently by two or three
agents).

### Stale doc-comments

| # | Location | Wrong claim | Fix |
|---|----------|-------------|-----|
| 3a | `src/interpreter/types.rs` — `EvalMap::holes` doc | Holes are "Empty for any map built outside a definition block (name lookups, …)" — false for name lookups: `eval_partial_map_basic`'s `Name` arm deliberately carries `Complex::map_holes(name)` so `F [ … ]` fills them (pinned by `prefix_extension_fills_holes`). Found independently by three agents. | Drop "name lookups" from the empty list; state the carry-through. |
| 3b | `src/interactive/fill.rs` — module header | Says `done` "appends `x => <proof>`". `edit_map_definition` *replaces an explicit `?` in place*; appending happens only for implicit holes (`pins_a_dotted_explicit_hole_in_place` / `appends_when_no_matching_explicit_hole`). | Describe the pin-in-place behaviour. |
| 3c | `src/interpreter/partial_map.rs` — `interpret_partial_map_ext` doc | Claims pending `=> ?` clauses become holes "in a final pass … in ascending source dimension" — no such pass exists; holes are created inline per clause by `assign_cell`. | Rewrite to inline creation. |
| 3d | `src/interpreter/partial_map.rs` — `hole_map_image` doc | Doc-links `ensure_hole`, which does not exist; the call is `ensure_defined`. | Fix the link. |
| 3e | `src/interpreter/diagram.rs` — `interpret_diagram_as_term`, `interpret_paste` docs | Call the explicit paste syntax `` `*k` `` — the surface token is `#k` (lexer `Token::Hash`; GRAMMAR.md). | `*k` → `#k`. |
| 3f | `src/aux/bitset.rs` — module rustdoc | `BitSet` "used exclusively in `ogposet::traverse`" — `core/reconstruct.rs` also uses it (`downsets`, `embedding_to_bitsets`). | Drop "exclusively". |
| 3g | `src/aux/loader.rs` — `with_virtual_files` rustdoc | Speaks of "`#use` directives" — the language form is `include <Name>`. | Rename. |
| 3h | `src/interactive/engine.rs` — `resolve_type` rustdoc | "Called by the REPL when the user types `@ <TypeName>`" — the `@` grammar is long gone (now `start <type> <source>`). | Re-point. |
| 3i | `src/interactive/cli.rs` — `ReplArgs.type_name` doc | "set via `@ <TypeName>` interactively if absent" — same obsolete `@` grammar. | Re-point. |
| 3j | `src/interactive/session.rs` — `Session::from_virtual` rustdoc | "Returns the session and the loaded store on success" — returns only `Result<Self, String>`. (Also dead, see below.) | Fix or delete with the fn. |
| 3k | `src/language/ast_print.rs` — module doc | Claims unconditional print→parse round-trip equivalence; false for `for`-blocks: `Printer::for_block` emits literal `{ ... }` (re-parses, but not equivalently). | Qualify the guarantee. |
| 3l | `web/wasm/src/lib.rs` — `run_command` and `get_strdiag` docs | "Supported commands" list omits `step_multi`/`redo`/`holes`/`fill`/`done` and claims `save` unsupported (the MCP descriptor lists it); `get_strdiag` doc has a duplicated paragraph. | Refresh both. |
| 3m | `.github/workflows/deploy.yml` — Assemble-dist comment | Says the manifest script "fails the build on duplicate stems" — no duplicate check exists since qualified path-names (web/EXAMPLES.md rule 3). | Drop the claim. |

### Dead code

- **`src/aux/mod.rs::dim_subscript`** — fully dead since `af611fc` (its last
  callers left with the inference layer); `pub` visibility dodges the warning.
- **`src/interactive/engine.rs` — `load_type_context`, `typecheck_proof`,
  `proof_label`** — `pub`, zero callers workspace-wide; rustdoc still claims CLI/
  daemon callers.
- **`src/interactive/session.rs::from_virtual`** — zero callers; the web
  deliberately bypasses it (`load_source_with_modules` → `Session::from_loaded`)
  to return structured `Diagnostic`s.
- **`src/analysis/strdiag.rs::StrDiag::from_named`** — zero callers
  workspace-wide; public builder surface only.
- **`src/core/diagram.rs::cell_with_input_embedding`** — not dead but vestigial
  in shape: its only caller `cell_n` discards the returned embedding (the
  matching path uses `RulePattern::pattern_to_rewrite` instead). Simplification
  candidate, not deletion.

### Parser / grammar divergence (`docs/GRAMMAR.md` vs `src/language`)

- `<ForBlock>` omits the optional `bar` exclusion clause the parser accepts.
- `<DComponent>` omits string literals (parser accepts `Str`, expanding to pastes).
- Lexical classes: `<Name>` is ASCII-only but the lexer accepts Unicode letters;
  `<Nat>` forbids leading zeros but the lexer lexes `007` (`test_nat_multi_digit`).
- `build_let_or_def` silently **drops `total`** on `let total x = <diagram>`
  (GRAMMAR grants `total` only to the `:: address =` map form). Parser leniency
  that loses user intent — arguably a bug, not just doc drift.

### Test gap

- [[0002-round-boundaries]]'s canonical loop example (`a : pt -> pt`) has no
  named pinning test — it is exercised only incidentally inside
  `virtual_loader_subdirectory_resolution`. A dedicated test would let 0002 cite
  behavioural evidence per the wiki's own convention.

## [2026-06-13] note | maps-with-holes composition landed (merge of main into wiki)

Commits `9cd2b92`..`a6f385f` merged in: `HoleId` dropped (holes keyed by source
generator, `Tag::Hole(Box<Tag>)`), core `PartialMap::compose` replaced by the
interpreter's hole-aware `compose_with_holes`, inline-body-map filling, and the
`done` edit behaviour. The affected wiki pages were updated in the same pass (see
`log.md` 2026-06-13). Two source-side doc-comment items shifted, and were
**fixed** in this pass:

- **Item 3a ✓ — DONE 2026-06-13.** `EvalMap::holes` doc
  (`src/interpreter/types.rs`) had listed "name lookups" *and* "compositions"
  among the cases where holes are empty — wrong for both (a name lookup
  re-attaches `Complex::map_holes`; `compose_with_holes` propagates them).
  Rewritten to state the carry-through and limit "empty" to identities and
  inclusions (total by construction).
- **Item 3b ✓ — DONE 2026-06-13.** `a6f385f` had updated `docs/INTERACTIVE.md`;
  the `fill.rs` module header still described only the append-after-`?` case.
  Rewritten to describe both — explicit `x => ?` replaced in place, implicit hole
  appended — and that `find_map_def` locates the definition in a `@Type` block, a
  type-body `let`, or at module level.
