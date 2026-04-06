use std::path::PathBuf;
use crate::aux::loader::Loader;

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
    let file = super::execute_file(&loader, &fixture("Magma.ali"))
        .expect("Magma.ali should interpret without errors");

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
