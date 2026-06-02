//! CLI rendering parity with the web front-end.
//!
//! Both front-ends render the *same* shared [`RichText`] produced by
//! [`render_response`] from the [`ResponseData`] that [`Session::apply`] returns
//! — the CLI styles it to ANSI (here, via [`Display::style`] in plain mode), the
//! web to CSS spans.  `cli_transcript_matches_web_style` pins the styled CLI
//! transcript; `cli_and_web_responses_are_identical` drives the same script
//! through [`WebRepl`] and asserts both the underlying `data` *and* the carried
//! `rendered` segments are byte-identical to the CLI's — so the two front-ends
//! cannot diverge in layout or content, only in medium.

use std::path::PathBuf;

use alifib::interactive::display::Display;
use alifib::interactive::protocol::{
    build_type_detail_from_store, build_types_from_store, Request, ResponseData,
};
use alifib::interactive::richtext::{render_response, RenderKind};
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
    let styled = |kind, data: &ResponseData| d.style(&render_response(kind, data));

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
        styled(RenderKind::State, &data),
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
    assert_eq!(styled(RenderKind::Proof, &data), "proof:\n  (id #0 id #0 id)");

    // apply 0
    let data = s.apply(Request::Step { choice: 0 }).expect("step");
    assert_eq!(
        styled(RenderKind::State, &data),
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
    assert_eq!(styled(RenderKind::Rules, &data), "  idem  (id #0 id) → id");

    // proof — the re-parseable expression `store` would persist, with boundary
    let data = s.apply(Request::Proof).expect("proof");
    assert_eq!(
        styled(RenderKind::Proof, &data),
        "proof : (id #0 id #0 id) → (id #0 id)\n  (idem #0 id)"
    );

    // store p
    let data = s.apply(Request::Store { name: "p".to_owned() }).expect("store");
    assert_eq!(styled(RenderKind::Store, &data), "Stored 'p'\n  let p = (idem #0 id)");

    // history
    let data = s.apply(Request::History).expect("history");
    assert_eq!(styled(RenderKind::History, &data), "  1. idem [choice 0]");

    // types — CLI's own layout, shared verbatim with the web from the same data.
    let mut data = ResponseData::empty();
    data.types = build_types_from_store(s.store(), &root);
    assert_eq!(styled(RenderKind::Types, &data), "  Idem (dim 2, 3 generators, 6 diagrams, 1 map)");

    // type Idem — generators by dimension, diagrams with `= expr` (incl. the
    // just-stored `p`), maps.
    let mut data = ResponseData::empty();
    data.type_detail = Some(build_type_detail_from_store(s.store(), &root, "Idem").expect("type detail"));
    assert_eq!(
        styled(RenderKind::TypeDetail, &data),
        "Type Idem\n  [0]\n    ob\n  [1]\n    id : ob → ob\n  [2]\n    idem : (id #0 id) → id\n  \
         Diagrams\n    id : ob → ob\n      = id\n    idem : (id #0 id) → id\n      = idem\n    \
         lhs : ob → ob\n      = (id #0 id #0 id)\n    ob\n      = ob\n    \
         p : (id #0 id #0 id) → (id #0 id)\n      = (idem #0 id)\n    rhs : ob → ob\n      = id\n  \
         Maps\n    Idem :: Idem"
    );

    // stop  (message-only command: canonical, capital-first, no period)
    let data = s.apply(Request::Stop).expect("stop");
    assert_eq!(data.message.as_deref(), Some("Session stopped"));

    // holes
    let data = s.apply(Request::Holes).expect("holes");
    assert_eq!(styled(RenderKind::Holes, &data), "(no open holes)");
}

/// The CLI and web front-ends drive the very same [`Session`] machine and share
/// one [`render_response`], so for a given command both the `ResponseData` and
/// the carried `RichText` must be byte-identical.  We compare the `Session`
/// (CLI/daemon) path against the `WebRepl` (web) path along a short script.
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

    // Web path: load the same source, then run the same script.
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

    // The web wraps payloads as {"status":"ok","data":{…},"rendered":{…}}: the
    // `data` matches the CLI's `ResponseData`, and `rendered` matches the shared
    // `render_response` for that data — pinning both content and layout parity.
    let check = |web: &serde_json::Value, cli: &ResponseData, kind, what: &str| {
        assert_eq!(web["data"], serde_json::to_value(cli).unwrap(), "{what} data differs");
        assert_eq!(web["rendered"], serde_json::to_value(render_response(kind, cli)).unwrap(),
            "{what} rendered differs");
    };
    check(&web_start, &cli_start, RenderKind::State, "start");
    check(&web_proof0, &cli_proof0, RenderKind::Proof, "step-0 proof");
    check(&web_step, &cli_step, RenderKind::State, "step");
    check(&web_proof, &cli_proof, RenderKind::Proof, "proof");

    // `type` is served from the store on both; the data and rendering must match.
    let cli_type = {
        let mut d = ResponseData::empty();
        d.type_detail = Some(build_type_detail_from_store(s.store(), &root, "Idem").expect("cli type"));
        d
    };
    assert_eq!(web_type["data"]["type_detail"],
        serde_json::to_value(cli_type.type_detail.as_ref().unwrap()).unwrap(), "type detail differs");
    assert_eq!(web_type["rendered"],
        serde_json::to_value(render_response(RenderKind::TypeDetail, &cli_type)).unwrap(),
        "type rendered differs");
}
