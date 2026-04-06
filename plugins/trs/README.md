# ari2ali — TRS to alifib plugin

Converts a term rewriting system (TRS) in the standard ARI format into an
alifib type definition, encoding each rewrite rule as a generator boundary.

## Input format (ARI)

ARI files are S-expression lists. Each top-level form is one of:

```
(format TRS)          ; required header
(fun name arity)      ; function symbol declaration
(rule lhs rhs)        ; rewrite rule
```

Terms are either a variable (any atom not declared as a function symbol) or a
function application `(f t1 t2 ...)`. Constants (arity 0) are written `(f)`.

Example — group axioms (`test/group.ari`):

```
(format TRS)
(fun e 0)
(fun i 1)
(fun * 2)
(rule (* (e) x) x)
(rule (* x (e)) x)
(rule (* (i x) x) (e))
(rule (* x (i x)) (e))
(rule (* (* x y) z) (* x (* y z)))
```

## Output

The tool emits an alifib source file with two type blocks:

- **`Eq`** — a generic equation shape with `dom`, `cod`, `lhs`, `rhs`,
  `dir` (forward), and `inv` (backward) generators.
- **`<module>`** — one type named after the input file, containing:
  - Structural cells: `ob`, `copy`, `swap`, and (if constants or erasing rules
    are present) `unit`, `erase`, `unit_l`, `unit_r`
  - One generator per function symbol, typed `ob^n -> ob`
  - Identity 2-cells `id_1`, `id_2`, ...
  - Naturality equations for `copy` with respect to each function symbol
  - One generator per rewrite rule, with the encoded LHS and RHS as its source
    and target boundaries

The encoding works by routing variable wires through copy/erase/swap networks
to match the leaf order of each term.

## Building

From `trs/`:

```
cargo build --release
```

## Usage

```
cargo run -- <input.ari>
```

or after building:

```
./target/release/ari2ali-rs <input.ari>
```

Output is written to stdout. Skipped rules (e.g. rules with extra RHS
variables) are reported on stderr.

## Tests

The `test/` directory contains ARI input files and corresponding `.ali`
reference outputs:

| File | Description |
|------|-------------|
| `group.ari` | Group axioms |
| `SK90_2.01.ari` – `SK90_2.10.ari` | Sims–Kirchner benchmarks (selected) |
| `GTSSK07_cade01.ari` | CADE 2007 benchmark |

To check a single case against its reference:

```
cargo run -- test/group.ari > /tmp/out.ali && diff /tmp/out.ali test/group.ali
```
