# Testing

## Running the tests

```
cargo test
```

## Strategy

The bulk of the suite is unit tests scattered through `src/`. On top of those,
`tests/` holds the integration tests that load whole `.ali` files: `fill.rs`,
`cli_render.rs`, and `web_fill.rs` exercise the interactive layers, while the two
files below carry the interpreter's example-driven coverage. Example files come
from the curated library in `examples/` and from small feature-specific fixtures
in `tests/fixtures/`.

### `tests/interpreter.rs` — targeted assertions

Each test loads an example or fixture directly via `InterpretedFile::load` and
asserts specific semantic properties of the resulting normalized `Store`: cell
and type counts, which maps a type exposes, which diagrams are present, whether
holes were detected, and so on. These tests are meant to be stable — they
capture the intent of a particular language feature and should not need updating
unless the semantics change.

The `magma_interpretation` test is the most complete example of this style: it
checks the entire `Store` for the small `Magma.ali` fixture.

### `tests/golden_examples.rs` — snapshot tests

A curated slice of the `examples/` library — `Monoidal_examples`,
`Delta_complexes`, `Hole_examples`, `SKI`, and `TM` — is checked as whole-state
snapshots using [insta](https://insta.rs). The snapshot is the `Debug`
representation of the normalized `Store` after interpretation, with absolute file
paths reduced to filenames for portability. Snapshots live in `tests/snapshots/`
and are committed alongside the code.

The very heavy files (`Braided_Monoidal`, `Symmetric_Monoidal`, `LambdaSigma`)
are deliberately left out so the suite stays fast; everything snapshotted loads
in well under a second.

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
