# Testing

## Running the tests

```
cargo test --workspace
```

## Strategy

The bulk of the suite is unit tests scattered through `src/`. On top of those,
`tests/` holds the integration tests that load whole `.ali` files: `fill.rs`,
`kernel.rs`, and `rpc.rs` exercise filling, the shared line evaluator, filesystem
policy, revisions, stale guards, and one-cut answers. `cli/tests/api_guard.rs`
covers HTTP framing, route guards, bounds, and connection concurrency;
`cli/tests/do_roundtrip.rs` covers the server/client workflow, disk reloads, and
project-relative paths. Server integration tests bind an automatically assigned
loopback port (`127.0.0.1:0`), avoiding fixed-port conflicts. The two files below
carry the interpreter's example-driven coverage. Example files come
from the curated library in `examples/` and from small feature-specific fixtures
in `tests/fixtures/`.

The engine's current-diagram/menu regression tests live beside it in
`src/interactive/engine.rs`: the Dining Philosophers routes cover manual steps,
resume, undo, auto, and backward mode; the Idem fill tests explore short routes.
`src/interpreter/diagram.rs` has `run_auto_independent_of_numbering`, comparing
`run auto` with its historical route. These guard normal-form menu ordering and
history without depending on the browser.

The browser's pure reducer and synchronization loop use Node's built-in test
runner, with no browser or DOM required:

```sh
just web-test
# equivalent: node --test web/frontend/test/*.test.js
```

`web/frontend/test/sync.test.js` covers one-cut commits, demand by missing
views, view holdings/fallbacks, restarts, stale actions, failures without
mutation replay, and commit-time editor writes (including closed/edited or
inactive tabs). `web/frontend/test/model.test.js` covers the pure derivations
from wire data: controls, session and menu keys, module labels, and thinness. `just test` runs the Rust
workspace tests; the Node suite is separate.

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
INSTA_UPDATE=always cargo test --workspace
```

If you have `cargo-insta` installed, you can instead review each change
interactively before accepting:

```
cargo insta test --workspace
cargo insta review
```
