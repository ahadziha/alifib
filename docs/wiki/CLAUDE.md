# Wiki schema

This folder is the **alifib wiki**: an LLM-maintained, interlinked knowledge base
documenting the alifib codebase and the mathematics it implements. You (the LLM)
own this layer entirely — you write and maintain every page; the human reads,
browses in Obsidian, and directs. The wiki's job is to **teach the
correspondence** between the theory and the code — plainly, directly, in
tutorial voice (see *Voice* below).

This is a **codebase-documentation wiki**, not a literature wiki. The primary
source is the code: the `src/` library is the bulk, and the workspace binaries
and wrappers (`cli/`, `web/{shared,server,wasm,mcp}`, `web/frontend`) are covered
too — the whole codebase, not just the library. (`plugins/trs` was retired to the
`attic` branch, 2026-06-04.) Papers in `docs/papers/` are *reference only* — no
page per paper, never ingested wholesale — but Hadzihasanovic 2024
(*Combinatorics of higher-categorical diagrams*, `docs/papers/Hadzihasanovic -
2024 - Combinatorics of higher-categorical diagrams.pdf`) is the mathematical
primary source: concept pages cite it by **numbered item** (2.1.1, 3.3.2,
5.3.15), verified against the text — `pdftotext` the PDF and check; never cite
an item number from memory.

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

The bridge is more than links. A concept page's `## Implementation` section
must state **plainly what the code checks, what it assumes, and what is open**
— not merely list symbols. "The gate is `Diagram::parallelism`; roundness is
checked here and nowhere else; the sign-restriction is inherited from
traversal order and unproven above dimension 3" is a bridge. A bullet list of
function names is not.

## Voice: tutorial, not encyclopedia

Pages are read by someone who does not completely understand the code, does
not completely understand the book, but is intelligent enough to gain
something real from a clear explanation. Write for that reader. (Direction of
2026-06-10; the concept cluster from that pass is the register reference.)

- **Open with the question the page answers.** "How do you hand a pasting
  diagram to a computer?" beats a definition dropped from orbit. Give the
  plain answer first, then refine it.
- **Work a concrete example.** The 2-cell $\alpha : f \Rightarrow g \#_0 h$,
  the loop `a : pt -> pt`, the whiskered 2-cell that is *not* an atom —
  small, reusable across pages, computed out in full at least once.
- **Tell definitions as algorithms where possible.** If the book's definition
  can be told as steps the code transcribes (boundary = seed at extremal
  cells, close downward, adopt strays), tell it that way — the
  Implementation section then reads as a transcription, not a leap.
- **Explain jargon at first use**; complete sentences; plain words in the
  connective tissue, notation in the mathematics.
- **Correct the tempting misreading head-on.** When the natural summary is
  wrong ("alifib represents RDCs", "one top-dimensional cell = atom"), say
  so explicitly and explain why — often the most valuable sentences on the
  page.

## Claim status: theorem, discipline, or open

Every mathematical claim about the system sits in exactly one of three tiers.
Never blur them — conflation is how this wiki once said "molecules are RDCs
by construction, so the interpreter never needs a global regularity check"
while the (Atom) gate's soundness was in fact unproven.

1. **Theorem.** Proved in the book; cited by numbered item, verified against
   the text.
2. **Construction discipline.** True of the running system because every code
   path that could mint the object goes through a gate — not because anything
   checks it. Name the gates (`Diagram::parallelism`, `Diagram::pastability`)
   and write "maintained by construction, never checked", not "guaranteed".
   When a predicate is correct only on a restricted domain (e.g.
   `Ogposet::is_round` is the book's 3.2.5 only on *globular* shapes), state
   the domain and why every actual call site lies inside it (molecules are
   globular, 3.3.8).
3. **Open.** Believed, partially proved, or plausible. Gets its own
   `open-questions/` page stating exactly what is proved and what is not
   (model: [[atom-gluing-sign-invariant]] — sound for generators of dimension
   $\le 3$, open above), with an inbound link from **every** page whose
   claims lean on it.

Two corollaries. *Unverified content stays out*: a taxonomy or example you
could not actually construct or check against book and code does not go on a
page, however plausible — a failed search for a witness is itself a finding,
recorded in the open question, not asserted as fact. And *get the object of
study right*: alifib's values are pasting diagrams — strict functors
$\mathsf{Mol}/U \to X$ (5.3.13/5.3.16) stored as their labellings $\ell(d)$,
arbitrary colimits, **not** RDCs; only shapes are regular, and Proposition
5.3.15 (the labelling determines the functor exactly when the shape is
regular) is why shape-regularity matters at all. A framing that misstates
what the system represents is an error even when each local fact is true.

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

**Concept page** (a tutorial: teach the idea, then map it onto the code):

```
# <Concept>
Lead: the question this page answers, and the plain answer in prose + house
notation. If there is a tempting misreading, correct it here, explicitly.
## <Definition / How it works> — the precise account, told as an algorithm
   or through a worked example where possible; book citations by numbered
   item. Section titles may be bespoke ("The rewrite construction, step by
   step") — the skeleton is not a straitjacket.
## Implementation — the bridge: impl pages and concrete symbols, stating
   plainly what the code checks, what it assumes from construction
   discipline, and what is open (the three tiers of Claim status).
## Related — sibling concepts, each with a phrase saying why it is related.
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
pages but lacking their own page. Also lint the **claim tiers**: "by
construction" or "guaranteed" with no named gate; book citations without item
numbers; an open question asserted as fact, or a page leaning on one without
linking it; and tutorial regressions — a concept page that defines without a
motivating question or worked example.

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
`cli/` / `web/` crates) in the 2026-06-09 full audit/rewrite pass — except the
`open-questions/` pages `module-open-semantics` and `atom-gluing-sign-invariant`,
which stay `draft` because their subjects are genuinely unresolved. The concept
cluster was rewritten in tutorial voice on 2026-06-10 (the register reference for
*Voice* above), in the same pass that corrected the wiki's framing of what alifib
represents (see *Claim status*). `stub` and `draft` remain valid statuses for new
pages; promote to `stable` only after a page's code refs have actually been
re-verified, not merely written.
