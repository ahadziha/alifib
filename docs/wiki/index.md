# alifib wiki — index

LLM-maintained documentation of the alifib codebase and the higher-categorical
rewriting theory it implements. See [[CLAUDE]] for conventions. Browse in
Obsidian; the graph view shows the concept↔implementation bridge.

Legend: `stub` = skeleton only · `draft` = real content, code refs unverified ·
`stable` = current and verified.

Maintenance: [[source-drift]] tracks verified source-side rot (stale doc-comments,
dead code, WET SNF) found during the review pass — code bugs to fix, not wiki bugs.

## Concepts

Mathematical and language ideas alifib realises.

| Page | Summary | Status |
|------|---------|--------|
| [[molecule]] | Hadzihasanovic's molecules: the diagrams alifib computes with | stable |
| [[atom]] | The indecomposable building blocks (generators) of molecules | stable |
| [[regular-directed-complex]] | The combinatorial structure underlying molecules | stable |
| [[oriented-graded-poset]] | Faces with input/output orientation; the substrate of a diagram | stable |
| [[diagram]] | A labelled molecule; pasting, top dimension, boundaries | stable |
| [[boundary]] | Input/output boundaries $\partial^\pm_k$ and how they're computed | stable |
| [[partial-map]] | Refinement / total maps between complexes; `attach … along` | stable |
| [[rewriting]] | Rewrite steps: matching a rule's input and substituting | stable |
| [[pushout]] | The colimit gluing a rule cell onto the target along the matched input | stable |
| [[flow-graph]] | $F_k(U)$: matching as induced labelled subgraph isomorphism | stable |
| [[reconstruction]] | Recovering a paste-tree layering from a bare ogposet + labels | stable |
| [[hole]] | The `?` placeholder: a pending assignment in a map with holes; pure vs conditional, filling | stable |
| [[homology]] | Integer cellular homology of a complex via Smith Normal Form | stable |
| [[string-diagram]] | The Poincaré-dual presentation; node/wire/region layout | stable |
| [[module-system]] | The language's types, modules, `include`, `attach` (`open` is aspirational) | stable |

## Implementation

One page per major module or workspace crate. The library (`src/`) is the bulk;
the `cli/` and `web/` crates are the binaries and deployment wrappers around it.

| Page | Documents | Status |
|------|-----------|--------|
| [[core-complex]] | `src/core/complex.rs` — scoped namespace of generators/diagrams/maps | stable |
| [[core-diagram]] | `src/core/diagram.rs` — `Diagram`, `CellData`, `Sign`, boundaries | stable |
| [[core-ogposet]] | `src/core/ogposet.rs` — `Ogposet` shape, `Sign`, signed face/coface tables | stable |
| [[core-matching]] | `src/core/{matching,embeddings,pushout,flow,reconstruct}.rs` | stable |
| [[core-paste-tree]] | `src/core/paste_tree.rs` — paste trees: realise, flatten, pseudo-normalise | stable |
| [[core-partial-map]] | `src/core/partial_map.rs` + `src/interpreter/partial_map.rs` — `attach … along` | stable |
| [[interpreter]] | `src/interpreter/*` — eval, `GlobalStore`, types; holes ride out on maps | stable |
| [[language-parser]] | `src/language/*` — lexer, chumsky parser, AST | stable |
| [[output]] | `src/output/*` — normalize to a name-keyed, ID-free render tree | stable |
| [[interactive-engine]] | `src/interactive/engine.rs` — rewrite sessions (start/resume/from-diagrams) | stable |
| [[interactive-session]] | `src/interactive/{session,command,fill}.rs` — the shared command core + hole-filling | stable |
| [[interactive-repl]] | `src/interactive/{repl,cli,richtext,display,render}.rs` — terminal front end + shared renderer | stable |
| [[interactive-daemon-web]] | `src/interactive/{daemon,protocol,web}.rs` — `Session` over the wire | stable |
| [[analysis]] | `src/analysis/{homology,strdiag}.rs` | stable |
| [[aux]] | `src/aux/*` — ids, errors, loader, bitset/intset, graph | stable |
| [[codegen]] | `src/codegen.rs` — fluent builders for programmatic ASTs | stable |
| [[cli]] | `cli/` — the `alifib` binary; argv → `RunMode` → interpret/ast/print/bench or repl/serve/web/mcp | stable |
| [[web-backends]] | `web/{shared,server,wasm,mcp}` — three transports (HTTP, WASM, MCP) over one `WebRepl` | stable |
| [[web-frontend]] | `web/frontend/*` — browser GUI: CodeMirror editor, REPL, canvas string-diagram renderer | stable |

## Decisions

| Page | Decision | Status |
|------|----------|--------|
| [[0001-no-identities]] | No identity cells (follows RDC theory); this does *not* forbid maps lowering dimension | stable |
| [[0002-round-boundaries]] | A cell's input/output boundaries must be round (directed spheres) | stable |

## Open questions

| Page | Question | Status |
|------|----------|--------|
| [[module-open-semantics]] | What exactly does `open` bring into scope vs `include`? | draft |
