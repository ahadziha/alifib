use std::path::PathBuf;
use std::sync::Arc;

use alifib::aux::loader::Loader;
use alifib::interpreter::{Context, interpret_program};
use alifib::output::InterpretedFile;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn magma_interpretation() {
    let loader = Loader::default(vec![]);
    let loaded = loader
        .load(&fixture("Magma.ali"))
        .expect("Magma.ali should load and parse");

    let context = Context::new_empty(loaded.canonical_path.clone());
    let result = interpret_program(&loaded.modules, context, &loaded.program);

    assert!(result.errors.is_empty(), "expected no interpretation errors");

    let file = InterpretedFile {
        state: Arc::clone(&result.context.state),
        holes: result.holes,
        source: loaded.source,
        path: loaded.canonical_path,
    };

    assert!(!file.has_holes());
    assert_eq!(file.state.cells.len(), 12);
    assert_eq!(file.state.types.len(), 5);
    assert_eq!(file.state.modules.len(), 1);

    let out = file.to_string();
    assert!(out.starts_with("12 cells, 5 types, 1 modules\n"));
    assert!(out.contains("Type Magma\n"));
    assert!(out.contains("m : Ob.ob Ob.ob -> Ob.ob"));
    assert!(out.contains("Type FrobeniusMagma\n"));
    assert!(out.contains("Comagma.c : Ob.ob -> Ob.ob Ob.ob"));
}
