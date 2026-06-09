# Wiki schema

This folder is the **alifib wiki**: an LLM-maintained, interlinked knowledge base
documenting the alifib codebase and the mathematics it implements. You (the LLM)
own this layer entirely — you write and maintain every page; the human reads,
browses in Obsidian, and directs.

This is a **codebase-documentation wiki**, not a literature wiki. The primary
source is the code: the `src/` library is the bulk, and the workspace binaries
and wrappers (`cli/`, `web/{shared,server,wasm,mcp}`, `web/frontend`) are covered
too — the whole codebase, not just the library. (`plugins/trs` was retired to the
`attic` branch, 2026-06-04.) Papers in `docs/papers/` are *reference only* — you may cite them, but
you never create a page per paper and never ingest them as sources.

## Layout

```
docs/wiki/
  CLAUDE.md            # this file — conventions and workflows
  index.md             # catalogue of every page, by category
  log.md               # append-only timeline of edits

  concepts/            # math & language ideas alifib implements
  implementation/      # code-side pages, ~one per major src module
  decisions/           # ADR-style records of design choices (NNNN-slug.md)
  open-questions/      # unresolved design/semantics questions
```

No `people/` and no `sources/` directory — this wiki documents code, not a
literature or biography.

## The bridge rule (the whole point)

alifib is mathematics realised as code. The value of this wiki is the
**bidirectional link between the two**:

- Every `concepts/` page has an **`## Implementation`** section linking to the
  `implementation/` pages and concrete code that realise it.
- Every `implementation/` page has a **`## Mathematics`** section linking to the
  `concepts/` pages it implements.

If you add a concept, wire it to its code. If you document a module, wire it to
its math. A concept with no implementation link or an implementation with no
math link is a lint failure.

## Page conventions

**Frontmatter** (YAML) on every page, so Dataview can query it:

```yaml
---
kind: concept | impl | decision | question
status: stub | draft | stable
last-touched: YYYY-MM-DD
code: [src/core/complex.rs, src/core/diagram.rs]   # impl pages: files this documents
---
```

**Status source of truth** is the page's own frontmatter. `index.md` mirrors it:
whenever you change a page's `status`, update its `index.md` row in the same
session so the two never disagree. Never hand-maintain a status in `index.md`
that contradicts the page.

**Links** use Obsidian wiki-links: `[[molecule]]`, `[[core-complex]]`. Page
basenames are unique across the whole wiki so bare wiki-links resolve.

**Code references** use repo-relative paths: `src/core/complex.rs`. You may add a
symbol — `Complex::add_diagram` — but **prefer symbol names over line numbers**:
line numbers drift as the code changes and silently rot. When you cite a line,
treat it as a hint, not a fact, and re-verify against current code (use the
`fff` MCP tools and `rg --no-ignore`) before asserting behaviour.

- **Private symbols are citable.** Much load-bearing logic lives in
  `fn`/`pub(crate)`/`pub(super)` items with no rustdoc page. Cite them as
  `module::symbol` like any other; wiki code-refs point at *source*, not rustdoc.
  Mark a symbol *(internal)* when its privacy is worth flagging to the reader.
- **Cite named tests as behavioural evidence.** A test name pins an intended
  behaviour better than prose — `boundary_normal_clamps_history_to_top_dim` says
  exactly what is guaranteed. Reference test names freely as proof of a claim.
- **Pasted code blocks rot like line numbers.** Reproduce a struct/signature
  only when the *exact text* is load-bearing, and treat it as a re-verify hint,
  not a fact. Prefer prose + symbol names to a copied definition that will drift.

**Mathematical notation** uses LaTeX — Obsidian renders MathJax natively. Inline
`$\partial^-_k$`, display `$$ \dots $$`. House notation (decide once, keep
consistent everywhere):

- Input/output boundary of dimension $k$: $\partial^-_k$ and $\partial^+_k$.
- Pasting / composition: $\#_k$ for composition along the $k$-boundary.
- A molecule/diagram: $U, V$; an atom: lowercase $a, b$; dimension: $\dim$.

**Diagrams**: link to [quiver](https://q.uiver.app/) URLs, or embed TikZ for the
Obsidian TikZJax plugin. Pasting diagrams are a recurring need — prefer quiver
links unless a rendered figure earns its keep.

## Page templates

Anchors so pages stay consistent and concept pages don't drift into mirroring
their impl page. Sections are a skeleton, not a straitjacket — drop or add as the
subject demands, but keep the bridge section.

**Implementation page** (codified from `core-matching`, the reference page):

```
# <slug> — <one-line what it is>
1. What it owns — the module's responsibility in one short paragraph.
2. Key public types — a table or list; what each is for.
3. Data flow — how the pieces connect (a numbered pipeline or ASCII diagram).
4. Non-obvious invariants & gotchas — the load-bearing, easily-missed facts;
   cite named tests as evidence.
## Mathematics — links to the concepts this realises (the bridge).
```

**Concept page** (math-first — describe the idea, then point at code):

```
# <Concept>
Lead paragraph: the definition in prose + house notation.
## Definition — the precise mathematical account; cite a paper in docs/papers/
   inline if the definition comes from one (no page per paper).
## Implementation — links to impl page(s) and concrete symbols (the bridge).
## Related — sibling concepts.
```

## Workflows

**Document a module.** When asked to document `src/<module>`: read the real code
(don't trust stale notes), write or update `implementation/<slug>.md` from the
impl-page template above. Fill the `## Mathematics` section with `concepts/`
links. Then complete the **closing checklist** — it is not optional:

1. `## Mathematics` (impl) / `## Implementation` (concept) section present and
   pointing at real pages — the bridge rule.
2. Page frontmatter `status` and `last-touched` set.
3. `index.md` row updated to match (status + summary).
4. `log.md` entry appended (`## [date] doc | <module>`).
5. Dangling links you created noted (a slug with no page file yet).

When many pages are written in parallel by an orchestrator, steps 3–4 (the shared
`index.md`/`log.md`) are consolidated once by the orchestrator after the batch to
avoid write races — but always in the same session, never deferred.

**Document a concept.** Write or update `concepts/<slug>.md`: the mathematical
definition (with notation), why alifib needs it, and the `## Implementation`
section pointing at the code. If the concept comes from a paper in
`docs/papers/`, cite it inline — but do not make a page for the paper.

**Record a decision.** When a non-obvious design choice is made (or discovered),
write `decisions/NNNN-slug.md` with: Context / Decision / Consequences / Code
refs. Number sequentially.

**After a code change.** If a diff changes behaviour this wiki documents, update
the affected `implementation/` page (and any `concepts/` page whose
`## Implementation` section cites the changed code) in the same breath. Append to
`log.md`. Stale code refs are the main failure mode — fix them when you touch a
page.

**Lint.** On request, health-check the wiki: concept pages missing an
`## Implementation` link (or vice versa); code refs that no longer resolve;
`status: stub` pages that are overdue; orphan pages with no inbound wiki-links;
modules in `src/` with no `implementation/` page yet; concepts mentioned across
pages but lacking their own page.

## index.md and log.md

**index.md** is content-oriented: every page listed under its category with a
one-line summary. Update on every page you create or substantially change.

**log.md** is chronological and append-only. Each entry starts with a fixed
prefix so it stays greppable:

```
## [YYYY-MM-DD] <kind> | <short description>
```

where `<kind>` ∈ `doc` (documented a module/concept), `decision`, `lint`,
`refactor` (updated docs after a code change). `grep "^## \[" log.md | tail` then
shows recent activity.

## Status of the wiki

All content pages are `status: stable` — verified against current `src/` (and the
`cli/` / `web/` crates) in the 2026-06-09 full audit/rewrite pass — except
`open-questions/module-open-semantics`, which stays `draft` because its subject
(`open` scoping) is genuinely unresolved. `stub` and `draft` remain valid statuses
for new pages; promote to `stable` only after a page's code refs have actually been
re-verified, not merely written.
