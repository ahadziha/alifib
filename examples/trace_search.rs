//! Exhaustive schedule-space analysis of the concurrency benchmarks
//! (`Philosophers.ali`, `Corridor.ali`).
//!
//! A *schedule* is a maximal run of the rewrite engine — a sequence of
//! `step` choices until no rewrite applies. A *trace* is the proof
//! diagram such a run assembles, which records only the causal order of
//! events: schedules that differ by commuting independent steps build
//! the same diagram. This driver measures the gap between the two —
//! the quotient that partial-order reduction computes by hand — using
//! nothing but the engine's session API and its ordinary diagram
//! equality. It contains no concurrency machinery of its own.
//!
//! Per benchmark it reports:
//!  - the reachable states (BFS, deduplicated by `Diagram::isomorphic`);
//!  - the exact number of schedules (path counting over the state DAG —
//!    the systems are terminating, so the graph is acyclic);
//!  - the verdict of every terminal state — success or deadlock. This
//!    is exhaustive over all schedules whatever their number, since the
//!    state graph covers them;
//!  - the distinct traces: by walking *every* schedule and collapsing
//!    the assembled proofs under `Diagram::isomorphic` when the count
//!    permits, by classifying random schedules the same way otherwise
//!    (reported as a lower bound).
//!
//! Run with: cargo run -p alifib --release --example trace_search
//! (about three minutes; the full walk of the 119,328 schedules of
//! four philosophers dominates).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alifib::interactive::engine::{eval_diagram_expr, load_type_context, RewriteEngine};
use alifib::interpreter::GlobalStore;
use alifib::output::render_diagram;
use alifib::{Complex, Diagram};

/// Walk every schedule when there are at most this many; sample beyond.
const FULL_WALK_BOUND: u128 = 150_000;
const SAMPLES: usize = 2_000;

struct Bench {
    title: String,
    file: &'static str,
    type_name: &'static str,
    initial: String,
    success: Option<String>,
}

/// `f p f p ... f` with `n` philosophers, and its all-fed final word.
fn philosophers(n: usize) -> Bench {
    Bench {
        title: format!("Philosophers, {n} in a row"),
        file: "Philosophers.ali",
        type_name: "Philosophers",
        initial: format!("f {}", "p f ".repeat(n).trim_end()),
        success: Some(format!("f {}", "done f ".repeat(n).trim_end())),
    }
}

/// `w s ... s e` with `m` track segments. No success state exists.
fn corridor(m: usize) -> Bench {
    Bench {
        title: format!("Corridor, {m} segments"),
        file: "Corridor.ali",
        type_name: "Corridor",
        initial: format!("w {}e", "s ".repeat(m)),
        success: None,
    }
}

struct Ctx {
    store: Arc<GlobalStore>,
    complex: Arc<Complex>,
    path: String,
    type_name: String,
}

impl Ctx {
    fn load(bench: &Bench) -> Self {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(bench.file)
            .to_string_lossy()
            .into_owned();
        let (store, complex, path) =
            load_type_context(&file, bench.type_name).expect("benchmark file should load");
        Ctx { store, complex, path, type_name: bench.type_name.to_owned() }
    }

    fn eval(&self, expr: &str) -> Diagram {
        eval_diagram_expr(&self.store, &self.complex, &self.path, expr)
            .expect("diagram expression should evaluate")
    }

    /// A fresh session sitting at `state`, with no target.
    fn engine_at(&self, state: &Diagram) -> RewriteEngine {
        RewriteEngine::from_diagrams(
            Arc::clone(&self.store),
            Arc::clone(&self.complex),
            state.clone(),
            None,
            self.path.clone(),
            self.type_name.clone(),
            String::new(),
            None,
            false,
        )
        .expect("session should start from a reachable state")
    }

    fn label(&self, d: &Diagram) -> String {
        render_diagram(d, &self.complex)
    }
}

/// BFS over reachable states. States are keyed by their rendered label
/// (canonical for these 1-dimensional words) and the keying is checked:
/// a label collision must be an isomorphism. Successor lists keep
/// duplicates — two distinct choices reaching the same state are two
/// distinct schedule steps.
fn state_graph(ctx: &Ctx, init: &Diagram) -> (Vec<(String, Diagram)>, HashMap<String, Vec<String>>) {
    let mut states: Vec<(String, Diagram)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut succs: HashMap<String, Vec<String>> = HashMap::new();

    let init_label = ctx.label(init);
    index.insert(init_label.clone(), 0);
    states.push((init_label.clone(), init.clone()));
    let mut frontier = vec![init_label];

    while let Some(label) = frontier.pop() {
        let state = states[index[&label]].1.clone();
        let mut engine = ctx.engine_at(&state);
        let mut out = Vec::new();
        for i in 0..engine.rewrites().len() {
            engine.step(i).expect("listed rewrite should apply");
            let succ = engine.current_diagram().clone();
            engine.undo().expect("undo after step");
            let succ_label = ctx.label(&succ);
            if let Some(&k) = index.get(&succ_label) {
                assert!(
                    Diagram::isomorphic(&states[k].1, &succ),
                    "label keying must be canonical: {succ_label}"
                );
            } else {
                index.insert(succ_label.clone(), states.len());
                states.push((succ_label.clone(), succ));
                frontier.push(succ_label.clone());
            }
            out.push(succ_label);
        }
        succs.insert(label, out);
    }
    (states, succs)
}

/// Exact schedule count: maximal paths through the (acyclic) state graph.
fn count_schedules(label: &str, succs: &HashMap<String, Vec<String>>, memo: &mut HashMap<String, u128>) -> u128 {
    if let Some(&n) = memo.get(label) {
        return n;
    }
    let out = &succs[label];
    let n = if out.is_empty() {
        1
    } else {
        out.iter().map(|s| count_schedules(s, succs, memo)).sum()
    };
    memo.insert(label.to_owned(), n);
    n
}

/// One causal story: a proof diagram up to isomorphism, with the
/// schedules observed building it and the final state they reach.
struct TraceClass {
    proof: Diagram,
    final_label: String,
    success: bool,
    schedules: u128,
}

fn classify(
    ctx: &Ctx,
    engine: &RewriteEngine,
    success: &Option<Diagram>,
    classes: &mut Vec<TraceClass>,
) {
    let proof = engine.assemble_proof().expect("maximal run should assemble");
    match classes.iter_mut().find(|c| Diagram::isomorphic(&c.proof, &proof)) {
        Some(c) => c.schedules += 1,
        None => classes.push(TraceClass {
            final_label: ctx.label(engine.current_diagram()),
            success: success
                .as_ref()
                .is_some_and(|s| Diagram::isomorphic(engine.current_diagram(), s)),
            proof,
            schedules: 1,
        }),
    }
}

/// Walk every schedule by depth-first search with step/undo, collapsing
/// completed runs into trace classes as they finish.
fn walk_all(ctx: &Ctx, engine: &mut RewriteEngine, success: &Option<Diagram>, classes: &mut Vec<TraceClass>) {
    let n = engine.rewrites().len();
    if n == 0 {
        classify(ctx, engine, success, classes);
        return;
    }
    for i in 0..n {
        engine.step(i).expect("listed rewrite should apply");
        walk_all(ctx, engine, success, classes);
        engine.undo().expect("undo after step");
    }
}

/// Classify `SAMPLES` uniformly random schedules instead of all of them.
fn sample(ctx: &Ctx, init: &Diagram, success: &Option<Diagram>, classes: &mut Vec<TraceClass>) {
    for _ in 0..SAMPLES {
        let mut engine = ctx.engine_at(init);
        engine.random(usize::MAX).expect("random run should complete");
        classify(ctx, &engine, success, classes);
    }
}

fn thousands(n: u128) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// How many trace-class rows to print before eliding.
const CLASS_ROWS: usize = 12;

fn run(bench: &Bench) {
    println!("== {} ==", bench.title);
    let ctx = Ctx::load(bench);
    let init = ctx.eval(&bench.initial);
    let success = bench.success.as_ref().map(|s| ctx.eval(s));
    println!("   initial:   {}", ctx.label(&init));

    let (states, succs) = state_graph(&ctx, &init);
    let mut memo = HashMap::new();
    let total = count_schedules(&states[0].0, &succs, &mut memo);
    println!("   states:    {}", thousands(states.len() as u128));
    println!("   schedules: {} (exact)", thousands(total));

    // Terminal-state verdicts cover every schedule, however many.
    let terminal: Vec<&(String, Diagram)> =
        states.iter().filter(|(l, _)| succs[l].is_empty()).collect();
    let stuck = terminal
        .iter()
        .filter(|(_, d)| !success.as_ref().is_some_and(|s| Diagram::isomorphic(d, s)))
        .count();
    println!(
        "   terminal:  {} state(s) — {} success, {} deadlock  (exhaustive over all schedules)",
        terminal.len(),
        terminal.len() - stuck,
        stuck
    );

    let mut classes = Vec::new();
    let walked_all = total <= FULL_WALK_BOUND;
    if walked_all {
        walk_all(&ctx, &mut ctx.engine_at(&init), &success, &mut classes);
    } else {
        sample(&ctx, &init, &success, &mut classes);
    }
    classes.sort_by(|a, b| b.schedules.cmp(&a.schedules));

    let (count_word, qualifier) = if walked_all {
        ("schedules", "every schedule walked".to_owned())
    } else {
        ("samples", format!("lower bound from {SAMPLES} random schedules"))
    };
    let at_least = if walked_all { "" } else { ">= " };
    println!("   traces:    {at_least}{}  ({qualifier})", classes.len());
    for c in classes.iter().take(CLASS_ROWS) {
        let verdict = if c.success { "success" } else { "DEADLOCK" };
        println!(
            "     {:>10} {count_word} -> {}  [{}]",
            thousands(c.schedules),
            c.final_label,
            verdict
        );
    }
    if classes.len() > CLASS_ROWS {
        println!("     ... and {} more trace classes", classes.len() - CLASS_ROWS);
    }

    if let Some(c) = classes.iter().find(|c| !c.success) {
        let cert = ctx.label(&c.proof);
        if cert.len() <= 300 {
            println!("   a deadlock certificate, replayable with `resume`:");
            println!("     {cert}");
        }
    }
    println!();
}

fn main() {
    for bench in [
        philosophers(2),
        philosophers(3),
        philosophers(4),
        philosophers(5),
        corridor(4),
        corridor(10),
        corridor(40),
    ] {
        run(&bench);
    }
}
