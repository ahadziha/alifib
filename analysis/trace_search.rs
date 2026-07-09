//! Reachable-state and deadlock analysis of the dining philosophers —
//! a 5-seat ring as a 2-diagram — using the engine's session API alone.
//!
//! Run: cargo run -p alifib --release --example trace_search

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alifib::interactive::engine::{eval_diagram_expr, load_type_context, RewriteEngine};
use alifib::interpreter::GlobalStore;
use alifib::output::render_diagram;
use alifib::{Complex, Diagram};

struct Ctx {
    store: Arc<GlobalStore>,
    complex: Arc<Complex>,
    path: String,
    type_name: String,
}

impl Ctx {
    fn load(file: &str, type_name: &str) -> Self {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(file)
            .to_string_lossy()
            .into_owned();
        let (store, complex, path) =
            load_type_context(&file, type_name).expect("benchmark file should load");
        Ctx { store, complex, path, type_name: type_name.to_owned() }
    }

    fn eval(&self, expr: &str) -> Diagram {
        eval_diagram_expr(&self.store, &self.complex, &self.path, expr)
            .expect("diagram expression should evaluate")
    }

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

/// BFS over reachable states, keyed by rendered label (canonical for these
/// words). Successor lists keep duplicates; a label collision must be an
/// isomorphism, which is asserted.
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

fn main() {
    let ctx = Ctx::load("examples/DiningPhilosophers.ali", "Table");
    let init = ctx.eval("5places");
    println!("== Dining philosophers, 5-seat ring (2-diagram) ==");
    println!("   initial:   {}", ctx.label(&init));

    let (states, succs) = state_graph(&ctx, &init);
    println!("   states:    {}", states.len());

    let deadlocks: Vec<&String> =
        states.iter().filter(|(l, _)| succs[l].is_empty()).map(|(l, _)| l).collect();
    println!("   deadlocks: {} terminal state(s)", deadlocks.len());
    for l in &deadlocks {
        println!("     {l}");
    }
}
