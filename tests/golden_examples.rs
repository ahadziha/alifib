use std::path::PathBuf;

use alifib::aux::loader::Loader;
use alifib::output::{InterpretedFile, Store};

fn example_path(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn load_example(name: &str) -> Store {
    InterpretedFile::load(&Loader::default(vec![]), &example_path(name))
        .ok()
        .unwrap_or_else(|| panic!("{} should interpret without errors", name))
        .state
        .normalize()
}

// Replace absolute paths with just the filename so snapshots are portable.
// The pattern matches an absolute Unix path (starting with /) and captures the filename.
const PATH_FILTER: (&str, &str) = (r#"/(?:[^/"]+/)*([^/"]+\.ali)"#, "$1");

#[test]
fn golden_category() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("Category.ali"));
    });
}

#[test]
fn golden_frobenius() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("Frobenius.ali"));
    });
}

#[test]
fn golden_semigroup() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("Semigroup.ali"));
    });
}
