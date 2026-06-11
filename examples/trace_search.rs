//! Exhaustive interleaving-space analysis of the concurrency benchmarks
//! (`Philosophers.ali`, `Corridor.ali`).
//!
//! An *interleaving* is a maximal run of the rewrite engine — a sequence
//! of `step` choices until no rewrite applies. A *trace* is the proof
//! diagram such a run assembles, which records only the causal order of
//! events: interleavings that differ by commuting independent steps build
//! the same diagram. This driver measures the gap between the two —
//! the quotient that partial-order reduction computes by hand — using
//! nothing but the engine's session API and its ordinary diagram
//! equality. It contains no concurrency machinery of its own.
//!
//! Per benchmark it reports:
//!  - the reachable states (BFS, deduplicated by `Diagram::isomorphic`);
//!  - the exact number of interleavings (path counting over the state DAG —
//!    the systems are terminating, so the graph is acyclic);
//!  - the verdict of every terminal state — success or deadlock. This
//!    is exhaustive over all interleavings whatever their number, since the
//!    state graph covers them;
//!  - the distinct traces: by walking *every* interleaving and collapsing
//!    the assembled proofs under `Diagram::isomorphic` when the count
//!    permits, by classifying random interleavings the same way otherwise
//!    (reported as a lower bound).
//!
//! Run with: cargo run -p alifib --release --example trace_search
//! (about three minutes; the full walk of the 119,328 interleavings of
//! four philosophers dominates).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alifib::interactive::engine::{eval_diagram_expr, load_type_context, RewriteEngine};
use alifib::interpreter::GlobalStore;
use alifib::output::render_diagram;
use alifib::{Complex, Diagram};

/// Walk every interleaving when there are at most this many; sample beyond.
const FULL_WALK_BOUND: u128 = 150_000;
const SAMPLES: usize = 2_000;

/// What counts as a successful state.
enum Success {
    /// Isomorphic to one specific diagram.
    Exact(String),
    /// Contains all the named cells (the rest may vary, e.g. drifting
    /// forks around the fed philosophers of the ring).
    AllOf(&'static [&'static str]),
    /// No state counts as success.
    Never,
}

struct Bench {
    title: String,
    file: &'static str,
    type_name: &'static str,
    initial: String,
    success: Success,
}

/// `f p f p ... f` with `n` philosophers, and its all-fed final word.
fn philosophers(n: usize) -> Bench {
    Bench {
        title: format!("Philosophers, {n} in a row"),
        file: "Philosophers.ali",
        type_name: "Philosophers",
        initial: format!("f {}", "p f ".repeat(n).trim_end()),
        success: Success::Exact(format!("f {}", "done f ".repeat(n).trim_end())),
    }
}

/// `w s ... s e` with `m` track segments. No success state exists.
fn corridor(m: usize) -> Bench {
    Bench {
        title: format!("Corridor, {m} segments"),
        file: "Corridor.ali",
        type_name: "Corridor",
        initial: format!("w {}e", "s ".repeat(m)),
        success: Success::Never,
    }
}

/// Three philosophers around a table, the ring encoded by fork sorts.
/// Success is everyone fed, with the free forks drifting anywhere.
fn ring() -> Bench {
    Bench {
        title: "Philosophers, 3 around a table".to_owned(),
        file: "PhilosophersRing.ali",
        type_name: "PhilosophersRing",
        initial: "f1 p1 f2 p2 f3 p3".to_owned(),
        success: Success::AllOf(&["done1", "done2", "done3"]),
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
/// distinct interleaving steps.
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

/// Exact interleaving count: maximal paths through the (acyclic) state graph.
fn count_interleavings(label: &str, succs: &HashMap<String, Vec<String>>, memo: &mut HashMap<String, u128>) -> u128 {
    if let Some(&n) = memo.get(label) {
        return n;
    }
    let out = &succs[label];
    let n = if out.is_empty() {
        1
    } else {
        out.iter().map(|s| count_interleavings(s, succs, memo)).sum()
    };
    memo.insert(label.to_owned(), n);
    n
}

/// One causal story: a proof diagram up to isomorphism, with the
/// interleavings observed building it and the final state they reach.
struct TraceClass {
    proof: Diagram,
    final_label: String,
    success: bool,
    interleavings: u128,
}

fn classify(
    ctx: &Ctx,
    engine: &RewriteEngine,
    is_success: &dyn Fn(&str, &Diagram) -> bool,
    classes: &mut Vec<TraceClass>,
) {
    let proof = engine.assemble_proof().expect("maximal run should assemble");
    match classes.iter_mut().find(|c| Diagram::isomorphic(&c.proof, &proof)) {
        Some(c) => c.interleavings += 1,
        None => {
            let final_label = ctx.label(engine.current_diagram());
            classes.push(TraceClass {
                success: is_success(&final_label, engine.current_diagram()),
                final_label,
                proof,
                interleavings: 1,
            })
        }
    }
}

/// Walk every interleaving by depth-first search with step/undo, collapsing
/// completed runs into trace classes as they finish.
fn walk_all(
    ctx: &Ctx,
    engine: &mut RewriteEngine,
    is_success: &dyn Fn(&str, &Diagram) -> bool,
    classes: &mut Vec<TraceClass>,
) {
    let n = engine.rewrites().len();
    if n == 0 {
        classify(ctx, engine, is_success, classes);
        return;
    }
    for i in 0..n {
        engine.step(i).expect("listed rewrite should apply");
        walk_all(ctx, engine, is_success, classes);
        engine.undo().expect("undo after step");
    }
}

/// Classify `SAMPLES` uniformly random interleavings instead of all of them.
fn sample(
    ctx: &Ctx,
    init: &Diagram,
    is_success: &dyn Fn(&str, &Diagram) -> bool,
    classes: &mut Vec<TraceClass>,
) {
    for _ in 0..SAMPLES {
        let mut engine = ctx.engine_at(init);
        engine.random(usize::MAX).expect("random run should complete");
        classify(ctx, &engine, is_success, classes);
    }
}

/// Is the state graph acyclic? Iterative three-colour DFS. Acyclicity
/// is what makes interleaving counting and exhaustive trace walks possible;
/// structural cells (the ring's fork drift) break it.
fn is_acyclic(start: &str, succs: &HashMap<String, Vec<String>>) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Colour {
        Open,
        Done,
    }
    let mut colour: HashMap<&str, Colour> = HashMap::new();
    let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
    colour.insert(start, Colour::Open);
    while let Some((label, next)) = stack.pop() {
        let out = &succs[label];
        if next < out.len() {
            stack.push((label, next + 1));
            let succ = out[next].as_str();
            match colour.get(succ) {
                Some(Colour::Open) => return false,
                Some(Colour::Done) => {}
                None => {
                    colour.insert(succ, Colour::Open);
                    stack.push((succ, 0));
                }
            }
        } else {
            colour.insert(label, Colour::Done);
        }
    }
    true
}

/// The labels of states from which no success state is reachable —
/// computed by reverse reachability from the success states.
fn doomed_states<'a>(
    states: &'a [(String, Diagram)],
    succs: &HashMap<String, Vec<String>>,
    is_success: &dyn Fn(&str, &Diagram) -> bool,
) -> Vec<&'a str> {
    let mut reverse: HashMap<&str, Vec<&str>> = HashMap::new();
    for (label, _) in states {
        for succ in &succs[label] {
            reverse.entry(succ).or_default().push(label);
        }
    }
    let mut saved: Vec<&str> = states
        .iter()
        .filter(|(l, d)| is_success(l, d))
        .map(|(l, _)| l.as_str())
        .collect();
    let mut reached: std::collections::HashSet<&str> = saved.iter().copied().collect();
    while let Some(label) = saved.pop() {
        for &prev in reverse.get(label).into_iter().flatten() {
            if reached.insert(prev) {
                saved.push(prev);
            }
        }
    }
    states
        .iter()
        .map(|(l, _)| l.as_str())
        .filter(|l| !reached.contains(l))
        .collect()
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
    let success_diag = match &bench.success {
        Success::Exact(expr) => Some(ctx.eval(expr)),
        _ => None,
    };
    let is_success: Box<dyn Fn(&str, &Diagram) -> bool> = match &bench.success {
        Success::Exact(_) => {
            let s = success_diag.clone().unwrap();
            Box::new(move |_: &str, d: &Diagram| Diagram::isomorphic(d, &s))
        }
        Success::AllOf(names) => Box::new(move |label: &str, _: &Diagram| {
            let tokens: Vec<&str> = label
                .split_whitespace()
                .map(|t| t.trim_matches(|c| c == '(' || c == ')'))
                .collect();
            names.iter().all(|n| tokens.contains(n))
        }),
        Success::Never => Box::new(|_: &str, _: &Diagram| false),
    };
    println!("   initial:   {}", ctx.label(&init));

    let (states, succs) = state_graph(&ctx, &init);
    println!("   states:    {}", thousands(states.len() as u128));

    // Terminal-state verdicts cover every interleaving, however many.
    let terminal: Vec<&(String, Diagram)> =
        states.iter().filter(|(l, _)| succs[l].is_empty()).collect();
    let stuck: Vec<&&(String, Diagram)> =
        terminal.iter().filter(|(l, d)| !is_success(l, d)).collect();
    println!(
        "   terminal:  {} state(s) — {} success, {} deadlock  (exhaustive over all interleavings)",
        terminal.len(),
        terminal.len() - stuck.len(),
        stuck.len()
    );
    if (1..=4).contains(&stuck.len()) {
        for (l, _) in stuck.iter().copied() {
            println!("     deadlock state: {l}");
        }
    }

    // States from which success is unreachable: the doomed basin.
    if !matches!(bench.success, Success::Never) {
        let doomed = doomed_states(&states, &succs, is_success.as_ref());
        println!(
            "   doomed:    {} state(s) from which success is unreachable",
            doomed.len()
        );
    }

    // Schedule counting and trace classification need a finite interleaving
    // space, i.e. an acyclic state graph. Structural cells (the ring's
    // fork drift) make it cyclic: counting traces then means rewriting
    // modulo the structural layer, which the engine does not yet do.
    if !is_acyclic(&states[0].0, &succs) {
        println!("   interleavings: infinite — the drift cells make the state graph cyclic;");
        println!("              trace counting here means rewriting modulo the structural");
        println!("              layer (see the trs-convergence open question)");
        println!();
        return;
    }

    let mut memo = HashMap::new();
    let total = count_interleavings(&states[0].0, &succs, &mut memo);
    println!("   interleavings: {} (exact)", thousands(total));

    let mut classes = Vec::new();
    let walked_all = total <= FULL_WALK_BOUND;
    if walked_all {
        walk_all(&ctx, &mut ctx.engine_at(&init), is_success.as_ref(), &mut classes);
    } else {
        sample(&ctx, &init, is_success.as_ref(), &mut classes);
    }
    classes.sort_by(|a, b| b.interleavings.cmp(&a.interleavings));

    let (count_word, qualifier) = if walked_all {
        ("interleavings", "every interleaving walked".to_owned())
    } else {
        ("samples", format!("lower bound from {SAMPLES} random interleavings"))
    };
    let at_least = if walked_all { "" } else { ">= " };
    println!("   traces:    {at_least}{}  ({qualifier})", classes.len());
    for c in classes.iter().take(CLASS_ROWS) {
        let verdict = if c.success { "success" } else { "DEADLOCK" };
        println!(
            "     {:>10} {count_word} -> {}  [{}]",
            thousands(c.interleavings),
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
        ring(),
        corridor(4),
        corridor(10),
        corridor(40),
    ] {
        run(&bench);
    }
}
