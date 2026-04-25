//! End-to-end MCP handshake test driving the in-process `serve` loop with
//! pipes — exercises the JSON-RPC framing, the initialise/tools-list/tools-call
//! sequence, and the `load_source` → `get_types` flow against a real on-disk
//! examples directory.

use std::io::{BufReader, Cursor};

use alifib_web_mcp::serve;
use alifib_web_shared::ExampleSet;
use serde_json::{Value, json};

fn drive(requests: Vec<Value>, examples: ExampleSet) -> Vec<Value> {
    let mut input = String::new();
    for r in requests {
        input.push_str(&r.to_string());
        input.push('\n');
    }
    let reader = BufReader::new(Cursor::new(input));
    let mut output = Vec::<u8>::new();
    serve(reader, &mut output, examples).expect("serve loop returned error");
    String::from_utf8(output)
        .expect("non-UTF8 output")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid JSON line"))
        .collect()
}

#[test]
fn initialize_returns_protocol_metadata() {
    let dir = std::env::temp_dir().join(format!("alifib-mcp-init-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let examples = ExampleSet::new(&dir);

    let responses = drive(
        vec![json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})],
        examples,
    );
    assert_eq!(responses.len(), 1);
    let r = &responses[0]["result"];
    assert_eq!(r["protocolVersion"], "2024-11-05");
    assert!(r["capabilities"]["tools"].is_object());
    assert_eq!(r["serverInfo"]["name"], "alifib-mcp");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tools_list_advertises_expected_surface() {
    let dir = std::env::temp_dir().join(format!("alifib-mcp-list-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let examples = ExampleSet::new(&dir);

    let responses = drive(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        ],
        examples,
    );

    // Two responses (initialize, tools/list) — the notification must not get one.
    assert_eq!(responses.len(), 2);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in [
        "load_source",
        "init_session",
        "run_command",
        "get_types",
        "get_strdiag",
        "get_session_strdiag",
        "get_rewrite_preview_strdiag",
        "list_examples",
    ] {
        assert!(names.contains(&expected), "missing tool {expected}");
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn load_source_then_get_types_round_trips() {
    let dir = std::env::temp_dir().join(format!("alifib-mcp-types-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let examples = ExampleSet::new(&dir);

    let source = "@Type\nFoo <<= { pt }";
    let responses = drive(
        vec![
            json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
                "name":"load_source","arguments":{"source": source}
            }}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
                "name":"get_types","arguments":{}
            }}),
        ],
        examples,
    );

    assert_eq!(responses.len(), 2);
    let load_text = responses[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let load: Value = serde_json::from_str(load_text).unwrap();
    assert_eq!(load["status"], "ok");
    assert!(load["types"].is_array());

    let types_text = responses[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    let types: Value = serde_json::from_str(types_text).unwrap();
    assert_eq!(types["status"], "ok");
    let names: Vec<&str> = types["data"]["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Foo"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn examples_dir_auto_seeded_for_include() {
    // Drop a tiny example into a fresh dir, then load source that `include`s
    // it.  If auto-seeding works, the load succeeds; otherwise the loader
    // would fail to resolve `Theory`.
    let dir = std::env::temp_dir().join(format!("alifib-mcp-seed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Theory.ali"), "@Type\nTheory <<= { pt }").unwrap();
    let examples = ExampleSet::new(&dir);

    let source = "@Type\ninclude Theory,\n";
    let responses = drive(
        vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"load_source","arguments":{"source": source}
        }})],
        examples,
    );
    let text = responses[0]["result"]["content"][0]["text"].as_str().unwrap();
    let env: Value = serde_json::from_str(text).unwrap();
    assert_eq!(
        env["status"], "ok",
        "expected ok envelope, got {}",
        text
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn list_examples_sees_seeded_dir() {
    let dir = std::env::temp_dir().join(format!("alifib-mcp-listex-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Theory.ali"), "@Type\nTheory <<= { pt }").unwrap();
    std::fs::write(dir.join("Foo.ali"), "@Type\nFoo <<= { pt }").unwrap();
    let examples = ExampleSet::new(&dir);

    let responses = drive(
        vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"list_examples","arguments":{}
        }})],
        examples,
    );
    let text = responses[0]["result"]["content"][0]["text"].as_str().unwrap();
    let env: Value = serde_json::from_str(text).unwrap();
    assert_eq!(env["status"], "ok");
    let names: Vec<&str> = env["data"]["examples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Foo", "Theory"]);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_tool_returns_iserror() {
    let dir = std::env::temp_dir().join(format!("alifib-mcp-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let examples = ExampleSet::new(&dir);

    let responses = drive(
        vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
            "name":"nope","arguments":{}
        }})],
        examples,
    );
    assert_eq!(responses[0]["result"]["isError"], true);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_method_returns_jsonrpc_error() {
    let dir = std::env::temp_dir().join(format!("alifib-mcp-meth-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let examples = ExampleSet::new(&dir);

    let responses = drive(
        vec![json!({"jsonrpc":"2.0","id":7,"method":"bogus/method"})],
        examples,
    );
    assert_eq!(responses[0]["error"]["code"], -32601);

    let _ = std::fs::remove_dir_all(&dir);
}
