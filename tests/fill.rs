//! Integration tests for the interactive hole-filling core (front-end-agnostic).

use std::path::PathBuf;

use alifib::interactive::engine::load_file_context;
use alifib::interactive::fill::{finalize, list_open_holes, start_fill, FillSession};

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

/// A 1-cell hole filled by a rewrite session: list it, drive the session to the
/// target, finalize, and confirm the map is hole-free afterwards.
#[test]
fn fill_one_dim_hole_via_rewrite() {
    let (store, path, _) = load_file_context(&fixture("RewriteFill.ali")).unwrap();

    let list = list_open_holes(&store, &path);
    assert_eq!(list.len(), 1, "one open hole (x)");
    assert_eq!(list[0].source_name, "x");

    let (ctx, mut session) = start_fill(&store, &path, &path, 0, false).unwrap();
    match &mut session {
        FillSession::Rewrite(engine) => {
            engine.step(0).expect("apply the rule r");
            assert!(engine.target_reached(), "session should reach the target");
        }
        FillSession::ZeroCell { .. } => panic!("expected a rewrite session for a 1-cell hole"),
    }

    let filler = session.filler().unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let (new_store, new_source) = finalize(&store, &ctx, &filler, &path, &source).unwrap();

    assert!(list_open_holes(&new_store, &path).is_empty(), "no holes left after filling x");
    assert!(new_source.contains("x =>"), "the new clause is appended to H");
}

/// A hole whose input and output boundaries coincide is filled by the identity
/// at step 0: the session reports target reached immediately and the filler is
/// the initial diagram.
#[test]
fn fill_identity_hole_at_step_zero() {
    let (store, path, _) = load_file_context(&fixture("IdHole.ali")).unwrap();

    let list = list_open_holes(&store, &path);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].source_name, "x");

    let (ctx, session) = start_fill(&store, &path, &path, 0, false).unwrap();
    match &session {
        FillSession::Rewrite(e) => assert!(e.target_reached(), "initial ≅ target → reached at step 0"),
        FillSession::ZeroCell(_) => panic!("expected a rewrite session for a 1-cell hole"),
    }

    // The step-0 filler is the initial diagram (the 0-cell `p`, a degenerate identity).
    let filler = session.filler().expect("identity proof at step 0");
    assert_eq!(filler.top_dim(), 0, "filler is the 0-dimensional initial diagram");

    let source = std::fs::read_to_string(&path).unwrap();
    let (new_store, new_source) = finalize(&store, &ctx, &filler, &path, &source).unwrap();
    assert!(new_source.contains("x =>"), "clause appended: {}", new_source);
    assert!(list_open_holes(&new_store, &path).is_empty(), "hole filled");
}

/// Dependency ordering + the boundaryless 0-cell session: `e` depends on `a1`,
/// so it is blocked until `a1` (a 0-cell hole) is chosen and filled.
#[test]
fn fill_zero_cell_then_dependent_becomes_available() {
    let (store, path, _) = load_file_context(&fixture("LayeredHole.ali")).unwrap();

    let list = list_open_holes(&store, &path);
    assert_eq!(list.len(), 2, "two open holes (a1, e)");
    let e_idx = list.iter().position(|h| h.source_name == "e").unwrap();
    let a1_idx = list.iter().position(|h| h.source_name == "a1").unwrap();

    // `e` is blocked by `a1`.
    assert!(
        start_fill(&store, &path, &path, e_idx, false).is_err(),
        "e should be blocked by its dependency a1"
    );

    // Fill the 0-cell hole `a1` by choosing the 0-cell `x` of the target.  The
    // chooser behaves like a session: choosing is a reversible step.
    let (ctx, mut session) = start_fill(&store, &path, &path, a1_idx, false).unwrap();
    match &mut session {
        FillSession::ZeroCell(zc) => {
            let k = zc.choices.iter().position(|(_, n)| n == "x").expect("target has 0-cell x");
            assert!(!zc.target_reached(), "no choice yet");
            zc.choose(k).unwrap();
            assert!(zc.target_reached(), "choosing reaches the target");
            assert_eq!(zc.chosen_name(), Some("x"));
            // Undo reopens the choice; redo restores it.
            zc.undo().unwrap();
            assert!(!zc.target_reached() && zc.can_redo());
            zc.redo().unwrap();
            assert_eq!(zc.chosen_name(), Some("x"));
        }
        FillSession::Rewrite(_) => panic!("expected a boundaryless session for a 0-cell hole"),
    }
    let filler = session.filler().unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let (new_store, new_source) = finalize(&store, &ctx, &filler, &path, &source).unwrap();

    assert!(new_source.contains("a1 => x"), "a1 => x appended");
    let new_list = list_open_holes(&new_store, &path);
    assert_eq!(new_list.len(), 1, "a1 filled; only e remains");
    assert_eq!(new_list[0].source_name, "e", "and e is now unblocked");
}
