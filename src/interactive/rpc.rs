//! The JSON surface shared by the HTTP server, the WASM build, and `do`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::command::parse_line;
use super::kernel::{Kernel, Reply};
use super::response::{ModuleInfo, State};
use super::view::View;

/// One kernel and its opaque, fixed server-process identity.
pub struct Rpc {
    kernel: Kernel,
    instance: String,
}

#[derive(Deserialize)]
#[serde(tag = "method", content = "args", rename_all = "snake_case")]
enum Call {
    Eval {
        line: String,
        #[serde(default)]
        expect: Option<At>,
        #[serde(default)]
        page: Option<Page>,
    },
    LoadSource {
        source: String,
        #[serde(default)]
        modules: HashMap<String, String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        expect: Option<At>,
        #[serde(default)]
        page: Option<Page>,
    },
    Poll {
        page: Page,
    },
}

#[derive(Deserialize)]
struct At {
    instance: String,
    revision: u64,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    seen: Option<Seen>,
    #[serde(default)]
    draw: Vec<Draw>,
}

#[derive(Deserialize)]
struct Draw {
    view: View,
    #[serde(default)]
    held: bool,
}

#[derive(Deserialize)]
struct Seen {
    instance: String,
    revision: u64,
    module_revision: u64,
}

/// A decoded call's answer, or why the call could not be decoded.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Answer<'a> {
    Ok(&'a Cut),
    Error { message: String },
}

/// One cut of the kernel, taken after the call's own commands.
#[derive(Serialize)]
struct Cut {
    instance: String,
    generation: u64,
    revision: u64,
    module_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    replies: Option<Vec<Reply>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<State>,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<ModuleInfo>,
    /// One place per `draw` entry; `None` where the page already holds it.
    #[serde(skip_serializing_if = "Option::is_none")]
    views: Option<Vec<Option<Drawn>>>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Drawn {
    Ok { data: Value },
    Error { message: String },
}

/// The call `do` sends: one line, with no guard and no page.
#[derive(Serialize)]
#[serde(tag = "method", content = "args", rename_all = "snake_case")]
enum OneShot<'a> {
    Eval { line: &'a str },
}

/// What `do` reads back from an answer to [`eval_call`].
#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum OneShotAnswer {
    Ok { replies: Vec<Reply> },
    Error { message: String },
}

/// Encode an unguarded, pageless `eval` of `line`.
pub fn eval_call(line: &str) -> String {
    serde_json::to_string(&OneShot::Eval { line }).expect("a call serializes")
}

/// The replies of an answer to [`eval_call`], or the error it carries.
pub fn eval_replies(answer: &str) -> Result<Vec<Reply>, String> {
    match serde_json::from_str(answer).map_err(|error| error.to_string())? {
        OneShotAnswer::Ok { replies } => Ok(replies),
        OneShotAnswer::Error { message } => Err(message),
    }
}

impl Rpc {
    /// The instance is an identity, not a number: server timestamps exceed
    /// JavaScript's exact integer range. WASM uses the fixed string `"0"`.
    pub fn new(kernel: Kernel, instance: String) -> Self {
        Self { kernel, instance }
    }

    /// Decode one call, run it, and encode its answer in one cut.
    /// Malformed calls return an error without running anything.
    pub fn call(&mut self, request: &str) -> String {
        let cut = serde_json::from_str::<Call>(request).map(|call| self.run(call));
        let answer = match &cut {
            Ok(cut) => Answer::Ok(cut),
            Err(error) => Answer::Error { message: error.to_string() },
        };
        serde_json::to_string(&answer).expect("an answer serializes")
    }

    fn run(&mut self, call: Call) -> Cut {
        let (replies, page) = match call {
            Call::Eval { line, expect, page } => {
                let line = parse_line(&line);
                let replies = if line.guarded() && self.stale(expect.as_ref()) {
                    vec![self.refusal(&line.text)]
                } else {
                    self.kernel.run(line)
                };
                (Some(replies), page)
            }
            Call::LoadSource { source, modules, name, expect, page } => {
                let reply = if self.stale(expect.as_ref()) {
                    self.refusal("")
                } else {
                    self.kernel.open_text(source, modules, name.as_deref())
                };
                (Some(vec![reply]), page)
            }
            Call::Poll { page } => (None, Some(page)),
        };
        let mut cut = Cut {
            instance: self.instance.clone(),
            generation: self.kernel.generation(),
            revision: self.kernel.revision(),
            module_revision: self.kernel.module_revision(),
            replies,
            state: None,
            module: None,
            views: None,
        };
        if let Some(page) = page {
            self.answer_page(&mut cut, page);
        }
        cut
    }

    fn stale(&self, expect: Option<&At>) -> bool {
        expect
            .is_some_and(|at| at.instance != self.instance || at.revision != self.kernel.revision())
    }

    fn refusal(&self, input: &str) -> Reply {
        Reply::refusal(
            input,
            self.kernel.generation(),
            "not run: the session changed since this page last showed it".to_owned(),
        )
    }

    /// Send what the page does not already hold in this cut.
    fn answer_page(&mut self, cut: &mut Cut, page: Page) {
        let seen = page.seen.filter(|seen| seen.instance == self.instance);
        let state_changed = seen.as_ref().is_none_or(|seen| seen.revision != cut.revision);
        let module_changed =
            seen.as_ref().is_none_or(|seen| seen.module_revision != cut.module_revision);
        if state_changed {
            cut.state = Some(self.kernel.state());
        }
        if module_changed {
            cut.module = self.kernel.module();
        }
        if state_changed || page.draw.iter().any(|draw| !draw.held) {
            cut.views = Some(page.draw.into_iter()
                .map(|draw| (state_changed || !draw.held).then(|| match self.kernel.view(draw.view) {
                    Ok(data) => Drawn::Ok { data },
                    Err(message) => Drawn::Error { message },
                }))
                .collect());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interactive::kernel::FsAccess;
    use serde_json::json;

    const INSTANCE: &str = "1780000000000000001";
    const IDEM: &str = include_str!("../../tests/fixtures/Idem.ali");

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

    fn seen(answer: &Value) -> Value {
        json!({
            "instance": answer["instance"],
            "revision": answer["revision"],
            "module_revision": answer["module_revision"]
        })
    }

    fn memory_rpc() -> Rpc {
        let mut rpc = Rpc::new(
            Kernel::new(FsAccess::Denied("wire tests have no disk")),
            INSTANCE.to_owned(),
        );
        let loaded = call(
            &mut rpc,
            json!({"method": "load_source", "args": {
                "source": IDEM, "name": "Idem.ali", "page": {}
            }}),
        );
        assert_ok_replies(&loaded, 1);
        assert_kernel_cut(&loaded, &rpc.kernel, true, true);
        rpc
    }

    fn assert_kernel_cut(answer: &Value, kernel: &Kernel, has_state: bool, has_module: bool) {
        assert_eq!(answer["instance"], INSTANCE);
        assert_eq!(answer["revision"], kernel.revision());
        assert_eq!(answer["generation"], kernel.generation());
        assert_eq!(answer["module_revision"], kernel.module_revision());
        assert_eq!(answer.get("state").is_some(), has_state, "{answer}");
        assert_eq!(answer.get("module").is_some(), has_module, "{answer}");
        if has_state {
            assert_eq!(
                answer["state"],
                serde_json::to_value(kernel.state()).unwrap()
            );
        }
        if has_module {
            assert_eq!(
                answer["module"],
                serde_json::to_value(kernel.module().unwrap()).unwrap()
            );
        }
    }

    #[test]
    fn rpc_answer_is_one_cut() {
        let mut rpc = memory_rpc();
        let initial = poll(&mut rpc, json!({}));
        assert_kernel_cut(&initial, &rpc.kernel, true, true);
        assert!(initial.get("replies").is_none());
        assert_eq!(
            initial["views"],
            json!([]),
            "changed state answers even an empty draw"
        );
        let unchanged = poll(&mut rpc, json!({"seen": seen(&initial)}));
        assert_kernel_cut(&unchanged, &rpc.kernel, false, false);
        assert!(
            unchanged.get("views").is_none(),
            "unchanged empty draw needs no views"
        );

        let started = call(
            &mut rpc,
            json!({"method": "eval", "args": {
                "line": "start Idem lhs rhs", "page": {"seen": seen(&initial)}
            }}),
        );
        assert_ok_replies(&started, 1);
        assert_kernel_cut(&started, &rpc.kernel, true, false);
        assert!(started["revision"].as_u64().unwrap() > initial["revision"].as_u64().unwrap());
        assert_eq!(started["module_revision"], initial["module_revision"]);

        let applied = call(
            &mut rpc,
            json!({"method": "eval", "args": {
                "line": "apply 0", "page": {"seen": seen(&started)}
            }}),
        );
        assert_ok_replies(&applied, 1);
        assert_kernel_cut(&applied, &rpc.kernel, true, false);
        assert_eq!(applied["state"]["rewrite"]["step_count"], 1);
        assert_ne!(applied["state"], started["state"]);

        let stored = call(
            &mut rpc,
            json!({"method": "eval", "args": {
                "line": "store saved", "page": {"seen": seen(&applied)}
            }}),
        );
        assert_ok_replies(&stored, 1);
        assert_kernel_cut(&stored, &rpc.kernel, true, true);
        assert_eq!(stored["replies"][0]["edited"], true);
        assert_eq!(stored["module_revision"], stored["revision"]);
        assert!(
            stored["module_revision"].as_u64().unwrap()
                > applied["module_revision"].as_u64().unwrap()
        );
        assert!(
            stored["module"]["source"]
                .as_str()
                .unwrap()
                .contains("let saved =")
        );

        // Each clock independently controls its snapshot, not the other clock.
        let mut old_module = seen(&stored);
        old_module["module_revision"] = initial["module_revision"].clone();
        assert_kernel_cut(
            &poll(&mut rpc, json!({"seen": old_module})),
            &rpc.kernel,
            false,
            true,
        );
        let mut old_state = seen(&stored);
        old_state["revision"] = initial["revision"].clone();
        assert_kernel_cut(
            &poll(&mut rpc, json!({"seen": old_state})),
            &rpc.kernel,
            true,
            false,
        );
        assert_kernel_cut(
            &poll(&mut rpc, json!({"seen": seen(&stored)})),
            &rpc.kernel,
            false,
            false,
        );
        assert_kernel_cut(&poll(&mut rpc, json!({})), &rpc.kernel, true, true);
        let mut other_instance = seen(&stored);
        other_instance["instance"] = json!("another instance");
        assert_kernel_cut(
            &poll(&mut rpc, json!({"seen": other_instance})),
            &rpc.kernel,
            true,
            true,
        );
    }

    #[test]
    fn rpc_answer_draws_views() {
        let mut rpc = memory_rpc();
        let current = json!({"of": "current"});
        let module = rpc.kernel.module().unwrap().root;
        let lhs = json!({"of": "item", "module": module, "type_name": "Idem", "item": "lhs"});
        let rhs = json!({"of": "item", "module": module, "type_name": "Idem", "item": "rhs"});
        let idle = poll(&mut rpc, json!({"draw": [{"view": current}]}));
        assert_kernel_cut(&idle, &rpc.kernel, true, true);
        assert_eq!(idle["views"][0]["status"], "error");
        assert_eq!(
            idle["views"][0]["message"],
            rpc.kernel.view(View::Current).unwrap_err()
        );

        let started = wire_eval(&mut rpc, "start Idem lhs rhs");
        assert_ok_replies(&started, 1);
        assert_kernel_cut(&started, &rpc.kernel, false, false);
        let initial = poll(
            &mut rpc,
            json!({"draw": [
                {"view": lhs, "held": true}, {"view": current}, {"view": rhs, "held": true}
            ]}),
        );
        assert_kernel_cut(&initial, &rpc.kernel, true, true);
        let views = initial["views"].as_array().unwrap();
        assert_eq!(views.len(), 3);
        for (index, view) in [lhs.clone(), current.clone(), rhs.clone()]
            .into_iter()
            .enumerate()
        {
            assert_eq!(views[index]["status"], "ok");
            assert_eq!(
                views[index]["data"],
                rpc.kernel
                    .view(serde_json::from_value(view).unwrap())
                    .unwrap()
            );
        }

        let unchanged = poll(
            &mut rpc,
            json!({"seen": seen(&initial), "draw": [
                {"view": lhs, "held": true}, {"view": current}, {"view": rhs},
                {"view": {"of": "preview", "choice": 0}, "held": true}
            ]}),
        );
        assert_kernel_cut(&unchanged, &rpc.kernel, false, false);
        let views = unchanged["views"].as_array().unwrap();
        assert_eq!(views.len(), 4);
        assert!(views[0].is_null());
        assert_eq!(views[1]["status"], "ok");
        assert_eq!(views[1]["data"], rpc.kernel.view(View::Current).unwrap());
        assert_eq!(views[2]["status"], "ok");
        assert_eq!(
            views[2]["data"],
            rpc.kernel
                .view(serde_json::from_value(rhs.clone()).unwrap())
                .unwrap()
        );
        assert!(views[3].is_null());
        let all_held = poll(
            &mut rpc,
            json!({"seen": seen(&initial), "draw": [
                {"view": current, "held": true}, {"view": lhs, "held": true}
            ]}),
        );
        assert!(all_held.get("views").is_none());
        assert_kernel_cut(&all_held, &rpc.kernel, false, false);

        let before_view = rpc.kernel.view(View::Current).unwrap();
        let applied = call(
            &mut rpc,
            json!({"method": "eval", "args": {
                "line": "apply 0", "page": {"seen": seen(&initial), "draw": [
                    {"view": current, "held": true}, {"view": rhs, "held": true}, {"view": lhs}
                ]}
            }}),
        );
        assert_ok_replies(&applied, 1);
        assert_kernel_cut(&applied, &rpc.kernel, true, false);
        let views = applied["views"].as_array().unwrap();
        assert_eq!(views.len(), 3);
        for (index, view) in [current.clone(), rhs.clone(), lhs.clone()]
            .into_iter()
            .enumerate()
        {
            assert_eq!(views[index]["status"], "ok");
            assert_eq!(
                views[index]["data"],
                rpc.kernel
                    .view(serde_json::from_value(view).unwrap())
                    .unwrap()
            );
        }
        assert_ne!(views[0]["data"], before_view, "current must be post-apply");

        // Hold a successful item view, then replace its module: a held old item
        // must be answered with an error, not silently retained or dropped.
        let gone = call(
            &mut rpc,
            json!({"method": "load_source", "args": {
                "source": "@Type\nOther <<= { x }", "name": "Other.ali",
                "page": {"seen": seen(&applied), "draw": [
                    {"view": lhs, "held": true}, {"view": current, "held": true}
                ]}
            }}),
        );
        assert_ok_replies(&gone, 1);
        assert_kernel_cut(&gone, &rpc.kernel, true, true);
        assert_eq!(gone["state"]["mode"], "idle");
        assert_eq!(gone["module"]["root"], "Other.ali");
        let views = gone["views"].as_array().unwrap();
        assert_eq!(views.len(), 2);
        for (index, view) in [lhs.clone(), current.clone()].into_iter().enumerate() {
            assert_eq!(views[index]["status"], "error");
            assert_eq!(
                views[index]["message"],
                rpc.kernel
                    .view(serde_json::from_value(view).unwrap())
                    .unwrap_err()
            );
            assert!(views[index].get("data").is_none());
        }
        let held_errors = poll(
            &mut rpc,
            json!({"seen": seen(&gone), "draw": [
                {"view": lhs, "held": true}, {"view": current, "held": true}
            ]}),
        );
        assert_kernel_cut(&held_errors, &rpc.kernel, false, false);
        assert!(held_errors.get("views").is_none());
        assert!(held_errors.get("state").is_none());
    }
}
