use std::path::PathBuf;

use alifib::aux::loader::Loader;
use alifib::interpreter::InterpretedFile;
use alifib::output::{render_solved_hole, Store};

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

/// Snapshot-tests the rendered boundary strings for every hole in Hole.ali.
///
/// This catches regressions that would produce the *wrong* inferred boundary
/// (the structural tests in interpreter.rs only verify that *some* boundary
/// exists and that there are no inconsistencies).
#[test]
fn golden_hole_boundaries() {
    let file = InterpretedFile::load(&Loader::default(vec![]), &example_path("Hole.ali"))
        .ok()
        .expect("Hole.ali should interpret without errors");

    let rendered: Vec<String> = file.solved_holes.iter()
        .map(render_solved_hole)
        .collect();

    insta::assert_debug_snapshot!(rendered);
}
