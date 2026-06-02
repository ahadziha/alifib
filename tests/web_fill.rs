//! Integration tests for hole-filling over the web `run_command` protocol.

use std::path::PathBuf;

use alifib::interactive::web::WebRepl;
use serde_json::Value;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name),
    )
    .unwrap()
}

fn cmd(repl: &mut WebRepl, json: &str) -> Value {
    serde_json::from_str(&repl.run_command(json)).unwrap()
}

/// A 1-cell hole: list, fill via a rewrite, step to the target, done — and the
/// returned source carries the new clause while the hole list empties.
#[test]
fn web_fill_one_dim_hole() {
    let mut repl = WebRepl::new();
    let loaded: Value = serde_json::from_str(&repl.load_source(&fixture("RewriteFill.ali"))).unwrap();
    assert_eq!(loaded["status"], "ok");

    let holes = cmd(&mut repl, r#"{"command":"holes"}"#);
    let list = holes["data"]["holes"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["source_name"], "x");
    assert_eq!(list[0]["index"], 0);

    let started = cmd(&mut repl, r#"{"command":"fill","index":0}"#);
    assert_eq!(started["status"], "ok");
    assert_eq!(started["data"]["fill"]["source_name"], "x", "response is marked as a fill");

    let stepped = cmd(&mut repl, r#"{"command":"step","choice":0}"#);
    assert_eq!(stepped["status"], "ok");
    assert_eq!(stepped["data"]["target_reached"], true);

    let done = cmd(&mut repl, r#"{"command":"done"}"#);
    assert_eq!(done["status"], "ok");
    let new_source = done["data"]["source"].as_str().unwrap();
    assert!(new_source.contains("x =>"), "the clause is appended: {}", new_source);

    let after = cmd(&mut repl, r#"{"command":"holes"}"#);
    assert!(after["data"]["holes"].as_array().unwrap().is_empty(), "no holes left");
}

/// `done` before the target is reached errors and leaves the session intact —
/// the user can step and finalise afterwards.
#[test]
fn web_premature_done_keeps_session() {
    let mut repl = WebRepl::new();
    repl.load_source(&fixture("RewriteFill.ali"));
    cmd(&mut repl, r#"{"command":"fill","index":0}"#);

    let early = cmd(&mut repl, r#"{"command":"done"}"#);
    assert_eq!(early["status"], "error", "no proof yet");

    // Session survives: stepping then finishing still works.
    let stepped = cmd(&mut repl, r#"{"command":"step","choice":0}"#);
    assert_eq!(stepped["data"]["target_reached"], true);
    let done = cmd(&mut repl, r#"{"command":"done"}"#);
    assert_eq!(done["status"], "ok");
    assert!(done["data"]["source"].as_str().unwrap().contains("x =>"));
}

/// A 0-cell hole + a dependent: `e` is blocked by `a1`; filling `a1` via the
/// boundaryless chooser unblocks it.
#[test]
fn web_fill_zero_cell_session() {
    let mut repl = WebRepl::new();
    assert_eq!(
        serde_json::from_str::<Value>(&repl.load_source(&fixture("LayeredHole.ali"))).unwrap()["status"],
        "ok"
    );

    let holes = cmd(&mut repl, r#"{"command":"holes"}"#);
    let list = holes["data"]["holes"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    let a1 = list.iter().find(|h| h["source_name"] == "a1").unwrap();
    let e = list.iter().find(|h| h["source_name"] == "e").unwrap();
    assert_eq!(a1["dim"], 0);

    // `e` is blocked by its dependency `a1`.
    let blocked = cmd(&mut repl, &format!(r#"{{"command":"fill","index":{}}}"#, e["index"]));
    assert_eq!(blocked["status"], "error");

    // Start the 0-cell fill of `a1`: a clickable candidate list, no step diagram.
    let started = cmd(&mut repl, &format!(r#"{{"command":"fill","index":{}}}"#, a1["index"]));
    assert_eq!(started["status"], "ok");
    assert!(started["data"].get("current").is_none(), "no step diagram for a 0-cell fill");
    let choices = started["data"]["zero_cell"]["choices"].as_array().unwrap();
    let x = choices.iter().find(|c| c["name"] == "x").unwrap();

    let chosen = cmd(&mut repl, &format!(r#"{{"command":"step","choice":{}}}"#, x["index"]));
    assert_eq!(chosen["data"]["zero_cell"]["chosen"], "x");

    let done = cmd(&mut repl, r#"{"command":"done"}"#);
    assert_eq!(done["status"], "ok");
    assert!(done["data"]["source"].as_str().unwrap().contains("a1 => x"));

    // `a1` filled, `e` now the only — and unblocked — hole.
    let after = cmd(&mut repl, r#"{"command":"holes"}"#);
    let list = after["data"]["holes"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["source_name"], "e");
}
