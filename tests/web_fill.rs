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

/// Flatten a `rendered` RichText to its plain text (every segment, every line).
fn rendered_text(v: &Value) -> String {
    v["rendered"]["lines"].as_array().unwrap().iter()
        .map(|line| line.as_array().unwrap().iter()
            .map(|seg| seg["text"].as_str().unwrap())
            .collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `help` is served by the backend without a loaded source and, in web mode,
/// lists the web-only commands while dropping the CLI-only ones.
#[test]
fn web_help_drops_cli_only_commands() {
    let mut repl = WebRepl::new();
    let help = cmd(&mut repl, r#"{"command":"help","web":true}"#);
    assert_eq!(help["status"], "ok");
    let text = rendered_text(&help);

    assert!(text.contains("clear"), "web help lists the web-only `clear`");
    assert!(text.contains("Keyboard:"), "web help keeps the keyboard footer");
    assert!(text.contains("holes"), "shared commands are present");
    assert!(!text.contains("print"), "web help drops the CLI-only `print`");
    assert!(!text.contains("save <path>"), "web help drops the CLI-only `save`");
    assert!(!text.contains("quit"), "web help drops the CLI-only `exit`");
}

/// The web parses typed lines with the *shared* parser, so its usage/unknown
/// errors read exactly as the CLI's, and it classifies each line as a backend
/// request or a UI action.
#[test]
fn web_parse_command_shares_the_cli_parser() {
    let repl = WebRepl::new();
    let parse = |line: &str| -> Value { serde_json::from_str(&repl.parse_command(line)).unwrap() };

    // Unknown command and usage errors — the CLI's exact wording, built once.
    assert_eq!(parse("frobnicate")["status"], "error");
    assert_eq!(parse("frobnicate")["message"], "Unrecognised command 'frobnicate' — type 'help' for a list");
    assert_eq!(parse("type")["message"], "Usage: type <name>");

    // A backend command becomes a ready-to-run request (aliases included).
    let p = parse("a 0 1");
    assert_eq!(p["status"], "request");
    assert_eq!(p["request"], serde_json::json!({ "command": "step_multi", "choices": [0, 1] }));

    // A UI command becomes an action carrying its parsed arguments.
    let s = parse("start C a b");
    assert_eq!((s["status"].as_str(), s["action"].as_str()), (Some("action"), Some("start")));
    assert_eq!(
        (s["type_name"].as_str(), s["initial"].as_str(), s["target"].as_str()),
        (Some("C"), Some("a"), Some("b")),
    );

    // The CLI-only `print` is unknown to the web.
    assert_eq!(parse("print")["status"], "error");
}

/// A 0-cell fill is a session like any other: `proof`/`history`/`rules` are
/// empty before a choice, then read out the chosen cell; `stop` says "Session
/// stopped".  This pins the messaging the CLI and web must share.
#[test]
fn web_zero_cell_fill_behaves_like_a_session() {
    let mut repl = WebRepl::new();
    repl.load_source(&fixture("LayeredHole.ali"));

    // Hole 0 is the 0-cell `a1`; filling it opens a 0-cell session.
    let started = cmd(&mut repl, r#"{"command":"fill","index":0}"#);
    assert_eq!(started["status"], "ok");
    assert_eq!(started["data"]["fill"]["dim"], 0);

    // Before a choice — empty, exactly as a fresh rewrite session reads.
    assert_eq!(rendered_text(&cmd(&mut repl, r#"{"command":"proof"}"#)), "(no proof yet)");
    assert_eq!(rendered_text(&cmd(&mut repl, r#"{"command":"history"}"#)), "(no moves yet)");
    assert_eq!(rendered_text(&cmd(&mut repl, r#"{"command":"list_rules"}"#)), "(no rules)");

    // Choose the 0-cell at index 0 (`x`).
    let chosen = cmd(&mut repl, r#"{"command":"step","choice":0}"#);
    assert_eq!(chosen["data"]["target_reached"], true);

    // After — the proof is the chosen cell, the history records the choice.
    assert_eq!(rendered_text(&cmd(&mut repl, r#"{"command":"proof"}"#)), "proof :\n  x");
    assert_eq!(rendered_text(&cmd(&mut repl, r#"{"command":"history"}"#)), "  1. x [choice 0]");

    // Stopping a fill is "Session stopped", as ending a rewrite is.
    assert_eq!(cmd(&mut repl, r#"{"command":"stop"}"#)["data"]["message"], "Session stopped");
}

/// `show`/`status` while idle reports the loaded module instead of erroring,
/// matching the CLI.
#[test]
fn web_show_when_idle_reports_module() {
    let mut repl = WebRepl::new();
    repl.load_source(&fixture("RewriteFill.ali"));
    let shown = cmd(&mut repl, r#"{"command":"show"}"#);
    assert_eq!(shown["status"], "ok", "idle show does not error");
    assert!(rendered_text(&shown).contains("module:"), "idle show reports the module");
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
    // The report matches the CLI: "Filled ?x with <filler> : in -> out".
    assert!(done["data"]["message"].as_str().unwrap().starts_with("Filled ?x with "),
        "message: {}", done["data"]["message"]);
    let new_source = done["data"]["source"].as_str().unwrap();
    assert!(new_source.contains("x =>"), "the clause is appended: {}", new_source);

    let after = cmd(&mut repl, r#"{"command":"holes"}"#);
    // Empty `holes` is omitted from the JSON; treat absent as none-left.
    assert!(after["data"]["holes"].as_array().map_or(true, |a| a.is_empty()), "no holes left");
}

/// The `backward` flag is honoured, swapping initial/target (as in the CLI).
#[test]
fn web_fill_respects_backward() {
    let mut fwd = WebRepl::new();
    fwd.load_source(&fixture("RewriteFill.ali"));
    let f = cmd(&mut fwd, r#"{"command":"fill","index":0}"#);
    assert_eq!(f["data"]["current"]["label"], "p");
    assert_eq!(f["data"]["target"]["label"], "q");

    let mut bwd = WebRepl::new();
    bwd.load_source(&fixture("RewriteFill.ali"));
    let b = cmd(&mut bwd, r#"{"command":"fill","index":0,"backward":true}"#);
    assert_eq!(b["data"]["current"]["label"], "q", "backward swaps initial");
    assert_eq!(b["data"]["target"]["label"], "p", "backward swaps target");
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

    // `show` (used by the text REPL's show/status/rules) lists the candidates.
    let shown = cmd(&mut repl, r#"{"command":"show"}"#);
    assert_eq!(shown["data"]["zero_cell"]["choices"].as_array().unwrap().len(), choices.len());

    let chosen = cmd(&mut repl, &format!(r#"{{"command":"step","choice":{}}}"#, x["index"]));
    assert_eq!(chosen["data"]["zero_cell"]["chosen"], "x");
    assert_eq!(chosen["data"]["zero_cell"]["target_reached"], true);
    // Once chosen, the candidate list is empty — no silent re-selection.
    assert!(chosen["data"]["zero_cell"]["choices"].as_array().unwrap().is_empty());

    // Undo reopens the candidates; redo restores the pick — like a session.
    let undone = cmd(&mut repl, r#"{"command":"undo"}"#);
    assert_eq!(undone["data"]["zero_cell"]["target_reached"], false);
    assert!(!undone["data"]["zero_cell"]["choices"].as_array().unwrap().is_empty());
    assert_eq!(undone["data"]["zero_cell"]["can_redo"], true);
    let redone = cmd(&mut repl, r#"{"command":"redo"}"#);
    assert_eq!(redone["data"]["zero_cell"]["chosen"], "x");

    let done = cmd(&mut repl, r#"{"command":"done"}"#);
    assert_eq!(done["status"], "ok");
    assert_eq!(done["data"]["message"], "Filled ?a1 with x");
    assert!(done["data"]["source"].as_str().unwrap().contains("a1 => x"));

    // `a1` filled, `e` now the only — and unblocked — hole.
    let after = cmd(&mut repl, r#"{"command":"holes"}"#);
    let list = after["data"]["holes"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["source_name"], "e");
}
