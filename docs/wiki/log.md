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

## [2026-06-03] refactor | maps-with-holes + the unified interactive Session

Brought the wiki up to date with two major source restructurings landed since the
wiki was first written (`af611fc`…`a151779`): the **maps-with-holes** redesign and
the **shared interactive `Session`**.

*Holes.* `src/interpreter/inference.rs` (the two-phase constraint solver) was
**deleted**; a `?` is now a *pending assignment* of a partial map, recorded as a
`MapHole` (`src/core/map_hole.rs`) — pure (`arr => ?`) or conditional (`x => a`
with unmapped faces) — whose boundaries are paste trees over `Tag::Hole`
metavariables, never realised. Resolution is local: case-1 + collapse inference
and a `cascade` of ready conditionals, in `MapBuild`/`assign_cell`/`commit_one`
(`src/interpreter/partial_map.rs`); leftovers ride out on `EvalMap::holes` and are
filled interactively. Rewrote `[[hole]]`; updated `[[partial-map]]`,
`[[core-partial-map]]` (interpreter half), `[[interpreter]]` (dropped the whole
inference section, `InterpResult {context, errors}`, no `solved_holes`),
`[[output]]` (solved-hole reporting → `render_map_holes` listing), `[[cli]]`
(no `report_solved_holes`), `[[aux]]` (`HoleId`/`Tag::Hole`, dropped
`inference.rs` ref), and noted collapse inference's deliberate dimension-lowering
on `[[0001-no-identities]]`.

*Interactive.* The CLI, daemon, and web REPLs were unified onto one
`Session::apply` command surface (`session.rs`), one shared command parser
(`command.rs`), one structured renderer (`richtext.rs`), plus interactive
hole-filling (`fill.rs`); the per-engine `handle` was retired and `repl.rs` gutted
to a thin adapter. New page `[[interactive-session]]` (session + command + fill).
Rewrote `[[interactive-repl]]` (now repl/cli/richtext/display/render),
`[[interactive-daemon-web]]` (`Session::apply` is the surface; new
`holes`/`fill`/`done`/`load`/`save`/`backward` requests and `fill`/`holes`/
`constraints`/`zero_cell`/`source`/`module` response fields; web `State` enum →
`Option<Session>`), and updated `[[interactive-engine]]` (Session wraps it,
`from_diagrams` for fills, `init` now unused) and `[[web-backends]]`/
`[[web-frontend]]` cross-refs. README.md and INTERACTIVE.md updated to match.

## [2026-06-03] decision | retract fabricated no-lowering rule; record roundness (0002)

The original `0001-no-identities` (LLM-seeded in `b625cfb`, never a human
decision) conflated two unrelated things: the real fact that molecules have **no
identity cells**, and an invented rule that **maps may not lower dimension**. The
latter does not follow from the former and is wrong — a 1-cell whose endpoints
collapse maps to the 0-cell itself, not to a (non-existent) identity; collapse
inference lowers dimension on purpose. That fabrication had propagated into
`source-drift` (a phantom "⚠️ correctness gap", item A), `core-partial-map`, and
`partial-map`.

Corrected with the author: rewrote `0001` to the narrow honest statement (no
identities; lowering is legitimate; the only `extend` guard is no-*raising*);
**retracted source-drift item A**; fixed the `core-partial-map`/`partial-map`
gotchas. Added **`0002-round-boundaries`** for the genuine theory-mandated
constraint — a cell's input/output boundaries must be *round* (directed spheres),
enforced in `Diagram::parallelism` via `cell_with_input_embedding`. Also fixed
`interactive-engine`: `target_reached` no longer gates on `active_len > 0` (that
guard was removed in the source — a zero-step/identity proof is a valid proof).

## [2026-06-03] decision | sharpen 0001 — no identity *cells*, but unital composition

Refined `0001` after the author's correction: the imprecise "no identities"
framing was still wrong. The narrow truth is that alifib has no *representation of
an n-cell as an (n+1)-cell* (no degenerate identity cells) — but **composition is
unital**: k-pasting a diagram with its k-dimensional input/output boundary returns
an isomorphic diagram, so the boundaries are units of `#_k`. Hence a **zero-step
proof is valid** (the unit of `#_n`, represented by the initial n-diagram), which
is why `target_reached` correctly no longer gates on `active_len > 0`. The
practical consequence (with [[0002-round-boundaries]]): lower-dimensional
structure must be represented explicitly — e.g. TRS constants are 2-cells
`node : unit -> cod` over an explicit unit 1-cell (`examples/TRS.ali`), with
explicit unitor cells, not 2-cells with empty input.

## [2026-06-04] refactor | Source-drift 1a — drop phantom "memoised" claims in ogposet.rs

`normalisation` and `boundary_traverse` were doc-commented as memoised (module
doc L8–9, plus per-fn "Memoised by pointer identity" / "by (pointer, sign,
effective_k)"). No cache exists — no field, `OnceCell`, or `lazy_static`; both
call `traverse(…)` fresh every time. Deleted the four false mentions; the only
short-circuit, `normalisation`'s `is_normal()` identity return, is now described
as such (idempotence, not memoisation). Pure doc fix, no behaviour change; build
green. Adding a real `Arc`-keyed cache was deliberately *not* done — that is a
perf design decision, deferred. See `docs/wiki/source-drift.md`.

## [2026-06-04] refactor | Source-drift 1b — re-point dead find_matches doc-link

`Engine::rule_patterns` (engine.rs:73) doc-linked `[find_matches]`, which is now
`#[cfg(test)]`-only in `core/matching.rs` (matching.rs:120) — a dead intra-doc
link in any non-test build. Re-pointed at `[for_each_rule_candidate]`, the
per-call production enumerator that actually receives `rule_patterns`. Verified
the link resolves via `cargo doc --no-deps`. Pure doc fix.

Incidental: `cargo doc` surfaced four *unrelated* pre-existing broken intra-doc
links not in this backlog — `repl`, `load_source`, `set_cell`,
`register_generator`. Recorded here; not yet triaged.

## [2026-06-04] refactor | Source-drift 1c — WebRepl backend count

web.rs module doc claimed `WebRepl` is shared by "both web backends" (server +
wasm); the MCP server (`web/mcp/src/lib.rs:2,31`) is a third consumer. Reworded
to count-free phrasing that lists all three (server/wasm/mcp) so a future fourth
backend can't re-rot the number. The secondary "L48" mention from the original
item was already removed in the web.rs rewrite. Pure doc fix.

## [2026-06-04] refactor | Source-drift 1e — partial-map extension grammar shorthand

`interpret_partial_map_ext` (partial_map.rs:616) documented the extension grammar
as `{ prefix? clause* }`. Per `parser.rs:172–199` the block is delimited by
`LBrack`/`RBrack` with comma-separated entries, and the prefix is the map *before*
the bracket (`F [ … ]`). Corrected to `prefix? [ clause, … ]`. Note this is more
accurate than the backlog's proposed `[ prefix? clause* ]`, which mislocated the
prefix inside the brackets and implied whitespace repetition. Pure doc fix.

## [2026-06-04] refactor | Source-drift dead-code — KEEP find_compatible_families, document intent

The exhaustive maximal-independent-set parallel-rewrite strategy
(`find_compatible_families` + `max_independent_set_size`/`max_is_dfs`/
`enumerate_independent_sets_of_size`, all `#[allow(dead_code)]`, test-only
callers) was flagged "wire-in-or-delete, don't leave half-alive". User decision:
KEEP it. It solves a different problem from the live greedy path
(`greedy_parallel_auto_step`): deterministic enumeration of *all* maximal
compatible families vs greedily grabbing one. Exponential, so intentionally out
of the engine hot path, but a wanted backend capability. Resolved the "half-alive"
status not by deleting/wiring but by making retention explicit: a rationale note
on the function (why kept, why `allow(dead_code)` stays) + helper markers. Tests
retained and green; greedy disjointness is independently covered by
`greedy_parallel_in_four_chain`. No behaviour change.

## [2026-06-04] refactor | Source-drift WET — unify Smith Normal Form behind a Tracker trait

`homology.rs` carried two parallel SNF families — untracked (`smith_normal_form`,
for `matrix_rank`) and basis-tracked (`smith_normal_form_with_basis`, for
`compute_homology`'s torsion witnesses) — with near-identical pivot loops and
duplicated 2×2 integer row arithmetic (~150 LOC overlap; correctness risk of two
copies drifting). Parameterised over a `Tracker` trait (7 elementary mirror ops):
`NoTrack` no-ops; `FullTrack` mirrors row ops inverted onto `u_inv` and column ops
directly onto `v`, preserving `U·M·V = diag`. One `snf_reduce<T>` + generic
`find_and_move_pivot`/`eliminate_column`/`eliminate_row` now drive both. Tails kept
separate (plain: enforce+sort on the diagonal; tracked: raw diagonal +
`enforce_divisibility_tracked`/`sort_diag_tracked` in the caller). Net −88 LOC; all
34 homology tests + full 143-test lib suite green before and after.

## [2026-06-04] refactor | Surface torsion witnesses in the homology command

Follow-on to the SNF Tracker refactor: the tracked path's whole purpose —
torsion witnesses (a witnessing n-cycle + the (n+1)-chain preimage certifying its
order) — had no user-facing consumer. `build_homology_data` computed them via
`compute_homology` and discarded them, so `homology <name>` showed only groups +
Euler characteristic on every front-end (CLI/web/MCP share `richtext::homology`).
Added `TorsionWitnessInfo` (order + formatted cycle + preimage) to
`HomologyGroupInfo`, exposed `TorsionWitness::cycle_str`/`preimage_str`, populated
from `h.torsion_witnesses`, and rendered each as an indented sub-line under its
`H_d`. Verified: `homology RP2` → `H_1 = Z/2` with `cycle: c.t (preimage: L.t +
U.t)`; free spaces (S1, H_3 of RP^3) show none. Full 201-test workspace green.
Wiki: updated implementation/analysis.md (SNF bullet now describes the unified
Tracker driver) and implementation/interactive-daemon-web.md (HomologyInfo row).

## [2026-06-04] refactor | Source-drift Section B — rename-leftover doc-comments swept

Cleared the remaining `03757c0` source/target→input/output rename leftovers
flagged in source-drift.md §B. Fixed: `core/diagram.rs::is_round` (was "boundaries
are equal" + "prerequisite for pasting" → mirrors `Ogposet::is_round`: disjoint
input/output interiors); `output/normalize.rs::cell_from_diagram` (doc → input/
output, locals `src_diag`/`tgt_diag` → `in_diag`/`out_diag`);
`interactive/engine.rs::step_sign` (Target/Source → Output/Input);
`interactive/cli.rs::ServeArgs` ("Init request" → "Start", matching `Request`).
Found already-fixed/obsolete (no action): `interpreter/diagram.rs`
(`push_parallel_constraints` + `globular_propagate` gone with the inference
layer), `output/normalize.rs::sign_superscript`/`render_solved_hole` (deleted),
`interactive/display.rs` palette (already "input/output side"),
`interpreter/types.rs::InterpResult` (already "merge"). Pure doc fixes; build green.

## [2026-06-04] refactor | Source-drift dead-code — closure attr removed; whisker_rewrite deleted; LAngle/RAngle retracted

Three §C dead-code items. (1) `ogposet::closure` carried a stale
`#[allow(dead_code)]` despite live callers (`matching` isomorphism/reconstruct) —
removed the attribute; no warning, confirming liveness. (2) `diagram::whisker_rewrite`
(pub, zero callers/tests) — deleted along with its now-orphaned private helper
`fold_trees` (build confirmed the orphan). Unlike `find_compatible_families` it
solves no distinct problem: `construct_parallel_step` builds the same step as a
1-member family. Net −122 LOC. (3) `Token::LAngle`/`RAngle` flagged "dead" —
**RETRACTED, not a bug** (user caught it): `<…>` is the for-block variable-instance
syntax; `parser::for_body` consumes the tokens via a wildcard scan and
`eval::expand_body` substitutes `<var>` textually. Deleting them would break every
for-block (`LambdaSigma_Term.ali` confirms). Added a lexer comment so it isn't
re-flagged. Build + tests green throughout.

## [2026-06-04] refactor | Source-drift dead-code — delete parse_complex/complex_parser and engine::init

Two more §C/§refactor dead items, both genuine deletions. (1)
`language::parse_complex` + `parser::complex_parser` served the removed `@ <expr>`
REPL grammar; no callers/tests, and the live for-block re-parse uses the separate
`parse_complex_instrs` → `complex_instrs_parser`. Deleted both; `complex_parser`
only composed shared builders so nothing downstream orphaned. (2)
`engine::init` (pub, superseded by `Session::from_disk`) — deleted with its
only-caller helper `load_context` and the orphaned alias `LoadedRewriteContext`;
re-pointed the struct doc-link to `from_store`/`resume`. Section C now fully
resolved (closure attr, whisker_rewrite, LAngle/RAngle-retracted, parse_complex,
init). Build clean; 201-test workspace green; 57 language + 11 interactive tests green.

## [2026-06-04] refactor | Source-drift Section D — external-doc notes resolved

The three notes outside `src/`. (1) `docs/interp/interp.tex` (the user's paper):
fixed two factual errors — deleted the phantom "cached by pointer identity"
sentence (no cache exists, cf. item 1a) and corrected "input-extremal = no input
cofaces" → "no output cofaces" (`ogposet::extremal`: `Sign::Input` ⇒
`cofaces_out.is_empty()`). (2) `docs/HOMOLOGY.md`: stale path
`src/core/homology.rs` → `src/analysis/homology.rs` (L39, L138, text + links).
(3) `web/EXAMPLES.md`: **retracted, not a bug** — the `dist/` manifest/deploy
workflow it describes is real, implemented by `scripts/build_examples_manifest.py`
run from `.github/workflows/deploy.yml:57`, not `package.json`. Backlog now fully
worked through.

## [2026-06-05] doc | Full update pass: conceptual corrections, coverage, invariants, code-free guides

Brought the whole wiki current with `main` after the 2026-06-04 retirement point.
Two things had landed since the last verification: a batch of **conceptual
corrections** in `docs/CONCEPTS.md`/`README.md` (commits `81c374c`, `f83186b`,
`497df91`, `cdbe9a9`) and the **qualified-names + scoped-include** loader work.
Re-verified every content page against current `src/` (and `cli/`/`web/`), one
Opus agent per subsystem cluster, with the orchestrator owning the keystone shape
pages and this consolidation.

*Conceptual corrections (orchestrator).* Added new concept page
**`directed-complex`**: a *type* is a directed complex, **not necessarily
regular**; the *shapes* of values (atoms, molecules) are the stricter
[[regular-directed-complex|regular directed complexes]]. The witness is
`a : pt -> pt` — a round attaching shape (the 0-sphere of two endpoints) whose
labelling identifies both endpoints to `pt`, giving a non-regular but perfectly
good directed complex. Sharpened `[[0002-round-boundaries]]`: **roundness is a
condition on the attaching shape, not the realised boundary** (`Ogposet::is_round`
inspects the bare shape, ignores labels; enforced at cell construction by
`Diagram::parallelism`, *not* by pasting). Fixed `[[boundary]]` (dropped the false
"`#_k` is only defined when both arguments are round" — `pastability` never calls
`is_round`). Corrected `[[regular-directed-complex]]` (no longer "the structure
the whole language rests on"; **fixed the false claim that a 1-cell cannot map to
a 0-cell** — dimension-lowering is legitimate, only raising is guarded). Nudged
`[[0001-no-identities]]` to the shapes-regular / type-directed framing.

*Pasting ≠ composition.* Across `[[molecule]]`, `[[diagram]]`, `[[pushout]]`,
`[[interpreter]]`, `[[language-parser]]`, the interactive pages: pasting builds a
*larger* diagram; composition (reduce-to-one-cell) is absent from plain types.
`f #k g` pastes along the shared $k$-boundary; juxtaposition `f g` is *principal
pasting* at `k = min(dim) − 1` (`interpret_sequence_as_term`).

*Qualified names + scoped include resolution (new coverage).* Documented the
loader precedence — own dir → same-named subdir → `ALIFIB_PATH`, closest wins
(`Loader::with_parent_dir`/`find_file`, `[[aux]]`) — and the dotted-address
qualified-name walk through module-domain maps (`resolve_address_prefix_scope`,
`resolve_module_by_name`, `[[interpreter]]`/`[[module-system]]`), with leaves-first
topological interpretation into one shared `GlobalStore`.

*Drift fixed.* Deleted-symbol citations purged: `Diagram::whisker_rewrite`
(`[[diagram]]`, `[[pushout]]`), `parse_complex`/`complex_parser` and a bogus
`DComponent::Hole` (`[[language-parser]]`); the stale `--bench refuses holes`
gotcha (`[[cli]]`); a false `add_generator`+`add_diagram` pairing claim for the
attach/include path (`[[core-complex]]`); the stale `ogposet::closure` dead-code
note (`[[core-ogposet]]`); the `interp.tex` citation re-pointed to
`interp.pdf §3.1` (the `.tex` is no longer on disk). The SNF Tracker unification,
surfaced torsion witnesses, unified `Session`, three web backends, and the
recursive example-manifest workflow were all re-confirmed.

*Invariants + form.* Every page records its load-bearing invariants with
named-test evidence and is a code-free prose guide (no pasted Rust). Bridge lint
clean: every impl page carries `## Mathematics`, every concept page
`## Implementation`; `directed-complex` resolves the new inbound links. All
content pages `last-touched: 2026-06-05`; `module-open-semantics` stays `draft`.
No source touched (the one `src/`-side note — `aux::intset::intersection` still
dead — was already on record in `source-drift.md`).

## [2026-06-09] doc | Full audit/rewrite pass — every page verified and rewritten

Parallel-agent batch over all 39 content pages: every claim, symbol, grammar
production, and cited test re-verified against current `src/`, `cli/`, `web/`;
pages rewritten to the templates (history narration cut, bridges strengthened,
named-test evidence added). All pages `stable`, dated today; only
[[module-open-semantics]] stays `draft` by design. Lint clean: zero dangling
wiki-links.

Highlights of what was actually wrong:

- **Atom definition error (book 3.3.9)** on [[molecule]]/[[atom]]/[[diagram]]:
  "one top-dimensional cell" → *greatest element* (whisker counterexample).
  [[directed-complex]]'s definition rewritten — a non-regular directed complex is
  *not* an oriented graded poset (the loop's cover would need both signs); it is a
  presentation by regular shapes + identifying attachments.
- **[[core-complex]]** missed the maps-with-holes `add_map(name, domain, map,
  holes)` / `MapEntry` redesign; `find_map` returns only the hole-free part.
- **[[homology]]** claimed the daemon serves `homology` — it refuses it; and the
  Lens5 example matrix was wrong (SNF is diag(1,5)).
- **[[flow-graph]]**: $F_{n-1}$'s vertices are exactly the $n$-cells (the old
  text described $M_k$). Citation pinned to HK LICS 2023, Def. 61.
- **[[pushout]]** notation un-clashed with [[rewriting]] ($U$ = seam everywhere);
  multi-pushout is one colimit, not an iterated pushout.
- **[[hole]]**: explicit `?` fills are pinned in place, not appended.
- **[[language-parser]]**: block bodies are not brace-delimited; two new
  gotchas (`print_program` loses for-block bodies; `let total x = <diagram>`
  silently drops `total`). [[codegen]] has no in-tree consumer since
  `plugins/trs` moved to `attic` (dd92312).
- **[[interactive-daemon-web]]**: `Cell` needs an engine; the web load path
  bypasses `Session::from_virtual`; `save` is asymmetric by `LoadStrategy`.
  [[web-backends]] now documents the ninth MCP tool `resume_session` (692486c);
  [[web-frontend]] the boundary-selector reset (6a638d1).
- **[[interpreter]]**: `Tag::Hole` third variant, `(run auto on d)`, assert
  semantics and the `Arc::ptr_eq` domain-identity invariant documented.

Source-side rot found during the pass (13 stale doc-comments, 4 newly dead
symbols, GRAMMAR.md divergences, one test gap) is recorded as the third pass in
[[source-drift]] — recorded, not fixed, per the standing direction.

## [2026-06-10] doc | Diagrams are colimits, not RDCs — reframed around Prop 5.3.15; new open question

Audit against the book (3.2–3.3, 5.3) prompted by the question whether the code
implements [[regular-directed-complex]] at all. Findings folded into the wiki:

- **[[regular-directed-complex]]**: corrected the framing — alifib does not
  represent RDCs; a value is a pasting diagram, a strict functor
  $\mathsf{Mol}/U \to X$ (5.3.13/5.3.16), an arbitrary colimit whose labelling
  may identify cells. Only the *shape* is an RDC, and shape regularity matters
  because Proposition 5.3.15 makes the `(shape, labels)` encoding faithful
  exactly on regular shapes. Implementation section now points at the two
  construction gates (`Diagram::parallelism`, `Diagram::pastability`) and no
  longer claims regularity holds "by construction" unconditionally.
- **[[diagram]]**: added the functor/colimit definition and the 5.3.15 licence
  for storing only the combinatorial diagram $\ell(d)$.
- **New [[atom-gluing-sign-invariant]]** (draft): the book's (Atom) requires
  $\varphi$ restricting to $\varphi^\pm$; `parallelism` checks only positional
  equality of `Sign::Both` canonical boundaries. Sound for generators of
  dimension $\le 3$ (proved via the phase-separated traversal); open for
  dimension $\ge 4$. Verified sound elsewhere: `Ogposet::is_round` ⟺
  Definition 3.2.5 on globular inputs (molecules are globular, 3.3.8);
  `pastability` ⟺ (Paste) incl. the Lemma 3.3.7 clamping; `build_cell_shape`
  ⟺ 3.2.1's $\Delta^\mp\top := U_n, V_n$.

## [2026-06-10] doc | Tutorial rewrite of the concept cluster — code↔theory correspondence told plainly

Complete rewrite of seven concept pages in tutorial voice, per direction: readable
by someone who knows neither the code nor the book completely. Each page now
teaches the book's definition (with item numbers), walks an example, and states
directly what the code checks, what it assumes, and what is open.

- [[oriented-graded-poset]] — starts from the encoding problem; orientation as
  Hasse-edge labelling (2.1.1); worked example $\alpha : f \Rightarrow g \#_0 h$;
  extremality/purity/canonical form as the derived vocabulary.
- [[boundary]] — $\partial^\pm_k$ as a three-step seed-and-close algorithm;
  **globularity presented as a theorem about molecules (3.3.8), not an axiom**;
  roundness per 3.2.5 vs the disjoint-interiors check `is_round` actually runs,
  equivalent only on globular shapes.
- [[molecule]] — the three constructors as a grammar; paste history as the
  derivation certificate; the theorems each derivation earns and where the code
  silently relies on each.
- [[atom]] — the rewrite construction (3.2.1) in three explicit steps mapped
  line-by-line onto `parallelism` → `pushout` → `build_cell_shape`; the
  sign-restriction caveat surfaced inline.
- [[regular-directed-complex]] — leads with the correction: alifib does not
  represent RDCs; values are functors/colimits (5.3.13–5.3.16); Prop 5.3.15 as
  the licence for `(shape, labels)`; honest status of the by-construction
  invariant.
- [[diagram]] — "what does `let d = ...` name?"; functor vs combinatorial
  diagram $\ell(d)$; pasting/principal pasting/atoms-as-cells retained.
- [[directed-complex]] — the loop walk-through showing the realised type is not
  an ogposet (one Hasse edge, two signs); types as presentations.

Index rows updated for molecule, atom, boundary. No factual changes beyond the
2026-06-10 reframing pass; this is exposition.

## [2026-06-10] doc | Wiki schema updated — tutorial voice and claim-tier rules codified in CLAUDE.md

Saved the session's working rules into `docs/wiki/CLAUDE.md`: a *Voice* section
(tutorial register: open with the page's question, work an example, tell
definitions as algorithms, correct tempting misreadings head-on); a *Claim
status* section (every claim is theorem / construction discipline / open —
named gates, restricted domains stated, open questions get pages with inbound
links from dependents); the book promoted to citable-by-numbered-item primary
source (verified, never from memory); concept-page template reshaped to the
tutorial form; lint extended to catch tier conflation and tutorial regressions.
