//! Shared-wire cuts, stale-call guards, views, and user-supplied modules.

use std::path::PathBuf;

use alifib::interactive::kernel::{CanonicalDir, FsAccess, Kernel, Reply, Status};
use alifib::interactive::rpc::Rpc;
use alifib::language::ast::Span;
use alifib::language::error::Error;
use serde_json::{Value, json};

#[test]
fn include_user_supplied_module_resolves() {
    let mut rpc = Rpc::new(Kernel::new(FsAccess::Denied("test has no disk")), "0".to_owned());
    let request = json!({
        "method": "load_source",
        "args": {
            "source": "@Type\ninclude Theory,\nlet S = Theory.Set",
            "modules": { "Theory.ali": "@Type\nSet <<= { pt, id: pt -> pt }" },
            "page": {}
        }
    });
    let out: Value = serde_json::from_str(&rpc.call(&request.to_string())).unwrap();
    let replies = out["replies"].as_array().expect("load replies");
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["status"], "ok", "include Theory failed: {out}");
    assert!(out["module"]["types"].as_array().unwrap().iter()
        .any(|ty| ty["name"] == "Set"), "included Set is listed: {out}");
}

const INSTANCE: &str = "1780000000000000001";
const IDEM: &str = include_str!("fixtures/Idem.ali");

fn call(rpc: &mut Rpc, request: Value) -> Value {
    let answer: Value = serde_json::from_str(&rpc.call(&request.to_string())).unwrap();
    assert_eq!(answer["status"], "ok", "{answer}");
    answer
}

fn poll(rpc: &mut Rpc, page: Value) -> Value {
    call(rpc, json!({"method": "poll", "args": {"page": page}}))
}

fn wire_eval(rpc: &mut Rpc, line: &str) -> Value {
    call(rpc, json!({"method": "eval", "args": {"line": line}}))
}

fn assert_ok_replies(answer: &Value, count: usize) {
    let replies = answer["replies"].as_array().expect("replies");
    assert_eq!(replies.len(), count, "{answer}");
    for reply in replies {
        assert_eq!(reply["status"], "ok", "{answer}");
        assert_eq!(reply["generation"], answer["generation"], "{answer}");
    }
}

fn memory_kernel() -> Kernel {
    let mut kernel = Kernel::new(FsAccess::Denied("wire tests have no disk"));
    let reply = kernel.open_text(IDEM.to_owned(), Default::default(), Some("Idem.ali"));
    assert!(reply.status == Status::Ok, "{:?}", reply.message);
    kernel
}

fn memory_rpc() -> Rpc {
    Rpc::new(memory_kernel(), INSTANCE.to_owned())
}

fn seen(answer: &Value) -> Value {
    json!({
        "instance": answer["instance"],
        "revision": answer["revision"],
        "module_revision": answer["module_revision"]
    })
}

fn expect(answer: &Value) -> Value {
    json!({"instance": answer["instance"], "revision": answer["revision"]})
}

fn counters(answer: &Value) -> Value {
    json!({
        "instance": answer["instance"],
        "generation": answer["generation"],
        "revision": answer["revision"],
        "module_revision": answer["module_revision"]
    })
}

#[test]
fn rpc_do_gets_replies() {
    let mut rpc = memory_rpc();
    let initial = poll(&mut rpc, json!({}));
    let answer = wire_eval(&mut rpc, "start Idem lhs rhs; apply 0; types");
    assert_ok_replies(&answer, 3);
    assert_eq!(answer["replies"][0]["input"], "start Idem lhs rhs");
    assert_eq!(answer["replies"][1]["input"], "apply 0");
    assert_eq!(answer["replies"][2]["input"], "types");
    assert_eq!(answer["instance"], INSTANCE);
    assert_eq!(answer["generation"], initial["generation"]);
    assert_eq!(answer["module_revision"], initial["module_revision"]);
    assert_eq!(answer["revision"].as_u64().unwrap(), initial["revision"].as_u64().unwrap() + 2);
    for field in ["state", "module", "views"] {
        assert!(answer.get(field).is_none(), "no page must omit {field}: {answer}");
    }
    assert_eq!(answer.as_object().unwrap().len(), 6, "only status, counters and replies");
}

#[test]
fn rpc_answer_brings_others_changes() {
    let mut rpc = memory_rpc();
    let page = poll(&mut rpc, json!({}));
    // A second caller uses no page record and edits the shared module.
    let other = wire_eval(&mut rpc, "start Idem lhs rhs; apply 0; store other");
    assert_ok_replies(&other, 3);
    let caught_up = poll(&mut rpc, json!({"seen": seen(&page)}));
    assert_eq!(counters(&caught_up), counters(&other));
    assert_eq!(caught_up["state"]["rewrite"]["step_count"], 1);
    assert!(caught_up["module"]["source"].as_str().unwrap().contains("let other ="));
    assert_ne!(caught_up["state"], page["state"]);
    assert_ne!(caught_up["module"], page["module"]);
    assert!(caught_up.get("replies").is_none());
    let settled = poll(&mut rpc, json!({"seen": seen(&caught_up)}));
    assert!(settled.get("state").is_none());
    assert!(settled.get("module").is_none());
}

fn assert_refusal(answer: &Value, before: &Value) {
    assert_eq!(counters(answer), counters(before), "refusal must not move any clock");
    let replies = answer["replies"].as_array().expect("refusal replies");
    assert_eq!(replies.len(), 1, "{answer}");
    assert_eq!(replies[0]["status"], "error");
    assert_eq!(replies[0]["generation"], before["generation"]);
    assert_eq!(replies[0]["edited"], false);
    assert_eq!(replies[0]["message"], "not run: the session changed since this page last showed it");
}

#[test]
fn rpc_stale_expect_is_refused() {
    let mut rpc = memory_rpc();
    let old = poll(&mut rpc, json!({}));
    assert_ok_replies(&wire_eval(&mut rpc, "start Idem lhs rhs"), 1);
    let before = poll(&mut rpc, json!({}));
    let refused = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "apply 0; store forbidden", "expect": expect(&old),
        "page": {"seen": seen(&old)}
    }}));
    assert_refusal(&refused, &before);
    assert_eq!(refused["state"], before["state"]);
    assert!(refused.get("module").is_none(), "module has not changed");

    let mut wrong_instance = expect(&before);
    wrong_instance["instance"] = json!("another instance");
    let mut wrong_seen = seen(&before);
    wrong_seen["instance"] = wrong_instance["instance"].clone();
    let refused = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "apply 0", "expect": wrong_instance, "page": {"seen": wrong_seen}
    }}));
    assert_refusal(&refused, &before);
    assert_eq!(refused["state"], before["state"]);
    assert_eq!(refused["module"], before["module"]);

    // Stale expect alone does not force snapshots when seen is already current.
    let refused = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "apply 0", "expect": expect(&old), "page": {"seen": seen(&before)}
    }}));
    assert_refusal(&refused, &before);
    assert!(refused.get("state").is_none());
    assert!(refused.get("module").is_none());

    let refused_load = call(&mut rpc, json!({"method": "load_source", "args": {
        "source": "@Type\nReplacement <<= { x }", "name": "Replacement.ali",
        "expect": expect(&old), "page": {"seen": seen(&old)}
    }}));
    assert_refusal(&refused_load, &before);
    assert_eq!(refused_load["state"], before["state"]);
    let after = poll(&mut rpc, json!({}));
    assert_eq!(counters(&after), counters(&before));
    assert_eq!(after["state"], before["state"], "old active session remains");
    assert_eq!(after["module"], before["module"], "old module remains");

    let queries = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "types; help; print", "expect": expect(&old),
        "page": {"seen": seen(&before)}
    }}));
    assert_ok_replies(&queries, 3);
    assert_eq!(counters(&queries), counters(&before));
    for (reply, input) in queries["replies"].as_array().unwrap().iter().zip(["types", "help", "print"]) {
        assert_eq!(reply["input"], input);
        assert!(reply["rendered"]["lines"].as_array().is_some_and(|lines| !lines.is_empty()));
    }
    assert!(queries.get("state").is_none());
    assert!(queries.get("module").is_none());

    stale_save_leaves_valid_disk_target_untouched();
}

// A real disk session and a successful fresh save prove stale save is guarded,
// rather than merely rejected because an editor-owned source cannot be saved.
fn stale_save_leaves_valid_disk_target_untouched() {
    let dir = RpcTempDir::new();
    std::fs::write(dir.0.join("Idem.ali"), IDEM).unwrap();
    let target = dir.0.join("saved.ali");
    let sentinel = b"@Type\nSentinel <<= { untouched }\n";
    std::fs::write(&target, sentinel).unwrap();
    let mut kernel = Kernel::new(FsAccess::Within(CanonicalDir::new(&dir.0).unwrap()));
    let loaded = kernel.open("Idem.ali");
    assert!(loaded.status == Status::Ok, "{:?}", loaded.message);
    let mut rpc = Rpc::new(kernel, INSTANCE.to_owned());
    let old = poll(&mut rpc, json!({}));
    assert_ok_replies(&wire_eval(&mut rpc, "start Idem lhs rhs; apply 0; store diskproof"), 3);
    let before = poll(&mut rpc, json!({}));
    let refused = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "save saved.ali", "expect": expect(&old), "page": {"seen": seen(&old)}
    }}));
    assert_refusal(&refused, &before);
    assert_eq!(refused["state"], before["state"]);
    assert_eq!(refused["module"], before["module"]);
    assert_eq!(std::fs::read(&target).unwrap(), sentinel);
    assert_eq!(std::fs::read_to_string(dir.0.join("Idem.ali")).unwrap(), IDEM);
    let after = poll(&mut rpc, json!({}));
    assert_eq!(counters(&after), counters(&before));
    assert_eq!(after["state"], before["state"]);
    assert_eq!(after["module"], before["module"]);
    let saved = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "save saved.ali", "expect": expect(&before)
    }}));
    assert_ok_replies(&saved, 1);
    assert_eq!(counters(&saved), counters(&before));
    let written = std::fs::read_to_string(&target).unwrap();
    assert_eq!(written, format!("{}\n", before["module"]["source"].as_str().unwrap().trim_end()));
    assert!(written.contains("let diskproof ="));
}

struct RpcTempDir(PathBuf);

impl RpcTempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "alifib-rpc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for RpcTempDir {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

#[test]
fn rpc_rejects_malformed_call() {
    let mut rpc = memory_rpc();
    let before = poll(&mut rpc, json!({}));
    for malformed in [
        "not JSON".to_owned(),
        "{".to_owned(),
        "null".to_owned(),
        "[]".to_owned(),
        json!({"method": "unknown", "args": {}}).to_string(),
        json!({"method": "eval", "args": {}}).to_string(),
        json!({"method": "eval", "args": {"line": 42}}).to_string(),
        json!({"method": "load_source", "args": {"modules": {}}}).to_string(),
        json!({"method": "poll", "args": {}}).to_string(),
        json!({"method": "eval", "args": {"line": "start Idem lhs rhs", "expect": {"instance": INSTANCE, "revision": -1}}}).to_string(),
        json!({"method": "eval", "args": {"line": "start Idem lhs rhs", "expect": {"instance": 1780000000000000001_u64, "revision": 1}}}).to_string(),
        json!({"method": "poll", "args": {"page": {"seen": {"instance": INSTANCE, "revision": 1}}}}).to_string(),
        json!({"method": "poll", "args": {"page": {"draw": [{"view": {"of": "unknown"}}]}}}).to_string(),
        json!({"method": "eval", "args": {"line": "start Idem lhs rhs", "page": {"draw": [{"view": {"of": "preview", "choice": "0"}}]}}}).to_string(),
    ] {
        let answer: Value = serde_json::from_str(&rpc.call(&malformed)).unwrap();
        assert_eq!(answer["status"], "error", "{malformed}: {answer}");
        assert!(answer["message"].as_str().is_some_and(|message| !message.is_empty()));
        assert_eq!(answer.as_object().unwrap().len(), 2, "no partial success: {answer}");
        let after = poll(&mut rpc, json!({}));
        assert_eq!(counters(&after), counters(&before), "{malformed}");
        assert_eq!(after["state"], before["state"], "{malformed}");
        assert_eq!(after["module"], before["module"], "{malformed}");
    }
}

#[test]
fn rpc_instance_is_a_string() {
    assert_eq!(INSTANCE.len(), 19);
    assert!(INSTANCE.bytes().all(|byte| byte.is_ascii_digit()));
    let mut rpc = memory_rpc();
    let first = poll(&mut rpc, json!({}));
    assert_eq!(first["instance"].as_str(), Some(INSTANCE));
    let roundtrip: Value = serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
    assert_eq!(roundtrip["instance"], json!(INSTANCE));
    let unchanged = poll(&mut rpc, json!({"seen": seen(&roundtrip)}));
    assert_eq!(unchanged["instance"], json!(INSTANCE));
    assert!(unchanged.get("state").is_none());
    assert!(unchanged.get("module").is_none());

    // These identities differ only below JavaScript Number precision.
    let rounded = "1780000000000000000";
    assert_eq!(INSTANCE.parse::<f64>().unwrap(), rounded.parse::<f64>().unwrap());
    let mut imprecise_seen = seen(&roundtrip);
    imprecise_seen["instance"] = json!(rounded);
    let refresh = poll(&mut rpc, json!({"seen": imprecise_seen}));
    assert_eq!(refresh["state"], first["state"]);
    assert_eq!(refresh["module"], first["module"]);
    let mut imprecise_expect = expect(&roundtrip);
    imprecise_expect["instance"] = json!(rounded);
    let refused = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "start Idem lhs rhs", "expect": imprecise_expect
    }}));
    assert_refusal(&refused, &first);
    let accepted = call(&mut rpc, json!({"method": "eval", "args": {
        "line": "start Idem lhs rhs", "expect": expect(&roundtrip),
        "page": {"seen": seen(&roundtrip)}
    }}));
    assert_ok_replies(&accepted, 1);
    assert_eq!(accepted["instance"].as_str(), Some(INSTANCE));
    assert_eq!(accepted["state"]["mode"], "rewrite");
    assert_eq!(accepted["revision"].as_u64().unwrap(), first["revision"].as_u64().unwrap() + 1);
    assert!(accepted.get("module").is_none());
}

#[test]
fn reply_roundtrips() {
    let diagnostic = Error::Syntax {
        message: "expected a definition".to_owned(),
        span: Span { start: 6, end: 7 },
    }.to_diagnostic("@Type\n?\n", Some("broken.ali".to_owned()));
    assert!(diagnostic.notes.is_empty());
    let mut reply = Reply::refusal("load broken.ali", 7, "load failed".to_owned());
    reply.diagnostics.push(diagnostic);
    let encoded = serde_json::to_value(&reply).unwrap();
    assert!(encoded["diagnostics"][0].get("notes").is_none(), "empty notes are omitted");
    let decoded: Reply = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded.diagnostics.len(), 1);
    assert!(decoded.diagnostics[0].notes.is_empty(), "missing notes default on deserialize");
    assert_eq!(serde_json::to_value(&decoded).unwrap(), encoded);
    // The reply's optional diagnostic list has the same defaulting contract.
    let empty = serde_json::to_value(Reply::refusal("", 7, "refused".to_owned())).unwrap();
    assert!(empty.get("diagnostics").is_none());
    let decoded: Reply = serde_json::from_value(empty.clone()).unwrap();
    assert!(decoded.diagnostics.is_empty());
    assert_eq!(serde_json::to_value(decoded).unwrap(), empty);
}
