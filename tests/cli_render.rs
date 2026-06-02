//! CLI rendering parity with the web front-end.
//!
//! Both the CLI ([`render`]) and the web (`web/frontend/src/app.js`) render the
//! *same* shared [`ResponseData`] produced by [`Session::apply`].  This test
//! drives a scripted session and pins the CLI's textual rendering, which mirrors
//! the web's `render*` functions line for line (the redex `[brackets]` are the
//! terminal analogue of the web's `repl-src` span; `→` and the `field:` labels
//! are identical).  A second pass drives the same script through [`WebRepl`] and
//! asserts the underlying `ResponseData` is byte-identical to the CLI's — so the
//! two front-ends can never diverge in substance, only in medium.

use std::path::PathBuf;

use alifib::interactive::display::Display;
use alifib::interactive::protocol::{build_type_detail_from_store, build_types_from_store, Request};
use alifib::interactive::render::{
    render_history, render_holes, render_proof, render_rules, render_state, render_store,
    render_type_detail, render_types,
};
use alifib::interactive::session::Session;
use alifib::interactive::web::WebRepl;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

#[test]
fn cli_transcript_matches_web_style() {
    let d = Display::plain();
    let mut s = Session::from_disk(&fixture("Idem.ali")).expect("load Idem");
    let root = s.root_path().to_owned();

    // start Idem lhs rhs
    let data = s
        .apply(Request::Start {
            source_file: root.clone(),
            type_name: "Idem".to_owned(),
            initial: "lhs".to_owned(),
            target: Some("rhs".to_owned()),
            backward: false,
        })
        .expect("start");
    assert_eq!(
        render_state(&d, &data),
        "step: 0\n\
         current: (id #0 id #0 id)\n\
         target: id\n\
         \n\
         available rewrites:\n\
         \x20 [0] idem  (id #0 id) → id\n\
         \x20     match: ([idem] #0 id)\n\
         \x20 [1] idem  (id #0 id) → id\n\
         \x20     match: (id #0 [idem])"
    );

    // proof at step 0 — a zero-step proof is still a proof: the identity on the
    // initial diagram, exactly what `store` would persist here.
    let data = s.apply(Request::Proof).expect("proof at step 0");
    assert_eq!(render_proof(&d, &data), "proof:\n  (id #0 id #0 id)");

    // apply 0
    let data = s.apply(Request::Step { choice: 0 }).expect("step");
    assert_eq!(
        render_state(&d, &data),
        "step: 1\n\
         current: (id #0 id)\n\
         target: id\n\
         \n\
         available rewrites:\n\
         \x20 [0] idem  (id #0 id) → id\n\
         \x20     match: [idem]"
    );

    // rules
    let data = s.apply(Request::ListRules).expect("rules");
    assert_eq!(render_rules(&d, &data.rules), "  idem  (id #0 id) → id");

    // proof — the re-parseable expression `store` would persist, with boundary
    let data = s.apply(Request::Proof).expect("proof");
    assert_eq!(
        render_proof(&d, &data),
        "proof : (id #0 id #0 id) → (id #0 id)\n  (idem #0 id)"
    );

    // store p
    let data = s.apply(Request::Store { name: "p".to_owned() }).expect("store");
    assert_eq!(render_store(&d, &data), "Stored 'p'\n  let p = (idem #0 id)");

    // history
    let data = s.apply(Request::History).expect("history");
    assert_eq!(render_history(&d, &data), "  1. idem [choice 0]");

    // types — CLI's own layout (the deliberate exception to web style), but now
    // shared verbatim with the web and rendered from the same data.
    let types = build_types_from_store(s.store(), &root);
    assert_eq!(render_types(&d, &types), "  Idem (dim 2, 3 generators, 6 diagrams, 1 map)");

    // type Idem — generators by dimension, diagrams with `= expr` (incl. the
    // just-stored `p`), maps.  Boundaries use `->` here, unlike the rewrite view.
    let detail = build_type_detail_from_store(s.store(), &root, "Idem").expect("type detail");
    assert_eq!(
        render_type_detail(&d, &detail),
        "Type Idem\n  [0]\n    ob\n  [1]\n    id : ob -> ob\n  [2]\n    idem : (id #0 id) -> id\n  \
         Diagrams\n    id : ob -> ob\n      = id\n    idem : (id #0 id) -> id\n      = idem\n    \
         lhs : ob -> ob\n      = (id #0 id #0 id)\n    ob\n      = ob\n    \
         p : (id #0 id #0 id) -> (id #0 id)\n      = (idem #0 id)\n    rhs : ob -> ob\n      = id\n  \
         Maps\n    Idem :: Idem"
    );

    // stop  (message-only command: canonical, capital-first, no period)
    let data = s.apply(Request::Stop).expect("stop");
    assert_eq!(data.message.as_deref(), Some("Session stopped"));

    // holes
    let data = s.apply(Request::Holes).expect("holes");
    assert_eq!(render_holes(&d, &data.holes), "(no open holes)");
}

/// The CLI and web front-ends drive the very same [`Session`] machine, so the
/// `ResponseData` they each receive for a given command must be byte-identical.
/// We compare the serialized payloads from a `Session` (CLI/daemon path) and a
/// `WebRepl` (web path) along a short script.
#[test]
fn cli_and_web_responses_are_identical() {
    // CLI/daemon path.
    let mut s = Session::from_disk(&fixture("Idem.ali")).expect("load Idem");
    let root = s.root_path().to_owned();
    let cli_start = s
        .apply(Request::Start {
            source_file: root.clone(),
            type_name: "Idem".to_owned(),
            initial: "lhs".to_owned(),
            target: Some("rhs".to_owned()),
            backward: false,
        })
        .expect("cli start");
    let cli_proof0 = s.apply(Request::Proof).expect("cli proof at step 0");
    let cli_step = s.apply(Request::Step { choice: 0 }).expect("cli step");
    let cli_proof = s.apply(Request::Proof).expect("cli proof");
    // `type` is served from the store (not Session::apply) on both front-ends.
    let cli_type = build_type_detail_from_store(s.store(), &root, "Idem").expect("cli type");

    // Web path: load the same source, then the same two commands.
    let mut web = WebRepl::new();
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    web.load_source(&source);
    let web_start: serde_json::Value =
        serde_json::from_str(&web.start_session("Idem", "lhs", Some("rhs".to_owned()), false)).unwrap();
    let web_proof0: serde_json::Value =
        serde_json::from_str(&web.run_command(r#"{"command":"proof"}"#)).unwrap();
    let web_step: serde_json::Value =
        serde_json::from_str(&web.run_command(r#"{"command":"step","choice":0}"#)).unwrap();
    let web_proof: serde_json::Value =
        serde_json::from_str(&web.run_command(r#"{"command":"proof"}"#)).unwrap();
    let web_type: serde_json::Value =
        serde_json::from_str(&web.run_command(r#"{"command":"type","name":"Idem"}"#)).unwrap();

    // The web wraps payloads as {"status":"ok","data":{...}}; compare `data`
    // against the CLI's serialized `ResponseData`.
    let cli_start_json: serde_json::Value = serde_json::to_value(&cli_start).unwrap();
    let cli_proof0_json: serde_json::Value = serde_json::to_value(&cli_proof0).unwrap();
    let cli_step_json: serde_json::Value = serde_json::to_value(&cli_step).unwrap();
    let cli_proof_json: serde_json::Value = serde_json::to_value(&cli_proof).unwrap();
    assert_eq!(web_start["data"], cli_start_json, "start responses differ");
    assert_eq!(web_proof0["data"], cli_proof0_json, "step-0 proof responses differ");
    assert_eq!(web_step["data"], cli_step_json, "step responses differ");
    assert_eq!(web_proof["data"], cli_proof_json, "proof responses differ");
    assert_eq!(web_type["data"]["type_detail"], serde_json::to_value(&cli_type).unwrap(),
        "type detail differs");
}
