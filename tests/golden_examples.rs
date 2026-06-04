use std::path::PathBuf;

use alifib::aux::loader::Loader;
use alifib::interpreter::InterpretedFile;
use alifib::output::Store;

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

// Golden snapshots over a curated slice of the `examples/` library — the main
// point of reference. The very heavy files (Braided_Monoidal, Symmetric_Monoidal,
// LambdaSigma) are deliberately excluded to keep the suite fast; everything here
// loads in well under a second.

#[test]
fn golden_monoidal_examples() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("Monoidal_examples.ali"));
    });
}

#[test]
fn golden_delta_complexes() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("Delta_complexes.ali"));
    });
}

#[test]
fn golden_hole_examples() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("Hole_examples.ali"));
    });
}

#[test]
fn golden_ski() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("SKI.ali"));
    });
}

#[test]
fn golden_tm() {
    insta::with_settings!({ filters => vec![PATH_FILTER] }, {
        insta::assert_debug_snapshot!(load_example("TM.ali"));
    });
}
