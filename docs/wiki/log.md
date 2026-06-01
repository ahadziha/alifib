# Log

Append-only timeline. Each entry: `## [YYYY-MM-DD] <kind> | <description>`.
`grep "^## \[" log.md | tail` for recent activity.

## [2026-05-29] doc | Scaffolded the wiki

Created the initial structure: `concepts/`, `implementation/`, `decisions/`,
`open-questions/`, plus `index.md`, `log.md`, and the `CLAUDE.md` schema.
Seeded `core-complex`, `interpreter`, `language-parser`, `interactive-repl`, and
`decisions/0001-no-identities` from prior auto-memory notes (marked `draft` —
code refs predate recent commits, re-verify before trusting). Concept pages and
remaining implementation pages are stubs. `module-open-semantics` open question
captured from `docs/new-notes.md`.

## [2026-05-29] doc | Pilot batch: core-diagram, diagram, core-matching, language-parser

Documented three representative modules via parallel Opus agents, plus the
paired `diagram` concept page, as a workflow pilot (not the full module sweep).
All re-verified against current `src/` (no line-number refs; ~20 cited symbols
spot-checked and confirmed). `core-matching` covers the
`matching/embeddings/pushout/flow/reconstruct` rewrite pipeline and is now the
de-facto impl-page template. `language-parser` rewritten from a badly stale
draft that had documented ~20% of the module and omitted the load-bearing
`for`/`index` deferred-text-expansion system. Statuses bumped stub→draft in
`index.md` for `core-diagram`, `core-matching`, `diagram`.

Pilot critique flagged process holes to close before scaling to the remaining
~9 modules: writers skipped the index/log bookkeeping step (consolidated here by
the orchestrator); the concept↔code bridge resolves only syntactically because
its concept targets are still stubs; and several central concepts lack their own
slugs (flow-graph/maximal-flow-graph, pushout/colimit, reconstruction/layering).

## [2026-05-29] doc | Interleaved batch: 11 impl modules + 11 concept pages

Documented the remaining implementation modules and fleshed the concept pages
they bridge to, in one parallel Opus run, so the concept↔code bridge lands
substantive rather than pointing at stubs. New impl pages: `core-complex`
(rewritten — the old draft wrongly claimed `add_generator` calls `add_diagram`),
`core-ogposet`, `core-partial-map`, `interpreter` (rewritten — `GlobalStore.modules`
is an insertion-ordered `IndexMap`, not a `HashMap`), `output`, `interactive-engine`,
`interactive-repl` (rewritten — stale command grammar), `interactive-daemon-web`,
`analysis`, `aux`, `codegen`. Fleshed concepts: `molecule`, `atom`,
`regular-directed-complex`, `oriented-graded-poset`, `boundary`, `partial-map`,
`rewriting`, `module-system`, plus new slugs `flow-graph`, `pushout`,
`reconstruction`.

## [2026-05-29] doc | Gap-fill: analysis page + homology/string-diagram/hole concepts

`implementation/analysis.md` was rewritten (the batch agent's write was blocked
by a guard; orchestrator persisted the returned content). Added concept pages
`homology`, `string-diagram`, and `hole` to close the analysis bridge and the
remaining `[[hole]]` dangling link.

## [2026-05-29] refactor | CLAUDE.md schema amendments + lint fixes

Amended `CLAUDE.md` after the pilot: status source-of-truth (page frontmatter,
index mirrors it), blessed private-symbol and named-test citations, code-block-rot
policy, impl + concept page templates (codifying `core-matching`), and a
non-optional closing checklist. Lint fixes: removed stray tool tags from
`regular-directed-complex.md`; downgraded `core-complex` stable→draft for batch
uniformity; verified the flow-graph `Definition 61 of Hadzihasanovic–Kessler`
citation against `src/core/flow.rs` (faithful to source, left as-is). All `index.md`
statuses reconciled to frontmatter; 8 new rows added.

## [2026-05-29] lint | Post-batch health check

All 14 impl pages carry `## Mathematics`, all 15 concept pages carry
`## Implementation` — bridge rule satisfied. ~25 spot-checked code refs resolve
against current `src/`. No orphans. Remaining coverage note: `core/ogposet.rs`'s
`restrict_ogposet` actually lives in `reconstruct.rs`; legacy `docs/HOMOLOGY.md`
cites the rotted path `src/core/homology.rs`.

## [2026-05-29] refactor | Full review pass: every page re-verified against current source

Ran an adversarial reviewer (one Opus agent per page) over all 31 content pages,
fixing staleness in place. 9 pages corrected: `core-complex` (counter field
`insertion_order`→`next_order`; `LocalCells.by_id`→`LocalCellEntry`; pairing site
`register_builtins`→`insert_global_cell`), `module-system` (full lexer keyword set;
`identity_map` location; type-lookup chain via `find_diagram`/`top_label`; Mode::Local
attach branch), `core-ogposet` (real `reconstruct_*` test names; `restrict_ogposet`
takes `&[BitSet]`), `interpreter` (`assert_invariants` scope), `language-parser`
(`ast_print` consumers = codegen + CLI, not loader), `output` (the four `pub use`
render helpers), `interactive-repl` (colour doc + cfg-gating), `homology`
(`Ogposet::faces_of` casing), `0001-no-identities` (verified `PartialMap::extend`
enforcement site, added `target_reached` consequence). Remaining pages verified
clean. The auto-memory notes that seeded the original rot were deleted (see below).

Source-side rot found during review (NOT wiki bugs — flagged for later code fixes):
stale doc-comments in `ogposet.rs` (claims memoisation that isn't there),
`engine.rs` (refs removed `find_matches`), `web.rs` ("both backends" — there are
three), `interactive/mod.rs` (lists renamed `render_match_highlight`),
`partial_map.rs` (brace vs bracket grammar comment); dead code behind
`#[allow(dead_code)]` in `matching.rs` (exhaustive parallel strategy, test-only) and
`intset.rs` (`intersection`); WET SNF pair in `homology.rs`.

## [2026-05-29] refactor | Purged stale auto-memories

Deleted the four point-in-time architecture memories that seeded the rotted drafts
(`ref_interpreter_arch`, `ref_complex_cells`, `ref_parser_reuse`, `ref_repl_arch`);
left a breadcrumb in MEMORY.md pointing to this code-verified wiki as the source of
truth. Kept `ref_no_identities` (stable fact) and the negative-results feedback note.

## [2026-05-30] doc | Promoted 30 pages draft→stable

After the full adversarial review pass (one Opus agent per page) re-verified every
page against current `src/` — the second verification on top of the documentation
pass — promoted all 15 concept, 14 implementation, and 1 decision page from `draft`
to `stable`. `last-touched` bumped to 2026-05-30; `index.md` status column reconciled
(30 stable, 1 draft). Left `open-questions/module-open-semantics` at `draft`: its
subject (`open` scoping) is explicitly unresolved/aspirational, so `stable` (= settled
and verified) would misrepresent it. The source-side rot flagged during the review
(stale doc-comments, `#[allow(dead_code)]` blocks, the WET SNF pair in `homology.rs`)
is being investigated separately — those are code bugs, not wiki bugs, and untouched here.

## [2026-05-30] note | Recorded verified source-side drift in source-drift.md

Verified every flagged drift item against current `src/` (one background agent,
file:symbol evidence). All confirmed present. Per user direction, made **no source
changes** — recorded the actionable fixes in new page `source-drift.md` (linked from
`index.md`): 5 stale doc-comments (`ogposet.rs` phantom memoisation, `engine.rs`
removed `find_matches`, `web.rs` "both"→three backends, `mod.rs` `render_match_highlight`
→`render_step`, `partial_map.rs` `{ }`→`[ ]`); dead code (`intset::intersection` fully
unused — zero callers even in tests, stronger than the test-only claim; `matching.rs`
`find_compatible_families` strategy ~110 LOC test-only behind `#[allow(dead_code)]`);
and the WET tracked/untracked SNF pair in `analysis/homology.rs` (~150 LOC overlap,
correctness risk — proposed `Tracker`-trait DRY refactor). Doc-comment fixes and the
matching.rs / SNF decisions left for a deliberate code session.

## [2026-06-01] refactor | Input/output boundary rename swept across the wiki

Commit `03757c0` renamed the boundary accessors from source/target to
input/output: `Sign::Source/Target` → `Sign::Input/Output`,
`Boundary { source, target }` → `{ input, output }`,
`cell_with_source_embedding` → `cell_with_input_embedding`, the
`protocol`/`output::types`/`codegen` struct fields, and
`step_target_strdiag_json` → `step_output_strdiag_json`. Swept the whole wiki to
match: fixed every code reference and aligned boundary-meaning prose to
input/output, keeping "source"/"target" only for source files and a partial map's
domain→image direction (which the code itself kept). ~20 content pages + `index.md`
touched (two via parallel agents, the rest by hand). Per user direction, recorded
the convention in auto-memory (`feedback-wiki-input-output-terminology`).

## [2026-06-01] refactor | Session-layer overhaul; new core-paste-tree page

Commits `77bb87b`/`8e86b74` (`explode`→`resume`), `eefecda` (retire `SessionFile`),
`59297b9` (drop `StepKind`, factor `install`, unify on `start`) and `463898c`
(prettify REPL, fix store module-key) rewrote the interactive session layer.
Rewrote `interactive-engine`: there is no `session.rs`/move-log anymore;
constructors are `from_store` (start) and proof-based `resume` (decompose a proof
diagram via `pseudo_normalise`/`flatten_at`/`realise_tree`); `HistoryEntry` is
display-only (no replay); `step_sign` = `Output`/`Input`; persistence is the proof
term via `proof_expr`/`store` + `resume`. Updated `interactive-daemon-web`
(`Request::{Start,Resume,Proof}`, no `Init`/`Save`; `start_session`/`resume_session`;
shared `install`) and `interactive-repl` (added `resume`, dropped `load` and the
goal-loop; semantic colour palette + `❯` prompt + `ReplHelper`; `Sign::Input`;
removed the now-fixed colour-doc-rot gotcha; `canonical_path` store key). Added new
impl page `core-paste-tree` (`src/core/paste_tree.rs`, split out of `diagram.rs`:
`PasteTree`, `realise_tree`, `flatten_at`, `top_generators`, `pseudo_normalise`)
and rewired `realise_tree`/`PasteTree` references in `core-diagram`,
`core-matching`, `output`, and `reconstruction` to point at it.

## [2026-06-01] refactor | Interpreter dotted-expression evaluation (decompose/execute)

Commit `a03b307` rewrote dotted diagram-expression evaluation in
`interpreter/diagram.rs` to a two-pass `decompose`/`execute` strategy: collect the
map-prefix / diagram / boundary-suffix cheaply, then take the boundary in one
direct `Diagram::boundary` call and apply maps innermost-out. Added a "Dotted
diagram expressions" section to `interpreter`, citing the new tests
(`boundary_suffix_collapses_to_one_direct_call`, `boundary_underflow_is_rejected`,
`maps_are_applied_after_the_boundary`, `delta_simplicial_identities_hold`).

## [2026-06-01] note | Re-verified source-drift items after the overhaul

Checked all nine `source-drift.md` items against current source: every one is
still present. `7e8d0a5` rewrote the `mod.rs` Interfaces prose but left the
`render_match_highlight` table entry (item 1d); `568caa7` pruned legacy docs but
left `docs/HOMOLOGY.md`'s rotted `src/core/homology.rs` path. The session-layer
rewrite shifted `engine.rs` line numbers, so item 1b's refs were refreshed to
symbol anchors.

## [2026-06-01] doc | Full verification + coverage pass over the whole codebase (minus plugins/trs)

One verifier/author agent per page-cluster (12 parallel agents) re-read current
`src/` and fixed staleness in place across all 31 content pages, plus three NEW
impl pages extending coverage from `src/` to the whole workspace: `cli`
(`cli/src/main.rs` — the `alifib` binary, argv→`RunMode`→interpret/ast/print/bench
or repl/serve/web/mcp), `web-backends` (`web/{shared,server,wasm,mcp}` — three
transports over one `WebRepl`), and `web-frontend` (`web/frontend` — CodeMirror
editor + canvas string-diagram GUI). `plugins/trs` excluded per direction.
`index.md` broadened to "module or workspace crate" with the three new rows;
`aux` now explicitly covers `src/aux/path.rs`.

Factual corrections to otherwise-solid `stable` pages (all stayed `stable`;
`module-open-semantics` stays `draft`): `is_round` is the directed-sphere
*disjointness* test (not "input = output equal") and gates cell construction, not
`paste` (`core-diagram`/`diagram`/`boundary`); greedy `build_greedy_family` is the
only live parallel-rewrite path, `find_compatible_families`/`whisker_rewrite`
unwired (`core-matching`/`pushout`); `used_names` not mirrored in `types.rs`
(`core-complex`); `include module` vs in-body `include` split + deleted-symbol refs
dropped (`interpreter`); `ast_fmt` is the indented `--ast` mode (`language-parser`);
SNF pair described per-caller and not-yet-DRY (`analysis`/`homology`); `init` is a
standalone daemon-preload constructor, `ResponseData` core fields fixed
(`interactive-engine`/`interactive-daemon-web`).

Adjudicated a cross-agent contradiction directly against source: the no-identities
invariant is **not** enforced at `PartialMap::extend` — a degenerate 1-cell→0-cell
map is constructible when both endpoints collapse. Corrected the false enforcement
claim on `[[0001-no-identities]]` and `[[core-partial-map]]`, and recorded it as a
correctness gap (item A) in `source-drift.md`'s 2026-06-01 batch, alongside the
other new drift (pre-rename source/target doc-comments across 6 files,
`InterpResult` "combine"→`merge`, dead `whisker_rewrite`/`LAngle`/`RAngle`/
`parse_complex`, and a stale `#[allow(dead_code)]` masking the *live*
`ogposet::closure`). Per standing direction, no source touched.

Bridge lint clean: every impl page carries `## Mathematics`, every concept page
`## Implementation`; the three new pages are bridged and given inbound links
(`cli` ← `interactive-repl`; `web-frontend` ← `web-backends`; `web-backends` ←
`interactive-daemon-web`). No orphans, no dangling wiki-links.
