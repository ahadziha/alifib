//! Retained transcript and hole-filling tests for the shared kernel.
//!
//! The former CLI/web parity test is structural now: there is only one
//! kernel, evaluator, and rendering path, so comparing adapters is redundant.

use std::path::{Path, PathBuf};

use alifib::interactive::command::{Command, Input, parse_line};
use alifib::interactive::kernel::{CanonicalDir, FsAccess, Kernel, Reply, Status};
use alifib::interactive::view::View;
use serde_json::Value;

fn fixture(name: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn loaded(name: &str) -> Kernel {
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    let reply = kernel.open(&fixture(name));
    assert!(reply.status == Status::Ok, "{:?}", reply.message);
    kernel
}

fn eval(kernel: &mut Kernel, line: &str) -> Reply {
    let mut replies = kernel.eval(line);
    assert_eq!(replies.len(), 1, "{line}");
    replies.pop().unwrap()
}

fn ok(kernel: &mut Kernel, line: &str) -> Reply {
    let reply = eval(kernel, line);
    assert!(reply.status == Status::Ok, "{line}: {:?}", reply.message);
    reply
}

fn rendered(reply: &Reply) -> String {
    reply.rendered.as_ref().expect("rendered reply").to_string()
}

fn state(kernel: &Kernel) -> Value {
    serde_json::to_value(kernel.state()).unwrap()
}

#[test]
fn kernel_transcript_matches_golden() {
    let mut kernel = loaded("Idem.ali");
    let reply = ok(&mut kernel, "start Idem lhs rhs");
    assert_eq!(
        rendered(&reply),
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
    let reply = ok(&mut kernel, "proof");
    assert_eq!(
        rendered(&reply),
        "proof : (id #0 id #0 id) → (id #0 id #0 id)\n  (id #0 id #0 id)"
    );

    // apply 0
    let reply = ok(&mut kernel, "apply 0");
    assert_eq!(
        rendered(&reply),
        "step: 1\n\
         current: (id #0 id)\n\
         target: id\n\
         \n\
         available rewrites:\n\
         \x20 [0] idem  (id #0 id) → id\n\
         \x20     match: [idem]"
    );

    // rules
    let reply = ok(&mut kernel, "rules");
    assert_eq!(rendered(&reply), "  idem  (id #0 id) → id");

    // proof — the re-parseable expression `store` would persist, with boundary
    let reply = ok(&mut kernel, "proof");
    assert_eq!(
        rendered(&reply),
        "proof : (id #0 id #0 id) → (id #0 id)\n  (idem #0 id)"
    );

    // store p
    let reply = ok(&mut kernel, "store p");
    assert_eq!(rendered(&reply), "Stored 'p'\n  let p = (idem #0 id)");

    // history
    let reply = ok(&mut kernel, "history");
    assert_eq!(rendered(&reply), "  1. idem [choice 0]");

    // types uses the shared transcript layout.
    let reply = ok(&mut kernel, "types");
    assert_eq!(rendered(&reply), "  Idem (dim 2, 3 generators, 6 diagrams, 1 map)");

    // type Idem — generators by dimension, diagrams with `= expr` (incl. the
    // just-stored `p`), maps.
    let reply = ok(&mut kernel, "type Idem");
    assert_eq!(
        rendered(&reply),
        "generators:\n  [0]\n    ob\n  [1]\n    id : ob → ob\n  [2]\n    idem : (id #0 id) → id\n\
         diagrams:\n  id : ob → ob\n    = id\n  idem : (id #0 id) → id\n    = idem\n  \
         lhs : ob → ob\n    = (id #0 id #0 id)\n  ob\n    = ob\n  \
         p : (id #0 id #0 id) → (id #0 id)\n    = (idem #0 id)\n  rhs : ob → ob\n    = id\n\
         maps:\n  Idem :: Idem"
    );

    // stop  (message-only command: canonical, capital-first, no period)
    let reply = ok(&mut kernel, "stop");
    assert_eq!(reply.message.as_deref(), Some("Session stopped"));

    // holes
    let reply = ok(&mut kernel, "holes");
    assert_eq!(rendered(&reply), "(no open holes)");
}

/// Help needs no loaded source and lists the universal command surface.
#[test]
fn help_lists_every_command() {
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    let help = ok(&mut kernel, "help");
    let text = rendered(&help);
    for command in ["load", "print", "save", "holes", "clear"] {
        assert!(text.contains(command), "help lists {command}");
    }
    assert!(text.contains("load [<path>]"));
    assert!(text.contains("quote a path with spaces"));
    assert!(text.contains("start <t> <s> [<g>]"));
    assert!(text.contains("group a multi-word diagram in ( )"));
    assert!(text.contains("Start a rewrite session (target optional)"));
    assert!(text.contains("resume <t> <p> [<g>]"));
    assert!(text.contains("resume <t> <p> [<g>] Resume a session from a diagram (target optional)"));
    assert!(!text.contains("[<g>]Resume"));
    assert!(text.ends_with("Front ends: quit / exit / q (terminal) · clear, ↑/↓ history, Ctrl+Enter evaluate (browser)"));
}

/// Constraints retain their type/map keys and equations in the module snapshot.
#[test]
fn kernel_holes_carries_constraints() {
    let mut kernel = loaded("Constraint.ali");
    ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    let cons = &module.constraints;
    assert_eq!(cons.len(), 1);
    assert_eq!((cons[0].type_name.as_str(), cons[0].map_name.as_str()), ("D", "F"));
    assert_eq!(cons[0].equations, ["?f #0 ?g = a #0 a"]);
}

/// Unknown commands carry the shared parser's wording, not an action protocol.
#[test]
fn kernel_unknown_command_uses_parser_wording() {
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    let reply = eval(&mut kernel, "frobnicate");
    assert!(reply.status == Status::Error);
    assert_eq!(reply.message.as_deref(), Some("Unrecognised command 'frobnicate' — type 'help' for a list"));
}

/// A 0-cell fill has the same proof/history/rules and stop messaging as a session.
#[test]
fn kernel_zero_cell_fill_behaves_like_a_session() {
    let mut kernel = loaded("LayeredHole.ali");
    ok(&mut kernel, "fill 0");
    assert_eq!(state(&kernel)["hole"]["dim"], 0);
    assert_eq!(rendered(&ok(&mut kernel, "proof")), "(no proof yet)");
    assert_eq!(rendered(&ok(&mut kernel, "history")), "(no moves yet)");
    assert_eq!(rendered(&ok(&mut kernel, "rules")), "(no rules)");

    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["zero_cell"]["target_reached"], true);
    assert_eq!(rendered(&ok(&mut kernel, "proof")), "proof :\n  x");
    assert_eq!(rendered(&ok(&mut kernel, "history")), "  1. x [choice 0]");
    assert_eq!(ok(&mut kernel, "stop").message.as_deref(), Some("Session stopped"));
}

#[test]
fn kernel_show_when_idle_reports_module() {
    let mut kernel = loaded("RewriteFill.ali");
    let shown = ok(&mut kernel, "show");
    assert!(rendered(&shown).contains("module:"), "idle show reports the module");
}

/// A 1-cell hole is filled through a rewrite; done updates the running source.
#[test]
fn kernel_fill_one_dim_hole() {
    let mut kernel = loaded("RewriteFill.ali");
    ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    assert_eq!(module.holes.len(), 1);
    assert_eq!(module.holes[0].source_name, "x");
    assert_eq!(module.holes[0].index, 0);

    ok(&mut kernel, "fill 0");
    assert_eq!(state(&kernel)["hole"]["source_name"], "x");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["rewrite"]["target_reached"], true);

    let done = ok(&mut kernel, "done");
    assert!(done.message.as_deref().unwrap().starts_with("Filled ?x with "),
        "message: {:?}", done.message);
    let new_source = kernel.module().unwrap().source;
    assert!(new_source.contains("x =>"), "the clause is appended: {new_source}");
    ok(&mut kernel, "holes");
    assert!(kernel.module().unwrap().holes.is_empty(), "no holes left");
}

#[test]
fn kernel_fill_respects_backward() {
    let mut fwd = loaded("RewriteFill.ali");
    ok(&mut fwd, "fill 0");
    let f = state(&fwd);
    assert_eq!(f["by"]["rewrite"]["current"]["label"], "p");
    assert_eq!(f["by"]["rewrite"]["target"]["label"], "q");

    let mut bwd = loaded("RewriteFill.ali");
    ok(&mut bwd, "backward on");
    ok(&mut bwd, "fill 0");
    let b = state(&bwd);
    assert_eq!(b["by"]["rewrite"]["current"]["label"], "q", "backward swaps initial");
    assert_eq!(b["by"]["rewrite"]["target"]["label"], "p", "backward swaps target");
}

/// Premature done errors but leaves the session available to finish.
#[test]
fn kernel_premature_done_keeps_session() {
    let mut kernel = loaded("RewriteFill.ali");
    ok(&mut kernel, "fill 0");
    assert!(eval(&mut kernel, "done").status == Status::Error, "no proof yet");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["rewrite"]["target_reached"], true);
    ok(&mut kernel, "done");
    assert!(kernel.module().unwrap().source.contains("x =>"));
}

/// The dependent e is blocked by a1; the boundaryless chooser supports undo/redo.
#[test]
fn kernel_fill_zero_cell_session() {
    let mut kernel = loaded("LayeredHole.ali");
    ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    let list = &module.holes;
    assert_eq!(list.len(), 2);
    let a1 = list.iter().find(|h| h.source_name == "a1").unwrap();
    let e = list.iter().find(|h| h.source_name == "e").unwrap();
    assert_eq!(a1.dim, 0);
    assert!(eval(&mut kernel, &format!("fill {}", e.index)).status == Status::Error);

    ok(&mut kernel, &format!("fill {}", a1.index));
    let started = state(&kernel);
    assert!(started["by"].get("rewrite").is_none(), "no step diagram for a 0-cell fill");
    assert!(started["by"]["zero_cell"].get("current").is_none());
    let choices = started["by"]["zero_cell"]["choices"].as_array().unwrap();
    let x = choices.iter().find(|c| c["name"] == "x").unwrap();

    ok(&mut kernel, "show");
    assert_eq!(state(&kernel)["by"]["zero_cell"]["choices"].as_array().unwrap().len(), choices.len());
    ok(&mut kernel, &format!("apply {}", x["index"]));
    let chosen = state(&kernel);
    assert_eq!(chosen["by"]["zero_cell"]["chosen"], "x");
    assert_eq!(chosen["by"]["zero_cell"]["target_reached"], true);
    assert!(chosen["by"]["zero_cell"]["choices"].as_array().unwrap().is_empty());

    ok(&mut kernel, "undo");
    let undone = state(&kernel);
    assert_eq!(undone["by"]["zero_cell"]["target_reached"], false);
    assert!(!undone["by"]["zero_cell"]["choices"].as_array().unwrap().is_empty());
    assert_eq!(undone["by"]["zero_cell"]["can_redo"], true);
    ok(&mut kernel, "redo");
    assert_eq!(state(&kernel)["by"]["zero_cell"]["chosen"], "x");

    let done = ok(&mut kernel, "done");
    assert_eq!(done.message.as_deref(), Some("Filled ?a1 with x"));
    assert!(kernel.module().unwrap().source.contains("a1 => x"));
    ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    assert_eq!(module.holes.len(), 1);
    assert_eq!(module.holes[0].source_name, "e");
}

fn parsed_inputs(line: &str) -> Vec<Input> {
    parse_line(line)
        .parts
        .expect("balanced line")
        .into_iter()
        .map(|part| part.input.expect("valid command"))
        .collect()
}

fn clocks(kernel: &Kernel) -> (u64, u64, u64) {
    (
        kernel.generation(),
        kernel.module_revision(),
        kernel.revision(),
    )
}

fn assert_stamp_order(kernel: &Kernel, reply: &Reply) {
    let (generation, module_revision, revision) = clocks(kernel);
    assert_eq!(reply.generation, generation);
    assert!(generation <= module_revision && module_revision <= revision);
}

#[test]
fn chain_stops_at_first_error() {
    let mut kernel = loaded("Idem.ali");
    let revision = kernel.revision();
    let replies = kernel.eval("start Idem lhs rhs; apply 99; apply 0; stop");
    assert_eq!(replies.len(), 2);
    assert!(replies[0].status == Status::Ok);
    assert!(replies[1].status == Status::Error);
    assert_eq!(replies[1].input, "apply 99");
    assert_eq!(kernel.revision(), revision + 2);
    assert_eq!(state(&kernel)["mode"], "rewrite");
    assert_eq!(state(&kernel)["rewrite"]["step_count"], 0);

    let revision = kernel.revision();
    let replies = kernel.eval("show; unknown; stop");
    assert_eq!(replies.len(), 2);
    assert!(replies[0].status == Status::Ok);
    assert!(replies[1].status == Status::Error);
    assert_eq!(kernel.revision(), revision);
    assert_eq!(state(&kernel)["mode"], "rewrite");
}

#[test]
fn chain_splits_at_top_level() {
    assert_eq!(
        parsed_inputs(
            "; ; start Assoc \"a;a\"; ; start Assoc (a; a); start Assoc [a; a]; start Assoc {a; a}; ;"
        ),
        ["\"a;a\"", "(a; a)", "[a; a]", "{a; a}"].map(|initial| {
            Input::Session(Command::Start {
                type_name: "Assoc".into(),
                initial: initial.into(),
                target: None,
            })
        })
    );
    let mut kernel = loaded("Idem.ali");
    let revision = kernel.revision();
    let replies = kernel.eval("; ; start Idem (id #0 id) id; ; apply 0; ; show; ;");
    assert_eq!(replies.len(), 3);
    assert!(replies.iter().all(|r| r.status == Status::Ok));
    assert_eq!(kernel.revision(), revision + 2);
    assert_eq!(state(&kernel)["rewrite"]["target_reached"], true);
    assert!(kernel.eval(" ; ; ").is_empty());
}

#[test]
fn unbalanced_line_runs_nothing() {
    let mut kernel = loaded("Idem.ali");
    let before = state(&kernel);
    let before_clocks = clocks(&kernel);
    for line in [
        "show; start T (a; apply 0",
        "start Idem lhs; show \"unterminated",
        "start Idem lhs; start T (a]",
        "start Idem lhs; show ]",
        "start Idem lhs; (* unterminated",
    ] {
        let reply = eval(&mut kernel, line);
        assert!(reply.status == Status::Error, "{line}");
        assert!(
            reply
                .message
                .as_deref()
                .unwrap()
                .starts_with("Unbalanced line:")
        );
        assert_eq!(state(&kernel), before, "{line}");
        assert_eq!(clocks(&kernel), before_clocks, "{line}");
    }
}

#[test]
fn words_keep_literals_and_brackets() {
    assert_eq!(
        parsed_inputs("start T (a #0 b) c"),
        vec![Input::Session(Command::Start {
            type_name: "T".into(),
            initial: "(a #0 b)".into(),
            target: Some("c".into()),
        }),]
    );
    for literal in ["\"aaa\"", "'aaa'"] {
        assert_eq!(
            parsed_inputs(&format!("start Assoc {literal}")),
            vec![Input::Session(Command::Start {
                type_name: "Assoc".into(),
                initial: literal.into(),
                target: None,
            }),]
        );
        let mut kernel = loaded("Assoc.ali");
        ok(&mut kernel, &format!("start Assoc {literal}"));
        let literal_diagram = state(&kernel)["rewrite"]["current"].clone();
        ok(&mut kernel, "stop");
        ok(&mut kernel, "start Assoc (a #0 a #0 a)");
        assert_eq!(state(&kernel)["rewrite"]["current"], literal_diagram);
        assert_eq!(literal_diagram["cell_count"], 3);
    }
}

#[test]
fn names_are_taken_as_written() {
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/Delta.ali");
    let reply = kernel.open(path.to_str().unwrap());
    assert!(reply.status == Status::Ok, "{:?}", reply.message);
    let reply = ok(&mut kernel, "type 1Simplex");
    assert!(rendered(&reply).contains("t : 0.t → 1.t"));
    ok(&mut kernel, "start 2Simplex 02.t (01.t #0 12.t)");
    let rewrite = &state(&kernel)["rewrite"];
    assert_eq!(rewrite["current"]["label"], "02.t");
    assert_eq!(rewrite["rewrites"][0]["rule_name"], "t");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["rewrite"]["target_reached"], true);
}

#[test]
fn comments_are_lexed() {
    let diagram = "(a (* it's a note; [ \" (* nested *) *) #0 a)";
    assert_eq!(
        parsed_inputs(&format!("start Assoc {diagram}")),
        vec![Input::Session(Command::Start {
            type_name: "Assoc".into(),
            initial: diagram.into(),
            target: None,
        }),]
    );
    let mut kernel = loaded("Assoc.ali");
    ok(&mut kernel, &format!("start Assoc {diagram}"));
    let commented = state(&kernel)["rewrite"]["current"].clone();
    ok(&mut kernel, "stop");
    ok(&mut kernel, "start Assoc (a #0 a)");
    assert_eq!(state(&kernel)["rewrite"]["current"], commented);
    ok(&mut kernel, "stop");
    ok(&mut kernel, "start Assoc(* note; *)a");
    let commented = state(&kernel)["rewrite"].clone();
    ok(&mut kernel, "stop");
    ok(&mut kernel, "start Assoc a");
    assert_eq!(state(&kernel)["rewrite"], commented);
    let before_clocks = clocks(&kernel);
    let before = state(&kernel);
    let reply = eval(&mut kernel, "stop; (* unterminated");
    assert!(reply.status == Status::Error);
    assert_eq!(
        reply.message.as_deref(),
        Some("Unbalanced line: unterminated comment")
    );
    assert_eq!(state(&kernel), before);
    assert_eq!(clocks(&kernel), before_clocks);
}

#[test]
fn path_literals_decode() {
    // Only this path-grammar test writes a temporary copy, never a disk fixture.
    let dir = std::env::temp_dir().join(format!(
        "alifib path literals {} {}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("F.ali");
    std::fs::copy(fixture("Idem.ali"), &path).unwrap();
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    for delimiter in ['"', '\''] {
        // The language decodes an otherwise unrecognised escape to its character.
        let escaped = path.to_str().unwrap().replace("F.ali", "\\F.ali");
        ok(
            &mut kernel,
            &format!("load {delimiter}{escaped}{delimiter}"),
        );
        assert_eq!(state(&kernel)["mode"], "idle");
        assert_eq!(
            kernel.module().unwrap().source,
            std::fs::read_to_string(fixture("Idem.ali")).unwrap()
        );
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn tab_separated_args_parse() {
    let mut kernel = loaded("Idem.ali");
    ok(&mut kernel, "start\tIdem\t(id #0 id)\tid");
    ok(&mut kernel, "apply\t0");
    assert_eq!(state(&kernel)["rewrite"]["target_reached"], true);
}

#[test]
fn store_name_must_be_a_name() {
    let mut kernel = loaded("Idem.ali");
    ok(&mut kernel, "start Idem lhs rhs");
    ok(&mut kernel, "apply 0");
    let before = state(&kernel);
    let before_clocks = clocks(&kernel);
    let source = kernel.module().unwrap().source;
    for line in ["store map", "store (p)", "store \"p\""] {
        let reply = eval(&mut kernel, line);
        assert!(reply.status == Status::Error);
        assert!(
            reply
                .message
                .as_deref()
                .unwrap()
                .starts_with("Usage: store <name>")
        );
        assert!(!reply.edited);
        assert_eq!(state(&kernel), before);
        assert_eq!(clocks(&kernel), before_clocks);
        assert_eq!(kernel.module().unwrap().source, source);
    }
    let reply = ok(&mut kernel, "store 2p");
    assert!(reply.edited);
    let source = kernel.module().unwrap().source;
    assert!(source.contains("let 2p = (idem #0 id)"));
    let mut reloaded = Kernel::new(FsAccess::Unrestricted);
    let reply = reloaded.open_text(source, Default::default(), Some("Stored"));
    assert!(reply.status == Status::Ok, "{:?}", reply.message);
    assert!(rendered(&ok(&mut reloaded, "type Idem")).contains("2p :"));
    ok(&mut reloaded, "resume Idem 2p rhs");
    assert_eq!(
        state(&reloaded)["rewrite"]["current"],
        before["rewrite"]["current"]
    );
}

fn assert_queries_preserve_state(kernel: &mut Kernel, type_name: &str) {
    let before = state(kernel);
    let before_clocks = clocks(kernel);
    let module = serde_json::to_value(kernel.module()).unwrap();
    for query in [
        "holes".to_owned(),
        "types".to_owned(),
        format!("type {type_name}"),
        format!("homology {type_name}"),
        "rules".to_owned(),
        "history".to_owned(),
        "proof".to_owned(),
        "print".to_owned(),
        "show".to_owned(),
        "backward".to_owned(),
        "parallel".to_owned(),
        "help".to_owned(),
    ] {
        let reply = eval(kernel, &query);
        let unavailable = (before["mode"] == "idle"
            && matches!(query.as_str(), "rules" | "history" | "proof"))
            || (query == "parallel"
                && (before["mode"] == "idle" || before["by"].get("zero_cell").is_some()));
        if unavailable {
            assert!(reply.status == Status::Error, "{query}");
            assert_eq!(
                reply.message.as_deref(),
                Some("No active session — use 'start' or 'fill'")
            );
        } else {
            assert!(reply.status == Status::Ok, "{query}: {:?}", reply.message);
        }
        assert!(!reply.edited, "{query}");
        assert_eq!(state(kernel), before, "{query}");
        assert_eq!(clocks(kernel), before_clocks, "{query}");
        assert_eq!(
            serde_json::to_value(kernel.module()).unwrap(),
            module,
            "{query}"
        );
    }
}

#[test]
fn state_mirrors_activity() {
    let mut kernel = loaded("Idem.ali");
    let idle = state(&kernel);
    assert_eq!(idle["mode"], "idle");
    assert_eq!(
        idle["module"],
        std::fs::canonicalize(fixture("Idem.ali"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(idle["backward"], false);
    assert_queries_preserve_state(&mut kernel, "Idem");
    ok(&mut kernel, "start Idem lhs rhs");
    let rewrite = state(&kernel);
    assert_eq!(rewrite["mode"], "rewrite");
    assert_eq!(rewrite["rewrite"]["step_count"], 0);
    assert_eq!(rewrite["rewrite"]["current"], rewrite["rewrite"]["initial"]);
    assert_eq!(rewrite["rewrite"]["current"]["cell_count"], 3);
    assert_eq!(rewrite["rewrite"]["target"]["cell_count"], 1);
    assert_eq!(rewrite["rewrite"]["rewrites"].as_array().unwrap().len(), 2);
    assert_queries_preserve_state(&mut kernel, "Idem");
    ok(&mut kernel, "apply 0");
    assert_queries_preserve_state(&mut kernel, "Idem");

    let mut kernel = loaded("RewriteFill.ali");
    ok(&mut kernel, "fill 0");
    let fill = state(&kernel);
    assert_eq!(fill["mode"], "fill");
    assert_eq!(fill["hole"]["source_name"], "x");
    assert_eq!(fill["hole"]["dim"], 1);
    assert_eq!(fill["by"]["rewrite"]["current"]["label"], "p");
    assert_eq!(fill["by"]["rewrite"]["target"]["label"], "q");
    assert_eq!(fill["by"]["rewrite"]["target_reached"], false);
    assert_queries_preserve_state(&mut kernel, "T");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["rewrite"]["target_reached"], true);
    assert_queries_preserve_state(&mut kernel, "T");

    let mut kernel = loaded("LayeredHole.ali");
    ok(&mut kernel, "fill 0");
    let fill = state(&kernel);
    assert_eq!(fill["mode"], "fill");
    assert_eq!(fill["hole"]["source_name"], "a1");
    assert_eq!(fill["hole"]["dim"], 0);
    assert_eq!(fill["by"]["zero_cell"]["target_reached"], false);
    assert!(
        !fill["by"]["zero_cell"]["choices"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_queries_preserve_state(&mut kernel, "A");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["zero_cell"]["chosen"], "x");
    assert_eq!(state(&kernel)["by"]["zero_cell"]["target_reached"], true);
    assert_queries_preserve_state(&mut kernel, "A");
}

#[test]
fn backward_survives_queries() {
    let mut kernel = loaded("Idem.ali");
    let replies = kernel.eval("backward on; holes; types");
    assert_eq!(replies.len(), 3);
    assert!(replies.iter().all(|r| r.status == Status::Ok));
    assert_eq!(state(&kernel)["backward"], true);
    assert_queries_preserve_state(&mut kernel, "Idem");
    ok(&mut kernel, "start Idem rhs lhs");
    assert_queries_preserve_state(&mut kernel, "Idem");
    assert_eq!(state(&kernel)["backward"], true);
}

#[test]
fn print_is_not_an_edit() {
    let mut kernel = loaded("Idem.ali");
    let source = kernel.module().unwrap().source;
    let before_clocks = clocks(&kernel);
    let reply = ok(&mut kernel, "print");
    assert!(!reply.edited);
    assert_eq!(rendered(&reply), source);
    assert_eq!(clocks(&kernel), before_clocks);
    ok(&mut kernel, "start Idem lhs rhs");
    ok(&mut kernel, "apply 0");
    let reply = ok(&mut kernel, "store p");
    assert!(reply.edited);
    assert_eq!(kernel.module_revision(), kernel.revision());
    assert!(kernel.module_revision() > before_clocks.1);
    let stored = kernel.module().unwrap().source;
    assert_ne!(stored, source);
    assert!(stored.contains("let p = (idem #0 id)"));
    let before_clocks = clocks(&kernel);
    let reply = ok(&mut kernel, "print");
    assert!(!reply.edited);
    assert_eq!(rendered(&reply), stored);
    assert_eq!(clocks(&kernel), before_clocks);

    let mut kernel = loaded("RewriteFill.ali");
    let source = kernel.module().unwrap().source;
    let module_revision = kernel.module_revision();
    ok(&mut kernel, "fill 0");
    ok(&mut kernel, "apply 0");
    let reply = ok(&mut kernel, "done");
    assert!(reply.edited);
    assert_eq!(kernel.module_revision(), kernel.revision());
    assert!(kernel.module_revision() > module_revision);
    let filled = kernel.module().unwrap().source;
    assert_ne!(filled, source);
    assert!(filled.contains("x => r"));
    let before_clocks = clocks(&kernel);
    let reply = ok(&mut kernel, "print");
    assert!(!reply.edited);
    assert_eq!(rendered(&reply), filled);
    assert_eq!(clocks(&kernel), before_clocks);
}

#[test]
fn revision_counts_mutations() {
    let mut kernel = loaded("Idem.ali");
    assert_queries_preserve_state(&mut kernel, "Idem");
    for (line, succeeds) in [
        ("apply 0", false),
        ("start Idem lhs rhs", true),
        ("apply 99", false),
        ("apply 0", true),
        ("undo", true),
        ("redo", true),
        ("backward on", true),
        ("stop", true),
        ("fill 99", false),
    ] {
        let before = kernel.revision();
        let reply = eval(&mut kernel, line);
        assert_eq!(
            reply.status == Status::Ok,
            succeeds,
            "{line}: {:?}",
            reply.message
        );
        assert_eq!(kernel.revision(), before + 1, "{line}");
        assert_stamp_order(&kernel, &reply);
    }
    for line in [
        "unknown",
        "apply nope",
        "store map",
        "load 'missing directory/F.ali'",
    ] {
        let before = clocks(&kernel);
        assert!(eval(&mut kernel, line).status == Status::Error);
        assert_eq!(clocks(&kernel), before, "{line}");
    }
    let before = kernel.revision();
    let reply = kernel.open_text("@Type T <<= { x }".into(), Default::default(), Some("T"));
    assert!(reply.status == Status::Ok, "{:?}", reply.message);
    assert_eq!(kernel.revision(), before + 1);
    let before = clocks(&kernel);
    assert!(eval(&mut kernel, "load").status == Status::Error);
    assert_eq!(clocks(&kernel), before);
    let reply = kernel.open_text("@Type T <<= {".into(), Default::default(), Some("T"));
    assert!(reply.status == Status::Error);
    assert_eq!(kernel.revision(), before.2 + 1);
}

#[test]
fn module_revision_counts_edits() {
    let mut kernel = loaded("RewriteFill.ali");
    let loaded_revision = kernel.revision();
    assert_eq!(kernel.module_revision(), loaded_revision);
    for (line, succeeds) in [
        ("fill 0", true),
        ("done", false),
        ("apply 0", true),
        ("undo", true),
        ("apply 0", true),
    ] {
        let reply = eval(&mut kernel, line);
        assert_eq!(reply.status == Status::Ok, succeeds, "{line}");
        assert!(!reply.edited);
        assert_eq!(kernel.module_revision(), loaded_revision, "{line}");
    }
    assert!(ok(&mut kernel, "done").edited);
    assert_eq!(kernel.module_revision(), kernel.revision());
    assert!(kernel.module_revision() > loaded_revision);
    let reply = kernel.open(&fixture("Idem.ali"));
    assert!(reply.status == Status::Ok);
    assert_eq!(kernel.module_revision(), kernel.revision());
    let loaded_revision = kernel.module_revision();
    ok(&mut kernel, "start Idem lhs rhs");
    ok(&mut kernel, "apply 0");
    assert_eq!(kernel.module_revision(), loaded_revision);
    assert!(ok(&mut kernel, "store p").edited);
    assert_eq!(kernel.module_revision(), kernel.revision());
    assert!(kernel.module_revision() > loaded_revision);
}

#[test]
fn stamps_are_revisions() {
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    assert_eq!(clocks(&kernel), (0, 0, 0));
    let reply = kernel.open(&fixture("Idem.ali"));
    assert!(reply.status == Status::Ok);
    assert_eq!(clocks(&kernel), (1, 1, 1));
    assert_stamp_order(&kernel, &reply);
    for line in [
        "start Idem lhs rhs",
        "apply 0",
        "store p",
        "undo",
        "apply 99",
        "show",
        "store map",
        "load 'missing directory/F.ali'",
    ] {
        let reply = eval(&mut kernel, line);
        assert_stamp_order(&kernel, &reply);
    }
    let before = kernel.revision();
    let reply = kernel.open(&fixture("RewriteFill.ali"));
    assert!(reply.status == Status::Ok);
    assert_eq!(clocks(&kernel), (before + 1, before + 1, before + 1));
    assert_stamp_order(&kernel, &reply);
    for line in ["fill 0", "done", "apply 0", "done", "print"] {
        let reply = eval(&mut kernel, line);
        assert_stamp_order(&kernel, &reply);
    }
    let before = kernel.revision();
    let reply = kernel.open_text("@Type T <<= {".into(), Default::default(), Some("Broken"));
    assert!(reply.status == Status::Error);
    assert_eq!(clocks(&kernel), (before + 1, before + 1, before + 1));
    assert_stamp_order(&kernel, &reply);
    assert_eq!(state(&kernel)["mode"], "unloaded");
    let before = kernel.revision();
    let reply = kernel.open_text("@Type T <<= { x }".into(), Default::default(), Some("T"));
    assert!(reply.status == Status::Ok);
    assert_eq!(clocks(&kernel), (before + 1, before + 1, before + 1));
    assert_stamp_order(&kernel, &reply);
}

fn proof_view(kernel: &mut Kernel, steps: usize) -> Value {
    let before = state(kernel);
    let before_clocks = clocks(kernel);
    let proof = kernel.view(View::Proof).expect("lazy proof view");
    assert_eq!(proof["step_count"], steps);
    assert_eq!(proof["strdiag"]["num_nodes"], steps);
    assert!(proof["output_boundary_map"].is_array());
    assert_eq!(state(kernel), before);
    assert_eq!(clocks(kernel), before_clocks);
    proof
}

#[test]
fn proof_view_lazy() {
    let mut kernel = loaded("Idem.ali");
    ok(&mut kernel, "start Idem lhs rhs");
    ok(&mut kernel, "apply 0");
    ok(&mut kernel, "apply 0");
    let completed = proof_view(&mut kernel, 2);
    ok(&mut kernel, "undo");
    let first = proof_view(&mut kernel, 1);
    assert_ne!(first, completed);
    ok(&mut kernel, "undo");
    ok(&mut kernel, "apply 1");
    let branched = proof_view(&mut kernel, 1);
    assert_ne!(branched["strdiag"], first["strdiag"]);
    assert_eq!(state(&kernel)["rewrite"]["can_redo"], false);
    ok(&mut kernel, "apply 0");
    proof_view(&mut kernel, 2);

    let mut backward = loaded("Idem.ali");
    ok(&mut backward, "backward on");
    ok(&mut backward, "start Idem rhs lhs");
    ok(&mut backward, "apply 0");
    ok(&mut backward, "apply 0");
    assert_eq!(state(&backward)["backward"], true);
    proof_view(&mut backward, 2);
    ok(&mut backward, "undo");
    proof_view(&mut backward, 1);
}

#[test]
fn preview_is_the_step_result() {
    for backward in [false, true] {
        // Each forward redex and the backward expansion get independent sessions.
        for choice in 0..if backward { 1 } else { 2 } {
            let mut kernel = loaded("Idem.ali");
            if backward {
                ok(&mut kernel, "backward on");
                ok(&mut kernel, "start Idem rhs lhs");
            } else {
                ok(&mut kernel, "start Idem lhs rhs");
            }
            let before = state(&kernel);
            let before_clocks = clocks(&kernel);
            let preview = kernel.view(View::Preview { choice }).expect("step preview");
            assert_eq!(state(&kernel), before);
            assert_eq!(clocks(&kernel), before_clocks);
            ok(&mut kernel, &format!("apply {choice}"));
            let current = kernel.view(View::Current).expect("current view");
            assert_eq!(preview, current, "backward={backward}, choice={choice}");
            assert_ne!(
                state(&kernel)["rewrite"]["current"],
                before["rewrite"]["current"]
            );
        }
    }
}

/// Each filesystem test owns its files; neither fixtures nor the process cwd move.
struct KernelTempDir(PathBuf);

impl KernelTempDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "alifib-kernel-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for KernelTempDir {
    fn drop(&mut self) {
        // Do not hide a failed assertion with a second panic during unwinding.
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

fn path_line(command: &str, path: &Path) -> String {
    format!(
        "{command} {}",
        serde_json::to_string(path.to_str().unwrap()).unwrap()
    )
}

fn within(dir: &Path) -> FsAccess {
    FsAccess::Within(CanonicalDir::new(dir).unwrap())
}

fn assert_refused_unchanged(kernel: &mut Kernel, line: &str, reason: &str) -> Reply {
    let before = state(kernel);
    let before_clocks = clocks(kernel);
    let module = serde_json::to_value(kernel.module()).unwrap();
    let reply = eval(kernel, line);
    assert!(reply.status == Status::Error, "{line}: {:?}", reply.message);
    assert!(
        reply.message.as_deref().unwrap().contains(reason),
        "{line}: {:?}",
        reply.message
    );
    assert!(!reply.edited, "{line}");
    assert!(
        reply.diagnostics.is_empty(),
        "a refusal is not a load attempt: {line}"
    );
    assert_eq!(state(kernel), before, "{line}");
    assert_eq!(clocks(kernel), before_clocks, "{line}");
    assert_eq!(
        serde_json::to_value(kernel.module()).unwrap(),
        module,
        "{line}"
    );
    assert_stamp_order(kernel, &reply);
    reply
}

fn assert_disk_reload(kernel: &mut Kernel, source: &str) {
    let revision = kernel.revision() + 1;
    let reply = ok(kernel, "load");
    assert!(!reply.edited);
    assert_eq!(clocks(kernel), (revision, revision, revision));
    assert_stamp_order(kernel, &reply);
    assert_eq!(state(kernel)["mode"], "idle");
    assert_eq!(state(kernel)["backward"], false);
    assert_eq!(kernel.module().unwrap().source, source);
}

fn start_filesystem_test_session(kernel: &mut Kernel) {
    ok(kernel, "backward on");
    ok(kernel, "start Idem lhs rhs");
    ok(kernel, "apply 0");
    assert_eq!(state(kernel)["backward"], true);
}

#[test]
fn load_reloads_from_disk() {
    let dir = KernelTempDir::new();
    let path = dir.join("Root.ali");
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    std::fs::write(&path, &source).unwrap();
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    ok(&mut kernel, &path_line("load", &path));
    start_filesystem_test_session(&mut kernel);
    let changed = format!("{source}\n(* changed on disk *)\n");
    std::fs::write(&path, &changed).unwrap();
    assert_eq!(kernel.module().unwrap().source, source);
    assert_disk_reload(&mut kernel, &changed);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
}

#[test]
fn failed_load_unloads() {
    let dir = KernelTempDir::new();
    let path = dir.join("Root.ali");
    std::fs::copy(fixture("Idem.ali"), &path).unwrap();
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    ok(&mut kernel, &path_line("load", &path));
    start_filesystem_test_session(&mut kernel);
    std::fs::write(&path, "@Type Broken <<= {").unwrap();
    let revision = kernel.revision() + 1;
    let reply = eval(&mut kernel, &path_line("load", &path));
    assert!(reply.status == Status::Error);
    assert!(!reply.edited);
    assert!(
        !reply.diagnostics.is_empty(),
        "syntax failure, not policy refusal"
    );
    assert_eq!(clocks(&kernel), (revision, revision, revision));
    assert_stamp_order(&kernel, &reply);
    assert_eq!(state(&kernel)["mode"], "unloaded");
    assert!(kernel.module().is_none());
    assert_refused_unchanged(&mut kernel, "show", "no source loaded");
}

#[test]
fn reload_after_failed_load() {
    let dir = KernelTempDir::new();
    let original = dir.join("Original.ali");
    let broken = dir.join("Broken.ali");
    std::fs::copy(fixture("Idem.ali"), &original).unwrap();
    std::fs::write(&broken, "@Type Broken <<= {").unwrap();
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    ok(&mut kernel, &path_line("load", &original));
    let revision = kernel.revision() + 1;
    let reply = eval(&mut kernel, &path_line("load", &broken));
    assert!(reply.status == Status::Error);
    assert_eq!(clocks(&kernel), (revision, revision, revision));
    assert_eq!(state(&kernel)["mode"], "unloaded");
    // A missing path is not an attempt and must not replace the failed target.
    assert_refused_unchanged(
        &mut kernel,
        &path_line("load", &dir.join("Missing.ali")),
        "cannot resolve",
    );
    let fixed = "@Type Repaired <<= { x }\n";
    std::fs::write(&broken, fixed).unwrap();
    assert_disk_reload(&mut kernel, fixed);
    assert_eq!(
        state(&kernel)["module"],
        std::fs::canonicalize(&broken).unwrap().to_str().unwrap()
    );
}

#[test]
fn missing_load_is_refused() {
    let dir = KernelTempDir::new();
    let root = dir.join("Root.ali");
    let missing = dir.join("Missing.ali");
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    std::fs::write(&root, &source).unwrap();
    for (scope, reason, disk) in [
        (FsAccess::Unrestricted, "cannot resolve", true),
        (within(&dir.0), "cannot resolve", true),
        (
            FsAccess::Denied("test disk policy"),
            "test disk policy",
            false,
        ),
    ] {
        let mut kernel = Kernel::new(scope);
        let line = path_line("load", &missing);
        assert_refused_unchanged(&mut kernel, &line, reason);
        assert_refused_unchanged(&mut kernel, "load", "no previous load to repeat");
        if disk {
            ok(&mut kernel, &path_line("load", &root));
        } else {
            let reply = kernel.open_text(source.clone(), Default::default(), Some("Root"));
            assert!(reply.status == Status::Ok);
        }
        start_filesystem_test_session(&mut kernel);
        assert_refused_unchanged(&mut kernel, &line, reason);
        if disk {
            assert_disk_reload(&mut kernel, &source);
        } else {
            assert_refused_unchanged(&mut kernel, "load", "in the browser, press Evaluate");
        }
        assert!(!missing.exists());
        assert_eq!(std::fs::read_to_string(&root).unwrap(), source);
    }
}

#[test]
fn reload_in_memory_is_refused() {
    let dir = KernelTempDir::new();
    let root = dir.join("Root.ali");
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    std::fs::write(&root, &source).unwrap();
    for (scope, disk) in [
        (FsAccess::Unrestricted, true),
        (within(&dir.0), true),
        (FsAccess::Denied("test disk policy"), false),
    ] {
        let mut kernel = Kernel::new(scope);
        // Where possible, establish an older disk target that open_text must forget.
        if disk {
            ok(&mut kernel, &path_line("load", &root));
        }
        let reply = kernel.open_text(source.clone(), Default::default(), Some("Root"));
        assert!(reply.status == Status::Ok);
        start_filesystem_test_session(&mut kernel);
        for _ in 0..2 {
            assert_refused_unchanged(&mut kernel, "load", "in the browser, press Evaluate");
        }
        assert_eq!(std::fs::read_to_string(&root).unwrap(), source);
    }
}

#[test]
fn save_in_memory_is_refused() {
    let dir = KernelTempDir::new();
    let existing = dir.join("Existing.ali");
    let new = dir.join("New.ali");
    std::fs::write(&existing, "do not overwrite").unwrap();
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    for scope in [
        FsAccess::Unrestricted,
        within(&dir.0),
        FsAccess::Denied("test disk policy"),
    ] {
        let mut kernel = Kernel::new(scope);
        let reply = kernel.open_text(source.clone(), Default::default(), Some("Root"));
        assert!(reply.status == Status::Ok);
        start_filesystem_test_session(&mut kernel);
        for line in [
            "save".to_owned(),
            path_line("save", &existing),
            path_line("save", &new),
        ] {
            assert_refused_unchanged(
                &mut kernel,
                &line,
                "save: this source belongs to the browser's editor",
            );
        }
        assert_refused_unchanged(&mut kernel, "load", "in the browser, press Evaluate");
        assert_eq!(
            std::fs::read_to_string(&existing).unwrap(),
            "do not overwrite"
        );
        assert!(!new.exists());
    }
}

#[test]
fn save_new_file_unrestricted() {
    let dir = KernelTempDir::new();
    let root = dir.join("Root.ali");
    let original = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    std::fs::write(&root, &original).unwrap();
    let mut kernel = Kernel::new(FsAccess::Unrestricted);
    ok(&mut kernel, &path_line("load", &root));
    ok(&mut kernel, "start Idem lhs rhs");
    ok(&mut kernel, "apply 0");
    ok(&mut kernel, "store saved_proof");
    let source = kernel.module().unwrap().source;
    assert_ne!(source, original);
    let before = state(&kernel);
    let before_clocks = clocks(&kernel);
    let module = serde_json::to_value(kernel.module()).unwrap();
    for name in ["New.ali", "New.txt"] {
        let path = dir.join(name);
        assert!(!path.exists());
        let reply = ok(&mut kernel, &path_line("save", &path));
        assert!(!reply.edited);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        assert_eq!(state(&kernel), before);
        assert_eq!(clocks(&kernel), before_clocks);
        assert_eq!(serde_json::to_value(kernel.module()).unwrap(), module);
        assert_stamp_order(&kernel, &reply);
    }
    for name in ["missing/New.ali", "missing/New.txt"] {
        assert_refused_unchanged(
            &mut kernel,
            &path_line("save", &dir.join(name)),
            "cannot resolve",
        );
        assert!(!dir.join(name).exists());
    }
    assert!(!dir.join("missing").exists());
    assert_eq!(std::fs::read_to_string(&root).unwrap(), original);
    // Saving elsewhere is not a load and does not change the remembered root.
    assert_disk_reload(&mut kernel, &original);
}

#[test]
fn fs_within_resolves_against_dir() {
    let dir = KernelTempDir::new();
    let cwd = std::env::current_dir().unwrap();
    let relative = PathBuf::from(format!(
        "{}.ali",
        dir.0.file_name().unwrap().to_str().unwrap()
    ));
    assert!(!cwd.join(&relative).exists());
    let root = dir.0.join(&relative);
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    std::fs::write(&root, &source).unwrap();
    let mut kernel = Kernel::new(within(&dir.0));
    ok(&mut kernel, &path_line("load", &relative));
    assert_eq!(kernel.module().unwrap().source, source);
    assert_eq!(state(&kernel)["mode"], "idle");
    assert_eq!(clocks(&kernel), (1, 1, 1));
    assert_eq!(std::env::current_dir().unwrap(), cwd);
    assert_disk_reload(&mut kernel, &source);
}

#[test]
fn fs_within_refuses_escape() {
    let dir = KernelTempDir::new();
    let scope = dir.join("scope");
    let outside = dir.join("outside");
    std::fs::create_dir(&scope).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let root = scope.join("Root.ali");
    let external = outside.join("Outside.ali");
    let non_ali = scope.join("NotAli.txt");
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    for path in [&root, &external, &non_ali] {
        std::fs::write(path, &source).unwrap();
    }
    let denied_paths = vec![
        (PathBuf::from("../outside/Outside.ali"), "outside"),
        (external.clone(), "outside"),
        (PathBuf::from("NotAli.txt"), "not an .ali file"),
    ];
    #[cfg(unix)]
    let denied_paths = {
        use std::os::unix::fs::symlink;
        let mut paths = denied_paths;
        symlink(&external, scope.join("Link.ali")).unwrap();
        symlink(outside.join("Missing.ali"), scope.join("Dangling.ali")).unwrap();
        symlink(&outside, scope.join("portal")).unwrap();
        paths.extend([
            (PathBuf::from("Link.ali"), "outside"),
            (PathBuf::from("Dangling.ali"), "cannot resolve"),
            (PathBuf::from("portal/Outside.ali"), "outside"),
        ]);
        paths
    };
    let mut kernel = Kernel::new(within(&scope));
    ok(&mut kernel, &path_line("load", &root));
    for (path, reason) in denied_paths {
        for command in ["load", "save"] {
            start_filesystem_test_session(&mut kernel);
            ok(&mut kernel, "store blocked_proof");
            assert_ne!(kernel.module().unwrap().source, source);
            assert_refused_unchanged(&mut kernel, &path_line(command, &path), reason);
            assert_eq!(std::fs::read_to_string(&external).unwrap(), source);
            assert_eq!(std::fs::read_to_string(&non_ali).unwrap(), source);
            assert_eq!(std::fs::read_to_string(&root).unwrap(), source);
            assert!(!scope.join("New.txt").exists());
            assert!(!outside.join("Missing.ali").exists());
            assert_disk_reload(&mut kernel, &source);
        }
    }
    start_filesystem_test_session(&mut kernel);
    assert_refused_unchanged(&mut kernel, "save New.txt", "not an .ali file");
    assert!(!scope.join("New.txt").exists());
    assert_disk_reload(&mut kernel, &source);
    #[cfg(unix)]
    {
        // A new filename has no entry to canonicalise: its parent must be checked.
        start_filesystem_test_session(&mut kernel);
        assert_refused_unchanged(&mut kernel, "save portal/New.ali", "outside");
        assert!(!outside.join("New.ali").exists());
        assert_disk_reload(&mut kernel, &source);
    }
}

#[cfg(unix)]
#[test]
fn fs_within_refuses_replaced_root_symlinks() {
    use std::os::unix::fs::symlink;

    for dangling in [false, true] {
        let dir = KernelTempDir::new();
        let scope = dir.join("scope");
        std::fs::create_dir(&scope).unwrap();
        let root = scope.join("Root.ali");
        let parked = scope.join("Parked.ali");
        let outside = dir.join("Outside.ali");
        let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
        std::fs::write(&root, &source).unwrap();
        if !dangling {
            std::fs::write(&outside, &source).unwrap();
        }
        let mut kernel = Kernel::new(within(&scope));
        ok(&mut kernel, &path_line("load", &root));
        start_filesystem_test_session(&mut kernel);
        ok(&mut kernel, "store blocked_proof");
        assert_ne!(kernel.module().unwrap().source, source);
        std::fs::rename(&root, &parked).unwrap();
        symlink(&outside, &root).unwrap();
        let reason = if dangling {
            "cannot resolve"
        } else {
            "outside"
        };
        for line in ["load", "save", "load"] {
            assert_refused_unchanged(&mut kernel, line, reason);
            if dangling {
                assert!(
                    !outside.exists(),
                    "save must not follow a dangling root link"
                );
            } else {
                assert_eq!(std::fs::read_to_string(&outside).unwrap(), source);
            }
            assert_eq!(std::fs::read_to_string(&parked).unwrap(), source);
        }
        // Replacing the link with the original file makes the remembered load usable.
        std::fs::rename(&parked, &root).unwrap();
        assert_disk_reload(&mut kernel, &source);
    }
}

#[test]
fn fs_denied_refuses_disk() {
    let dir = KernelTempDir::new();
    let existing = dir.join("Existing.ali");
    let new = dir.join("New.ali");
    let source = std::fs::read_to_string(fixture("Idem.ali")).unwrap();
    std::fs::write(&existing, &source).unwrap();
    let reason = "disk disabled for this test";
    let mut kernel = Kernel::new(FsAccess::Denied(reason));
    for path in [&existing, &new] {
        let reply = assert_refused_unchanged(&mut kernel, &path_line("load", path), reason);
        assert_eq!(reply.message.as_deref(), Some(reason));
        assert_refused_unchanged(&mut kernel, "load", "no previous load to repeat");
    }
    for line in [
        "save".to_owned(),
        path_line("save", &existing),
        path_line("save", &new),
    ] {
        let reply = assert_refused_unchanged(&mut kernel, &line, "no source loaded");
        assert_eq!(reply.message.as_deref(), Some("no source loaded"));
    }
    let reply = kernel.open_text(source.clone(), Default::default(), Some("Root"));
    assert!(reply.status == Status::Ok);
    start_filesystem_test_session(&mut kernel);
    ok(&mut kernel, "store blocked_proof");
    assert_ne!(kernel.module().unwrap().source, source);
    for path in [&existing, &new] {
        let reply = assert_refused_unchanged(&mut kernel, &path_line("load", path), reason);
        assert_eq!(reply.message.as_deref(), Some(reason));
        assert_refused_unchanged(&mut kernel, "load", "in the browser, press Evaluate");
    }
    for line in [
        "save".to_owned(),
        path_line("save", &existing),
        path_line("save", &new),
    ] {
        let reply = assert_refused_unchanged(
            &mut kernel,
            &line,
            "save: this source belongs to the browser's editor",
        );
        assert_eq!(
            reply.message.as_deref(),
            Some("save: this source belongs to the browser's editor")
        );
    }
    assert_refused_unchanged(&mut kernel, "load", "in the browser, press Evaluate");
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), source);
    assert!(!new.exists());
}

#[test]
fn same_name_types_in_two_modules() {
    let mut kernel = loaded("SameNameRoot.ali");
    ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    let canonical = |name: &str| {
        std::fs::canonicalize(fixture(name))
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    assert_eq!(module.root, canonical("SameNameRoot.ali"));
    let a_path = canonical("SameNameRoot/A.ali");
    let b_path = canonical("SameNameRoot/B.ali");
    assert_ne!(a_path, b_path);
    let mut all_tags = std::collections::HashSet::new();
    let cases = [
        (
            a_path.as_str(),
            ["a0", "a1", "aEdge"],
            "aEdge",
            &["F", "Ob"][..],
            "F",
            "s",
        ),
        (
            b_path.as_str(),
            ["b0", "b1", "bEdge"],
            "b0",
            &["BMap", "Ob"][..],
            "BMap",
            "b0",
        ),
        (
            module.root.as_str(),
            ["root0", "root1", "rootEdge"],
            "root1",
            &["F", "Ob", "RootMap"][..],
            "F",
            "s",
        ),
    ];
    for (path, names, thin_name, map_names, map, source) in cases {
        let ty = module
            .types
            .iter()
            .find(|t| t.module == path && t.name == "Ob")
            .expect("each module has its own Ob");
        assert_eq!(
            ty.generators
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>(),
            names
        );
        let tags: Vec<usize> = ty.generators.iter().map(|g| g.tag.unwrap()).collect();
        for tag in &tags {
            assert!(
                all_tags.insert(*tag),
                "different Ob types cannot share generator IDs"
            );
        }
        assert_eq!(
            ty.generators.iter().map(|g| g.dim).collect::<Vec<_>>(),
            [0, 0, 1]
        );
        assert!(ty.generators[0].face_tags.is_empty());
        assert!(ty.generators[1].face_tags.is_empty());
        assert_eq!(ty.generators[2].face_tags, tags[..2]);
        assert_eq!(ty.generators[2].input.as_ref().unwrap().label, names[0]);
        assert_eq!(ty.generators[2].output.as_ref().unwrap().label, names[1]);
        let thin_tag = ty
            .generators
            .iter()
            .find(|g| g.name == thin_name)
            .unwrap()
            .tag
            .unwrap();
        assert!(!ty.thin_tags.is_empty());
        // Identity maps can repeat a thin tag; every occurrence must still be
        // this module's actual generator ID, never another Ob's ID.
        assert!(ty.thin_tags.iter().all(|tag| *tag == thin_tag));
        assert_eq!(
            ty.maps.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            map_names
        );
        let own_map = ty.maps.iter().find(|m| m.name == map).unwrap();
        if map == "F" {
            assert_eq!(
                own_map.holes,
                [format!("?arr : {} → {}", names[0], names[1])]
            );
            assert_eq!(
                own_map.domain,
                if path == module.root {
                    "A.Arrow"
                } else {
                    "Arrow"
                }
            );
        } else {
            assert!(own_map.holes.is_empty());
            assert_eq!(own_map.domain, "Ob");
        }
        let item = ty.diagrams.iter().find(|d| d.name == "item").unwrap();
        assert_eq!(item.expr, names[2]);
        assert_eq!(item.dim, 1);
        let drawn = kernel
            .view(View::Item {
                module: path.to_owned(),
                type_name: "Ob".into(),
                item: "item".into(),
                boundary: None,
            })
            .expect("included and root items resolve in their own modules");
        assert_eq!(drawn["label"], names[2]);
        assert_eq!(drawn["dim"], 1);
        assert_eq!(drawn["input"], names[0]);
        assert_eq!(drawn["output"], names[1]);
        let vertices = drawn["strdiag"]["vertices"].as_array().unwrap();
        assert_eq!(vertices.len(), 3);
        let node = vertices.iter().find(|v| v["kind"] == "node").unwrap();
        assert_eq!(node["label"], names[2]);
        assert_eq!(node["tag"], serde_json::json!(tags[2]));
        let image = kernel
            .view(View::MapImage {
                module: path.to_owned(),
                type_name: "Ob".into(),
                map: map.into(),
                generator: source.into(),
                boundary: None,
            })
            .expect("map image uses the module too");
        assert_eq!(image["label"], names[0]);
        assert_eq!(image["dim"], 0);
        assert_eq!(
            image["strdiag"]["vertices"][0]["tag"],
            serde_json::json!(tags[0])
        );
    }
    assert_eq!(all_tags.len(), 9);
    for (path, alias) in [(&a_path, "A.Ob"), (&b_path, "B.Ob")] {
        let original = module
            .types
            .iter()
            .find(|t| &t.module == path && t.name == "Ob")
            .unwrap();
        let imported = module
            .types
            .iter()
            .find(|t| t.module == module.root && t.name == alias)
            .unwrap();
        assert_eq!(
            imported
                .generators
                .iter()
                .map(|g| (&g.name, g.tag, &g.face_tags))
                .collect::<Vec<_>>(),
            original
                .generators
                .iter()
                .map(|g| (&g.name, g.tag, &g.face_tags))
                .collect::<Vec<_>>()
        );
        assert_eq!(imported.thin_tags, original.thin_tags);
    }
    assert_eq!(module.holes.len(), 1, "A.Ob.F exists but is not root-owned");
    let hole = &module.holes[0];
    assert_eq!(hole.module, module.root);
    assert_eq!(
        (
            hole.index,
            hole.type_name.as_str(),
            hole.map_name.as_str(),
            hole.source_name.as_str()
        ),
        (0, "Ob", "F", "arr")
    );
    assert_eq!(hole.boundary, "?arr : root0 → root1");
    assert!(module.constraints.is_empty());
}

#[test]
fn holes_are_the_roots_to_edit() {
    let mut kernel = loaded("IncludeHole.ali");
    let listed = ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    assert_eq!(
        module.root,
        std::fs::canonicalize(fixture("IncludeHole.ali"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        module.holes.len(),
        1,
        "included H must never receive a number"
    );
    let hole = &module.holes[0];
    assert_eq!(hole.module, module.root);
    assert_eq!(
        (
            hole.index,
            hole.type_name.as_str(),
            hole.map_name.as_str(),
            hole.source_name.as_str()
        ),
        (0, "MapHole.A", "G", "arr")
    );
    assert!(!rendered(&listed).contains(".H"));
    let included_path = std::fs::canonicalize(fixture("MapHole.ali")).unwrap();
    let included = module
        .types
        .iter()
        .find(|t| t.module == included_path.to_str().unwrap() && t.name == "A")
        .unwrap();
    assert_eq!(
        included
            .maps
            .iter()
            .find(|m| m.name == "H")
            .unwrap()
            .holes
            .len(),
        1,
        "H is still open, just not editable in this source"
    );
    let old_source = module.source;
    ok(&mut kernel, "fill 0");
    assert_eq!(state(&kernel)["hole"]["map_name"], "G");
    assert_eq!(state(&kernel)["hole"]["type_name"], "MapHole.A");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["rewrite"]["target_reached"], true);
    let done = ok(&mut kernel, "done");
    assert!(done.edited);
    let updated = kernel.module().unwrap();
    assert_eq!(
        updated.source,
        old_source.replacen("arr => ?", "arr => f", 1)
    );
    ok(&mut kernel, "holes");
    assert!(kernel.module().unwrap().holes.is_empty());

    // Load the edited source a second time, with the dependency in memory.
    // No save, fixture rewrite, or temporary disk file is involved.
    let modules = std::collections::HashMap::from([(
        included_path.to_string_lossy().into_owned(),
        std::fs::read_to_string(fixture("MapHole.ali")).unwrap(),
    )]);
    let mut reloaded = Kernel::new(FsAccess::Denied("memory-only reload"));
    let reply = reloaded.open_text(updated.source.clone(), modules, Some(&updated.root));
    assert!(reply.status == Status::Ok, "{:?}", reply.message);
    ok(&mut reloaded, "holes");
    let again = reloaded.module().unwrap();
    assert_eq!(again.root, updated.root);
    assert_eq!(again.source, updated.source);
    assert!(again.holes.is_empty());
    let alias = again
        .types
        .iter()
        .find(|t| t.module == again.root && t.name == "MapHole.A")
        .unwrap();
    assert!(
        alias
            .maps
            .iter()
            .find(|m| m.name == "G")
            .unwrap()
            .holes
            .is_empty()
    );
    assert_eq!(
        alias
            .maps
            .iter()
            .find(|m| m.name == "H")
            .unwrap()
            .holes
            .len(),
        1
    );
    // Do not compare global IDs across this reload: those are fresh allocations.
}

#[test]
fn map_owner_is_matched_whole() {
    let mut kernel = loaded("MapOwner.ali");
    ok(&mut kernel, "holes");
    let module = kernel.module().unwrap();
    assert_eq!(
        module.root,
        std::fs::canonicalize(fixture("MapOwner.ali"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(module.holes.len(), 2);
    assert_eq!(
        module
            .holes
            .iter()
            .map(|h| (
                h.type_name.as_str(),
                h.map_name.as_str(),
                h.source_name.as_str()
            ))
            .collect::<Vec<_>>(),
        [("M.Ob", "F", "arr"), ("Ob", "F", "arr")]
    );
    assert!(module.holes.iter().all(|h| h.module == module.root));
    let index = module
        .holes
        .iter()
        .find(|h| h.type_name == "Ob")
        .unwrap()
        .index;
    assert_eq!(index, 1);
    let (prefix, root_block) = module.source.split_once("\n@Ob\n").unwrap();
    assert!(prefix.contains("@M.Ob\n"));
    assert!(prefix.contains("arr => ?"));
    assert!(root_block.contains("arr => ?"));
    // The included type must remain addressable by the exact whole address.
    let before = kernel
        .view(View::MapImage {
            module: module.root.clone(),
            type_name: "M.Ob".into(),
            map: "F".into(),
            generator: "s".into(),
            boundary: None,
        })
        .unwrap();
    assert_eq!(before["label"], "inc0");

    ok(&mut kernel, &format!("fill {index}"));
    assert_eq!(state(&kernel)["hole"]["type_name"], "Ob");
    assert_eq!(state(&kernel)["hole"]["dim"], 1);
    assert_eq!(state(&kernel)["by"]["rewrite"]["current"]["label"], "root0");
    assert_eq!(state(&kernel)["by"]["rewrite"]["target"]["label"], "root1");
    ok(&mut kernel, "apply 0");
    assert_eq!(state(&kernel)["by"]["rewrite"]["target_reached"], true);
    let done = ok(&mut kernel, "done");
    assert!(done.edited);
    let updated = kernel.module().unwrap();
    let (after_prefix, after_root_block) = updated.source.split_once("\n@Ob\n").unwrap();
    assert_eq!(
        after_prefix.as_bytes(),
        prefix.as_bytes(),
        "including every byte of @M.Ob"
    );
    assert_eq!(
        after_root_block,
        root_block.replacen("arr => ?", "arr => rootEdge", 1),
        "a 0-cell rewrite proves the 1-cell rootEdge assignment, not root0"
    );
    assert_eq!(updated.source, format!("{prefix}\n@Ob\n{after_root_block}"));
    ok(&mut kernel, "holes");
    let remaining = kernel.module().unwrap();
    assert_eq!(remaining.holes.len(), 1);
    assert_eq!(remaining.holes[0].module, remaining.root);
    assert_eq!(
        (
            remaining.holes[0].index,
            remaining.holes[0].type_name.as_str(),
            remaining.holes[0].map_name.as_str()
        ),
        (0, "M.Ob", "F")
    );
    let image = kernel
        .view(View::MapImage {
            module: remaining.root,
            type_name: "M.Ob".into(),
            map: "F".into(),
            generator: "s".into(),
            boundary: None,
        })
        .expect("exact M.Ob is still mappable after editing Ob");
    assert_eq!(image["label"], "inc0");
}
