# Testing

## Running the tests

```
cargo test
```

## Strategy

Tests are split across two files.

### `tests/interpreter.rs` — targeted assertions

Each test loads an example or fixture directly via `InterpretedFile::load` and
asserts specific semantic properties of the resulting `NormalizedStore`: cell
and type counts, which maps a type exposes, which diagrams are present, whether
holes were detected, and so on. These tests are meant to be stable — they
capture the intent of a particular language feature and should not need updating
unless the semantics change.

The `magma_interpretation` test is the most complete example of this style: it
checks the entire `NormalizedStore` for a small fixture file.

### `tests/golden_examples.rs` — snapshot tests

Four of the larger examples (Category, Frobenius, Semigroup, YangBaxter) are
checked as whole-state snapshots using [insta](https://insta.rs). The snapshot
is the `Debug` representation of `NormalizedStore` after interpretation, with
absolute file paths reduced to filenames for portability. Snapshots are stored
in `tests/snapshots/` and committed alongside the code.

These tests are expected to need occasional updates when a significant change
affects the output format or interpreter behaviour.

## Updating snapshots

When a change intentionally affects the output of the snapshotted examples,
regenerate and accept the snapshots in one step:

```
INSTA_UPDATE=always cargo test
```

If you have `cargo-insta` installed, you can instead review each change
interactively before accepting:

```
cargo insta test
cargo insta review
```
