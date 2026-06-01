---
kind: note
status: stable
last-touched: 2026-06-01
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
| 1a | `src/core/ogposet.rs` — module doc (L8–9), `normalisation` (L532), `boundary_traverse` (L576) | Says results are "memoised" / "memoised by pointer identity / by (pointer, sign, k)". There is **no cache** (no field, no `OnceCell`, no `lazy_static`); both fns call `traverse(…)` on every invocation. Only short-circuit is `is_normal()`. | Delete the four "memoised" mentions. (Or actually add an `Arc`-keyed cache — but that's a perf design decision, not a doc fix.) |
| 1b | `src/interactive/engine.rs` (doc on `rule_patterns`) | Doc-links `find_matches`, which now exists **only** as a `#[cfg(test)]` helper in `core/matching.rs`. Production path is `for_each_rule_candidate` + `confirm_candidate`. | Re-point the comment at `for_each_rule_candidate`/`confirm_candidate`. |
| 1c | `src/interactive/web.rs:3` (also L48) | Says `WebRepl` is used by "both web backends" (server + wasm). There are **three**: the MCP backend also uses it (`web/mcp/src/lib.rs:2,54`). | "the three web backends" + add `web/mcp/`. |
| 1d | `src/interactive/mod.rs:47` (submodule table) | Lists `render_match_highlight` under `render` — **no such symbol**. Public render entry point is `render_step` (`render.rs:22`); also `print_state`, `print_history`. | Replace `render_match_highlight` → `render_step`. |
| 1e | `src/interpreter/partial_map.rs:392` (doc on `interpret_partial_map_ext`) | Says extension grammar is `{ prefix? clause* }` (curly). Real grammar uses `LBrack`/`RBrack` → `[ … ]` (`language/parser.rs:167–171`). Curly braces are used elsewhere (parser.rs:244), so this actively misleads. | Change to `[ prefix? clause* ]`. |

## Dead code

- **`src/aux/intset.rs:72` — `pub fn intersection`** (`#[allow(dead_code)]`, L71).
  **Fully dead**: whole-repo `rg --no-ignore` finds the definition and *zero*
  call sites — not even in tests. → Safe outright delete. *(Not selected for
  action by the user; recorded for completeness.)*

- **`src/core/matching.rs:590` — `find_compatible_families`** (`pub(crate)`,
  `#[allow(dead_code)]` L589) **+ helpers** `max_independent_set_size` (L818),
  `max_is_dfs` (L826), `enumerate_independent_sets_of_size` (L851). ~110 LOC
  implementing a maximal-independent-set exhaustive parallel-rewrite strategy.
  Only callers are in `#[cfg(test)] mod tests` (L1045, L1096); no production path
  reaches it (engine uses `for_each_rule_candidate`/`confirm_candidate`).
  **Decision needed:** either wire it into the engine's parallel-rewrite mode, or
  delete it together with its tests and the dangling module-header doc-link (L7).
  Don't leave it half-alive.

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
  **DRY fix (deliberate, design change):** parameterise over a `Tracker` —
  a trait whose elementary ops (`row_swap`, `row_add`, `row_negate`, `row_gcd`, and
  the column ops) are no-ops in the untracked case and mirror onto the `u_inv`/`v`
  basis matrices in the tracked case. A single generic `smith_normal_form<T: Tracker>`
  drives the pivot/eliminate/enforce loop once; `matrix_rank` uses a zero-cost
  `NoTrack`, `compute_homology` uses `FullTrack`. Keep the existing SNF tests green
  before and after.

## Notes

Items 1b and the matching.rs strategy compile away in release (`#[cfg(test)]` /
`#[allow(dead_code)]`), so the stale doc-links are invisible at runtime but mislead
readers of the source.

## [2026-06-01] Second pass — drift found verifying the full wiki against `src/`

A page-by-page re-verification of every wiki page (plus new coverage of `cli/`
and `web/`) surfaced further source-side rot. Same rule as above: **recorded, not
fixed.** Item A is a **behavioural / correctness gap**, not a doc bug —
prioritise it over the rest.

### A. `PartialMap::extend` does not enforce no-dimension-*lowering* ⚠️ correctness

`src/core/partial_map.rs::extend` rejects dimension-*raising* (`if image.dim() >
dim`) but has **no** guard against *lowering*. A source 1-cell whose two
endpoints both map to the same 0-cell `p`, itself sent to `p`, is accepted: `0 >
1` is false, and `check_boundary_match` at `k = 0` compares `p` against `p` and
passes (`Diagram::boundary_normal(·, 0, p)` clamps to the image's top dimension
and returns `p`). So a degenerate 1-cell→0-cell map — an *identity in disguise* —
is constructible: `let total F :: Edge` with `s => K.o, t => K.o, arr => K.o`
loads cleanly (empirically verified; the resulting entry has source dim 1, image
dim 0). This contradicts decision [[0001-no-identities]]; the no-identities
discipline is only *accidentally* enforced — it fails solely when the two endpoint
images differ (the input/output boundaries then genuinely disagree). **Fix
(design decision):** add a lower-bound guard in `extend`. The open question is
whether a `k`-cell may map to a longer *composite* of `k`-cells (should be
allowed) while a strictly lower-dimensional image is forbidden — i.e. reject
`image.dim() < dim`, not merely `>`. Wiki pages [[0001-no-identities]] and
[[core-partial-map]] now state the gap honestly.

### B. Stale doc-comments — the `03757c0` source/target→input/output rename left comments behind

The rename swept symbols and the whole wiki but not all doc-comments. Behaviour is
correct (code uses `Sign::Input`/`Sign::Output`); only the comments mislead.

| Location | Wrong text | Fix |
|---|---|---|
| `core/diagram.rs` — `Diagram::is_round` doc | "True if … input and output boundaries are **equal**." Wrong twice: it delegates to `ogposet::is_round`, the directed-sphere **disjointness** test, not equality. | Mirror `Ogposet::is_round`'s doc (disjoint interiors). This comment seeded the now-fixed wiki errors in `diagram`/`core-diagram`/`boundary`. |
| `output/normalize.rs` — `sign_superscript`, `cell_from_diagram`, `render_solved_hole` | "⁻ for Source / ⁺ for Target", "source and target boundaries", "principal boundary `src -> tgt`" | input/output. |
| `interpreter/diagram.rs` — `push_parallel_constraints` doc (+ `interpret_assert`, `interpret_paste` comments) | "source/target boundaries"; principal slots named `(Source, n-1)`/`(Target, n-1)`; "derived afterwards by `globular_propagate`" | input/output; the solver fn is `inference::globular_sub_boundaries` — no `globular_propagate` exists. |
| `interactive/engine.rs` — `step_sign` doc | "Forward: `Target` (output). Backward: `Source` (input)." | `Output`/`Input`. |
| `interactive/cli.rs` — `ServeArgs` doc | "waits for an `Init` request" — there is no `Init` variant; it is `Start`. | `Start`. |
| `interactive/display.rs` — palette comments (`C_SRC`, `C_TGT`) | "matched source pattern", "rewrite target" | input/output (cosmetic). |
| `interpreter/types.rs` — `InterpResult` doc (×2) | "merged with `combine`" — there is no `combine`; the method is `InterpResult::merge`. | `merge`. |

### C. Dead code

- **`core/ogposet.rs::closure`** carries `#[allow(dead_code)]` but has **live**
  production callers (`matching::check_match_isomorphism`, `reconstruct`). The
  attribute is stale — remove it (the opposite of the others: it masks a *live*
  symbol).
- **`core/diagram.rs::whisker_rewrite`** (`pub`): fully dead — whole-repo
  `rg --no-ignore` finds zero callers, not even tests. Production rewrite steps go
  through `matching::construct_parallel_step` → `pushout::multi_pushout`. It is
  *not* behind `#[allow(dead_code)]`; only its `pub` visibility dodges the warning.
  Delete, or wire into the rewrite path.
- **`language` — `Token::LAngle` / `Token::RAngle` (`<` / `>`)**: lexed but
  consumed by no grammar production anywhere in the repo. Dead tokens.
- **`language::parse_complex` + `parser::complex_parser`** (`pub`): fully dead (no
  caller repo-wide). Their doc-comments still claim "Used by the interactive REPL
  to parse `@ <expr>` commands" — the `@`-prompt grammar was removed in the
  session-layer overhaul. Dead public API + stale doc-comment.

### D. Notes (outside `src/`)

- `docs/interp/interp.tex` repeats the same phantom-"cached by pointer identity"
  claim as `ogposet.rs` (item 1a) and mislabels input-extremality as "no input
  cofaces" (source uses no *output* coface). A paper, not code — but it seeds the
  same false claims.
- `web/EXAMPLES.md` describes a build-time-generated `dist/` manifest / deploy
  workflow that `web/frontend/package.json` (only `build`/`watch` scripts) does
  not implement — possible doc/build drift.
