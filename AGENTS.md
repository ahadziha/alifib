## Overview

This is `alifib`, an interpreter for directed higher-categorical rewriting. It is based on Amar Hadzihasanovic's notion of molecules.

We are building a core interpreter and toolchain for this language which must be simple, modular, and human auditable. Keep code beautiful, terse, and poetic.

## Running

The workspace's default binary is `alifib` (in `cli/`). Run it with `cargo run -- <args>` (or `just run <args>`); `cargo test` (or `just test`) runs the suite.

**Batch modes** — `alifib <file.ali> [flags]`:

- `cargo run -- examples/TRS.ali` — interpret and elaborate the file, printing its cells/types/modules to stdout (the default mode).
- `--ast` dumps the parsed AST; `--print` pretty-prints; `--bench N` times `N` reloads; `-o <out>` redirects to a file.

**Interactive front ends** — the subcommand comes first:

- `repl <file> [--type <t>] [--source <s>] [--target <t>] [--emacs]` — live rewrite session in the terminal.
- `web [<examples-dir>] [--bind <addr>]` — HTTP server + browser GUI (examples dir defaults to `examples/`). Prefer `just web`, which bundles the frontend JS first.
- `mcp [<examples-dir>]` — MCP server over the same engine.
- `serve [<file> --type <t> --source <s> [--target <t>]]` — line-protocol daemon.

The `Justfile` also carries the web build recipes (`just web` / `web-bun` / `web-wasm`) and the Quartz wiki (`just wiki` to build, `just wiki-dev` to serve with hot-reload).

## Documentation

When present, the curated wiki under `docs/wiki/` is the authoritative documentation layer: concept pages (the maths and language ideas) and one implementation page per module, indexed by `docs/wiki/index.md`, with claims pinned to named tests. Consult it **first** for any "how does X work" / semantics / concept question, before answering from source plus recall — then verify against current code, which may have advanced past a page. The wiki's own conventions live in `docs/wiki/CLAUDE.md`.

## Background notes (read on demand)

Unofficial working notes on the mathematical and conceptual picture behind alifib — deeper than the code, more informal than the wiki. Not authoritative (where they conflict with the code, the wiki, or the papers, those win), but they carry the *why*. Read when the task actually touches this material:

- `.claude/notes/CONCEPTS.md` — the conceptual/semantic picture (what a type, map, diagram, module *is*; what alifib is and is not).
- `.claude/notes/THREADS.md` — the intellectual lineage across Amar's papers and how each thread lands in alifib.
