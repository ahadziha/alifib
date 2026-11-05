//! Stateful rewrite engine: holds session state in memory for incremental
//! step/apply without re-interpreting the source file.

use crate::aux::{GlobalId, Tag};
use crate::core::complex::Complex;
use crate::core::diagram::{Diagram, Sign};
use crate::core::matching::{
    FamilyMember, MatchResult, RulePattern,
    build_rule_patterns,
    confirm_candidate,
    for_each_rule_candidate,
    greedy_parallel_auto_step,
    try_family_from_members,
};
use crate::core::paste_tree::{flatten_at, is_pseudo_normal, pseudo_normalise, realise_tree, top_generators};
use crate::interpreter::GlobalStore;
use rand_xoshiro::rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;
use std::collections::HashMap;
use std::sync::Arc;

/// One applied rewrite step in the session history (display only — there is no
/// replay).  `choice` holds the rewrite-menu indices the user picked, or `None`
/// for a step with no manual choice (an `auto` step, or one recovered by
/// `resume`).
pub(crate) struct HistoryEntry {
    pub(crate) rule_name: String,
    pub(crate) choice: Option<Vec<usize>>,
}

/// Cached assembled proof diagram at a known step count.
///
/// Built on the first proof view request. Rather than rebuilding from scratch
/// each time, we cache the last assembled proof and extend it incrementally.
struct ProofCache {
    snapshot: Diagram,
    at_step: usize,
}

/// Stateful rewrite session engine.
///
/// Load once with [`RewriteEngine::from_store`] or [`RewriteEngine::resume`];
/// then use [`step`](RewriteEngine::step), [`undo`](RewriteEngine::undo), and
/// the accessor methods to drive the session without re-interpreting the
/// source file.
pub struct RewriteEngine {
    // Immutable context (loaded once)
    store: Arc<GlobalStore>,
    type_complex: Arc<Complex>,
    initial_diagram: Diagram,
    target_diagram: Option<Diagram>,
    backward: bool,

    // Mutable session state
    current_diagram: Diagram,
    /// The individual (n+1)-dimensional rewrite steps applied so far.
    /// The full proof diagram is only built on demand — see [`assemble_proof`].
    /// Entries beyond `active_len` form the redo buffer.
    steps: Vec<Diagram>,
    history: Vec<HistoryEntry>,
    /// How many entries in `steps`/`history` are active. The rest are the
    /// redo buffer, discarded only when a genuinely new step is applied.
    active_len: usize,
    rewrites: Vec<MatchResult>,
    parallel: bool,
    rng: Xoshiro256PlusPlus,

    /// Per-rule precomputed pattern data (normalised input or output boundary
    /// and embedding into the rule's shape, depending on [`backward`]).
    /// Built once when the engine is constructed and reused for every
    /// [`for_each_rule_candidate`] call.
    rule_patterns: HashMap<String, RulePattern>,

    /// Incremental proof cache, initialised on the first proof view request.
    proof_cache: Option<ProofCache>,

    // Metadata (for session file persistence)
    source_file: String,
    type_name: String,
    initial_diagram_name: String,
    target_diagram_name: Option<String>,
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Resolve a type name in a store module to its global id.
pub fn resolve_type_id(
    store: &GlobalStore,
    canonical_path: &str,
    type_name: &str,
) -> Result<GlobalId, String> {
    let module_complex = store
        .find_module(canonical_path)
        .ok_or_else(|| format!("module '{}' not found in store", canonical_path))?;

    match module_complex.find_generator(type_name) {
        Some((Tag::Global(gid), _)) => Ok(*gid),
        Some((Tag::Local(_) | Tag::Hole(_), _)) => Err(format!("'{}' is a local cell, not a type", type_name)),
        None => Err(format!("type '{}' not found", type_name)),
    }
}

/// Resolve a type name in a store module to its [`Complex`].
pub fn resolve_type(
    store: &GlobalStore,
    canonical_path: &str,
    type_name: &str,
) -> Result<Arc<Complex>, String> {
    let gid = resolve_type_id(store, canonical_path, type_name)?;
    store
        .find_type(gid)
        .ok_or_else(|| format!("type entry for '{}' not found", type_name))
        .map(|e| Arc::clone(&e.complex))
}

fn seeded_rng() -> Xoshiro256PlusPlus {
    #[cfg(not(target_arch = "wasm32"))]
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    #[cfg(target_arch = "wasm32")]
    let seed = {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    };
    Xoshiro256PlusPlus::seed_from_u64(seed)
}

fn random(rng: &mut Xoshiro256PlusPlus, upper_bound: usize) -> usize {
    (rng.next_u64() % upper_bound as u64) as usize
}

/// Resolve a source or target diagram from either a name or a diagram expression.
///
/// First tries a fast named lookup in `type_complex` and the module complex at
/// `module_key` in `store`. If neither succeeds, parses `expr` as a diagram
/// expression and evaluates it against `type_complex` using the interpreter.
///
/// This means `source <name>` and `source f g` both work at the REPL.
pub fn eval_diagram_expr(
    store: &Arc<GlobalStore>,
    type_complex: &Complex,
    module_key: &str,
    expr: &str,
) -> Result<Diagram, String> {
    // Fast path: named lookup.
    if let Some(d) = type_complex.find_diagram(expr) {
        return Ok(d.clone());
    }
    if let Some(d) = store.find_module(module_key).and_then(|m| m.find_diagram(expr)) {
        return Ok(d.clone());
    }
    // Slow path: parse and interpret as a diagram expression.
    let ast = crate::language::parse_diagram(expr)
        .map_err(|e| format!("'{}' is not a diagram name or valid expression: {}", expr, e))?;
    let ctx = crate::interpreter::Context::new_with_resolutions(
        module_key.to_owned(),
        std::sync::Arc::new(crate::aux::loader::ModuleResolutions::empty()),
        Arc::clone(store),
        Arc::new(String::new()),
    );
    let (diagram_opt, interp_result) = crate::interpreter::interpret_diagram(&ctx, type_complex, &ast);
    if interp_result.has_errors() {
        let msgs: Vec<String> = interp_result.errors.iter()
            .map(|e| format!("{}", e))
            .collect();
        return Err(format!("'{}': {}", expr, msgs.join("; ")));
    }
    diagram_opt.ok_or_else(|| format!("'{}' did not produce a diagram", expr))
}

/// Check that `source` and `target` are parallel: same dimension, and (for
/// dim > 0) isomorphic input and output boundaries.
fn check_parallel(source: &Diagram, target: &Diagram) -> Result<(), String> {
    if source.dim() != target.dim() {
        return Err(format!(
            "source has dimension {} but target has dimension {}",
            source.top_dim(), target.top_dim(),
        ));
    }
    if source.dim() <= 0 {
        return Ok(());
    }
    let k = source.top_dim() - 1;
    let src_in = Diagram::boundary_normal(Sign::Input, k, source)
        .map_err(|e| format!("source input boundary: {}", e))?;
    let tgt_in = Diagram::boundary_normal(Sign::Input, k, target)
        .map_err(|e| format!("target input boundary: {}", e))?;
    if !Diagram::isomorphic(&src_in, &tgt_in) {
        return Err("source and target are not parallel: input boundaries do not match".to_owned());
    }
    let src_out = Diagram::boundary_normal(Sign::Output, k, source)
        .map_err(|e| format!("source output boundary: {}", e))?;
    let tgt_out = Diagram::boundary_normal(Sign::Output, k, target)
        .map_err(|e| format!("target output boundary: {}", e))?;
    if !Diagram::isomorphic(&src_out, &tgt_out) {
        return Err("source and target are not parallel: output boundaries do not match".to_owned());
    }
    Ok(())
}

/// Confirm each candidate individually and collect all successes (non-parallel).
fn collect_confirmed_matches(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Vec<MatchResult>, String> {
    let mut results = Vec::new();
    for_each_rule_candidate(type_complex, rule_patterns, current, |cand| {
        if let Some(mr) = confirm_candidate(&cand, type_complex, current, rule_patterns) {
            results.push(mr);
        }
        false
    })?;
    Ok(results)
}

/// Find the first confirmed match lazily (non-parallel auto).
fn find_first_match(
    type_complex: &Complex,
    rule_patterns: &HashMap<String, RulePattern>,
    current: &Diagram,
) -> Result<Option<MatchResult>, String> {
    let mut result = None;
    for_each_rule_candidate(type_complex, rule_patterns, current, |cand| {
        if let Some(mr) = confirm_candidate(&cand, type_complex, current, rule_patterns) {
            result = Some(mr);
            true
        } else {
            false
        }
    })?;
    Ok(result)
}

// ── Step history ─────────────────────────────────────────────────────────────

/// A linear history with a redo buffer, addressed by step count: what `undo`
/// and `redo` move through, whether a step is a rewrite or a 0-cell choice.
pub trait Steps {
    /// The number of steps applied.
    fn applied(&self) -> usize;
    /// The number of steps applied or kept for redo.
    fn recorded(&self) -> usize;
    /// Move to `step ≤ recorded()`, keeping later steps for redo.
    fn seek(&mut self, step: usize) -> Result<(), String>;

    fn can_redo(&self) -> bool {
        self.applied() < self.recorded()
    }

    /// Undo the last applied step.
    fn undo(&mut self) -> Result<(), String> {
        match self.applied() {
            0 => Err("nothing to undo".to_owned()),
            applied => self.seek(applied - 1),
        }
    }

    /// Redo the last undone step.
    fn redo(&mut self) -> Result<(), String> {
        if !self.can_redo() {
            return Err("nothing to redo".to_owned());
        }
        self.seek(self.applied() + 1)
    }

    /// Undo back to `step` (0 = fully undone); a no-op at the current step.
    fn undo_to(&mut self, step: usize) -> Result<(), String> {
        let applied = self.applied();
        if step > applied {
            return Err(format!("cannot undo to step {}: only {} step(s) applied", step, applied));
        }
        if step == applied { Ok(()) } else { self.seek(step) }
    }

    /// Redo forward to `step`; a no-op at or before the current step.
    fn redo_to(&mut self, step: usize) -> Result<(), String> {
        let recorded = self.recorded();
        if step > recorded {
            return Err(format!("cannot redo to step {}: only {} step(s) in history", step, recorded));
        }
        if step <= self.applied() { Ok(()) } else { self.seek(step) }
    }
}

impl Steps for RewriteEngine {
    fn applied(&self) -> usize { self.active_len }
    fn recorded(&self) -> usize { self.history.len() }

    fn seek(&mut self, step: usize) -> Result<(), String> {
        self.current_diagram = match step {
            0 => self.initial_diagram.clone(),
            _ => self.diagram_after_step(step - 1)?,
        };
        self.active_len = step;
        self.refresh_rewrites()
    }
}

// ── Constructor impls ─────────────────────────────────────────────────────────

impl RewriteEngine {
    /// Create a fresh session from an already-loaded store and type complex.
    ///
    /// `initial_diagram_name` and `target_diagram_name` may be either plain diagram
    /// names (resolved against `type_complex` and the module at `source_file`) or
    /// full diagram expressions in the alifib language (e.g. `"f g"` or `"(f #0 g)"`).
    /// This constructor is used by the interactive REPL and the web backends.
    pub fn from_store(
        store: Arc<GlobalStore>,
        type_complex: Arc<Complex>,
        initial_diagram_name: &str,
        target_diagram_name: Option<&str>,
        source_file: String,
        type_name: String,
        backward: bool,
    ) -> Result<Self, String> {
        let initial_diagram =
            eval_diagram_expr(&store, &type_complex, &source_file, initial_diagram_name)?;
        let target_diagram = target_diagram_name
            .map(|expr| eval_diagram_expr(&store, &type_complex, &source_file, expr))
            .transpose()?;
        Self::from_diagrams(
            store, type_complex, initial_diagram, target_diagram, source_file, type_name,
            initial_diagram_name.to_owned(), target_diagram_name.map(str::to_owned), backward,
        )
    }

    /// Create a fresh session from already-evaluated initial/target diagrams.
    ///
    /// Like [`from_store`](Self::from_store) but skips diagram-expression parsing —
    /// used when the diagrams are known directly, as in hole-filling (where they
    /// are the realised boundaries of the hole).  `*_name` are kept only as
    /// display/metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn from_diagrams(
        store: Arc<GlobalStore>,
        type_complex: Arc<Complex>,
        initial_diagram: Diagram,
        target_diagram: Option<Diagram>,
        source_file: String,
        type_name: String,
        initial_diagram_name: String,
        target_diagram_name: Option<String>,
        backward: bool,
    ) -> Result<Self, String> {
        let initial_diagram = Diagram::normal(&initial_diagram);
        if let Some(ref target) = target_diagram {
            check_parallel(&initial_diagram, target)?;
        }

        let rule_patterns = build_rule_patterns(&type_complex, initial_diagram.top_dim(), backward)?;
        let rewrites = collect_confirmed_matches(
            &type_complex, &rule_patterns, &initial_diagram,
        )?;

        let engine = Self {
            current_diagram: initial_diagram.clone(),
            steps: Vec::new(),
            history: Vec::new(),
            active_len: 0,
            rewrites,
            parallel: true,
            backward,
            rng: seeded_rng(),
            rule_patterns,
            proof_cache: None,
            source_file,
            type_name,
            initial_diagram_name,
            target_diagram_name,
            store,
            type_complex,
            initial_diagram,
            target_diagram,
        };
        debug_assert!(engine.current_diagram.is_normal());
        Ok(engine)
    }

    /// Resume a session from a proof diagram: decompose it into the rewrite
    /// steps that produce it, with those steps already applied.
    ///
    /// `proof_name` must resolve to a diagram `d` of dimension `n+1 > 0`.
    /// Pseudo-normalising its paste tree makes every dimension-`n` paste
    /// outermost; flattening that chain expresses `d = d₁ #ₙ … #ₙ dₘ` with each
    /// `dᵢ` pasted below dimension `n`.  The `dᵢ` become the rewrite steps —
    /// reversed in backward mode — and each step's history label lists the
    /// `(n+1)`-dimensional generators it applies.
    ///
    /// The session starts at `d`'s input boundary (forward) or output boundary
    /// (backward), with every step applied, so the current diagram is the
    /// opposite boundary and the assembled proof is `d`.  `target` is the goal
    /// to keep working toward — the *original* session's target, never inferred
    /// from `d`'s own boundary — or `None` to resume open-ended.  The result
    /// behaves like any other session (undo, redo, continue).
    pub fn resume(
        store: Arc<GlobalStore>,
        type_complex: Arc<Complex>,
        proof_name: &str,
        target_name: Option<&str>,
        source_file: String,
        type_name: String,
        backward: bool,
    ) -> Result<Self, String> {
        let proof = eval_diagram_expr(&store, &type_complex, &source_file, proof_name)?;
        if proof.dim() < 1 {
            return Err(format!(
                "'{}' has dimension {}; resume needs a proof diagram of dimension > 0",
                proof_name, proof.dim().max(0),
            ));
        }
        let top = proof.top_dim(); // n + 1
        let n = top - 1;

        let tree = proof.tree(Sign::Input, top)
            .ok_or_else(|| format!("'{}' has no paste tree", proof_name))?;
        let normalised = pseudo_normalise(tree, &type_complex).map_err(|e| e.to_string())?;
        debug_assert!(is_pseudo_normal(&normalised), "pseudo_normalise must return a pseudo-normal tree");

        // Each maximal subtree pasted below dimension `n` is one rewrite step.
        let mut steps: Vec<Diagram> = Vec::new();
        let mut history: Vec<HistoryEntry> = Vec::new();
        for sub in flatten_at(&normalised, n) {
            let step = realise_tree(&sub, &type_complex).map_err(|e| e.to_string())?;
            let rule_name = top_generators(&sub, &type_complex)
                .map_err(|e| e.to_string())?
                .iter()
                .map(|tag| type_complex.find_generator_by_tag(tag)
                    .cloned()
                    .unwrap_or_else(|| format!("{}", tag)))
                .collect::<Vec<_>>()
                .join(", ");
            steps.push(step);
            history.push(HistoryEntry { rule_name, choice: None });
        }
        if backward {
            steps.reverse();
            history.reverse();
        }

        // Forward starts at the input boundary and steps along target
        // boundaries; backward is the dual.
        let (initial_sign, step_sign) = if backward {
            (Sign::Output, Sign::Input)
        } else {
            (Sign::Input, Sign::Output)
        };
        let initial_diagram = Diagram::boundary_normal(initial_sign, n, &proof)
            .map_err(|e| format!("initial boundary: {}", e))?;
        let current_diagram = match steps.last() {
            Some(last) => Diagram::boundary_normal(step_sign, n, last)
                .map_err(|e| format!("current boundary: {}", e))?,
            None => initial_diagram.clone(),
        };

        // The target is the supplied goal — never `d`'s own boundary.
        let target_diagram = match target_name {
            Some(t) => Some(eval_diagram_expr(&store, &type_complex, &source_file, t)?),
            None => None,
        };
        if let Some(ref target) = target_diagram {
            check_parallel(&initial_diagram, target)?;
        }

        let rule_patterns = build_rule_patterns(&type_complex, n, backward)?;
        let rewrites = collect_confirmed_matches(&type_complex, &rule_patterns, &current_diagram)?;
        let active_len = steps.len();

        let engine = Self {
            current_diagram,
            steps,
            history,
            active_len,
            rewrites,
            parallel: true,
            backward,
            rng: seeded_rng(),
            rule_patterns,
            proof_cache: None,
            source_file,
            type_name,
            initial_diagram_name: proof_name.to_owned(),
            target_diagram_name: target_name.map(str::to_owned),
            store,
            type_complex,
            initial_diagram,
            target_diagram,
        };
        debug_assert!(engine.current_diagram.is_normal());
        Ok(engine)
    }

}

// ── Mutating operations ───────────────────────────────────────────────────────

impl RewriteEngine {
    pub(super) fn advance(&self, step: &Diagram) -> Result<Diagram, String> {
        let n = self.initial_diagram.top_dim();
        Diagram::boundary_normal(self.step_sign(), n, step)
            .map_err(|e| format!("step boundary failed: {}", e))
    }

    fn diagram_after_step(&self, step_idx: usize) -> Result<Diagram, String> {
        self.advance(&self.steps[step_idx])
    }

    /// Recompute available rewrites (individual matches for manual selection).
    /// Parallel mode only affects `auto()`, not the manual rewrites list.
    fn refresh_rewrites(&mut self) -> Result<(), String> {
        debug_assert!(self.current_diagram.is_normal());
        self.rewrites = collect_confirmed_matches(
            &self.type_complex, &self.rule_patterns, &self.current_diagram,
        )?;
        Ok(())
    }

    fn total_choices(&self) -> usize {
        self.rewrites.len()
    }

    /// Discard the redo buffer, keeping only the active prefix.
    fn truncate_redo(&mut self) {
        self.steps.truncate(self.active_len);
        self.history.truncate(self.active_len);
        if self.proof_cache.as_ref().is_some_and(|cache| cache.at_step > self.active_len) {
            self.proof_cache = None;
        }
    }

    /// Apply the rewrite at index `choice`. Records the step and advances
    /// the current diagram. Discards any redo buffer.
    pub fn step(&mut self, choice: usize) -> Result<&str, String> {
        let total = self.total_choices();
        if choice >= total {
            return Err(format!(
                "choice {} out of range ({} rewrite(s) available)",
                choice, total,
            ));
        }

        let pr = &self.rewrites[choice];
        let names: Vec<&str> = pr.members.iter()
            .map(|m| m.rule_name.as_str())
            .collect();
        let rule_name = names.join(", ");
        let step = pr.step.clone();

        let new_current = self.advance(&step)?;

        self.truncate_redo();
        self.current_diagram = new_current;
        self.steps.push(step);

        self.history.push(HistoryEntry {
            rule_name: rule_name.clone(),
            choice: Some(vec![choice]),
        });
        self.active_len = self.steps.len();

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().rule_name)
    }

    /// Apply multiple rewrites in parallel by their indices.
    ///
    /// Checks that the selected matches are pairwise disjoint, then builds a
    /// combined parallel step via multi-pushout. Discards any redo buffer.
    pub fn step_multi(&mut self, choices: &[usize]) -> Result<&str, String> {
        let total = self.total_choices();
        for &c in choices {
            if c >= total {
                return Err(format!(
                    "choice {} out of range ({} rewrite(s) available)", c, total,
                ));
            }
        }
        let mut sorted = choices.to_vec();
        sorted.sort_unstable();
        if sorted.windows(2).any(|w| w[0] == w[1]) {
            return Err("duplicate choice index".to_string());
        }

        for i in 0..choices.len() {
            for j in (i + 1)..choices.len() {
                let a = &self.rewrites[choices[i]].image_positions;
                let b = &self.rewrites[choices[j]].image_positions;
                if a.iter().any(|x| b.contains(x)) {
                    return Err(format!(
                        "rewrites {} and {} overlap", choices[i], choices[j],
                    ));
                }
            }
        }

        let members: Vec<FamilyMember> = choices.iter().map(|&c| {
            let pr = &self.rewrites[c];
            pr.members[0].clone()
        }).collect();

        let names: Vec<&str> = members.iter().map(|m| m.rule_name.as_str()).collect();
        let rule_name = names.join(", ");

        let pr = try_family_from_members(
            members, &self.type_complex, &self.current_diagram, &self.rule_patterns,
        ).ok_or("parallel rewrite construction failed")?;

        let new_current = self.advance(&pr.step)?;

        self.truncate_redo();
        self.current_diagram = new_current;
        self.steps.push(pr.step);
        self.history.push(HistoryEntry {
            rule_name,
            choice: Some(choices.to_vec()),
        });
        self.active_len = self.steps.len();

        self.refresh_rewrites()?;

        Ok(&self.history.last().unwrap().rule_name)
    }

    /// Apply up to `max_steps` rewrites automatically, always picking the
    /// first available candidate. Discards any redo buffer.
    ///
    /// In parallel mode, uses a greedy algorithm to find a compatible family
    /// and apply it in one step.  In non-parallel mode, applies the first
    /// confirmed individual match.
    pub fn auto(&mut self, max_steps: usize) -> Result<(usize, Option<&'static str>), String> {
        self.truncate_redo();
        let mut applied = 0usize;
        let stop_reason: Option<&'static str>;

        loop {
            if self.target_reached() {
                stop_reason = Some("target reached");
                break;
            }
            if applied >= max_steps {
                stop_reason = None;
                break;
            }

            let pr = if self.parallel {
                greedy_parallel_auto_step(
                    &self.type_complex, &self.rule_patterns, &self.current_diagram,
                )?
            } else {
                find_first_match(
                    &self.type_complex, &self.rule_patterns, &self.current_diagram,
                )?
            };
            let Some(pr) = pr else {
                stop_reason = Some("no rewrites available");
                break;
            };

            let names: Vec<&str> = pr.members.iter()
                .map(|m| m.rule_name.as_str())
                .collect();
            let rule_name = names.join(", ");
            let is_parallel = self.parallel && pr.members.len() > 1;

            let new_current = self.advance(&pr.step)?;

            self.current_diagram = new_current;
            self.steps.push(pr.step);
            self.history.push(HistoryEntry {
                rule_name,
                choice: if is_parallel { None } else { Some(vec![0]) },
            });
            applied += 1;
        }

        self.active_len = self.steps.len();
        self.refresh_rewrites()?;

        Ok((applied, stop_reason))
    }

    /// Apply randomly selected available rewrites.
    pub fn random(&mut self, max_steps : usize) -> Result<(usize, Option<&'static str>), String> {
        for applied in 0..max_steps {
            if self.target_reached() {
                return Ok((applied, Some("target reached")));
            }

            // TODO: Speed this up by not using `step`, but first picking a
            // random rule, and then finding a random instance
            let total = self.total_choices();
            if total == 0 {
                return Ok((applied, Some("no rewrites available")));
            }
            let choice = random(&mut self.rng, total);
            self.step(choice)?;
        }

        Ok((max_steps, None))
    }

    /// Toggle parallel rewrite mode on or off. Only affects `auto()`.
    pub fn set_parallel(&mut self, on: bool) {
        self.parallel = on;
    }
}

// ── Read-only accessors ───────────────────────────────────────────────────────

impl RewriteEngine {
    /// Number of active rewrite steps (excludes the redo buffer).
    pub fn step_count(&self) -> usize { self.active_len }

    /// The n-diagram being actively rewritten (changes after each [`step`](Self::step)).
    pub fn current_diagram(&self) -> &Diagram { &self.current_diagram }

    /// The fixed n-diagram that the session started from.
    pub fn initial_diagram(&self) -> &Diagram { &self.initial_diagram }

    /// The declared goal diagram, or `None` if no target was specified.
    pub fn target_diagram(&self) -> Option<&Diagram> { self.target_diagram.as_ref() }

    /// Whether this session uses backward rewriting (output → input).
    pub fn backward(&self) -> bool { self.backward }

    /// The boundary sign used to extract the new current diagram from a step.
    /// Forward: `Output`. Backward: `Input`.
    fn step_sign(&self) -> Sign {
        if self.backward { Sign::Input } else { Sign::Output }
    }

    /// The active (n+1)-dimensional rewrite steps, in order.
    ///
    /// Each step is a single rewrite — they are *not* pasted together here.
    /// Use [`assemble_proof`](Self::assemble_proof) to build the full proof
    /// diagram (only done when storing or typechecking).
    pub fn steps(&self) -> &[Diagram] { &self.steps[..self.active_len] }

    /// Paste the recorded rewrite steps together into the full (n+1)-dimensional
    /// proof diagram.  With no steps the proof is the initial diagram itself (the
    /// identity / degenerate proof) — the same thing `store` and `proof_diagram`
    /// record at step 0.
    ///
    /// This is the one place where the "diagram up to now" is actually built —
    /// call it only when the assembled proof is needed (storing, typechecking,
    /// or rendering the final proof label).
    pub fn assemble_proof(&self) -> Result<Diagram, String> {
        let n = self.initial_diagram.top_dim();
        let active = &self.steps[..self.active_len];
        let mut iter = active.iter();
        let Some(first) = iter.next() else {
            return Ok(self.initial_diagram.clone());
        };
        let mut acc = first.clone();
        for step in iter {
            let (left, right) = if self.backward { (step, &acc) } else { (&acc, step) };
            acc = Diagram::paste(n, left, right)
                .map_err(|e| format!("compose step failed: {}", e))?;
        }
        Ok(acc)
    }

    /// The rewrites applicable to the current diagram, precomputed after
    /// each [`step`](Self::step) or [`undo`](Self::undo).
    pub fn rewrites(&self) -> &[MatchResult] { &self.rewrites }

    /// Whether parallel rewrite mode is currently enabled.
    pub fn parallel(&self) -> bool { self.parallel }

    /// The interpreter's global store, shared read-only across the session.
    pub fn store(&self) -> &GlobalStore { &self.store }

    /// Clone the `Arc` to the interpreter's global store.  Used by callers
    /// that need to keep an independent handle (e.g. the web adapter, which
    /// resyncs its cached store after a successful `register_proof`).
    pub fn store_arc(&self) -> Arc<GlobalStore> { Arc::clone(&self.store) }

    /// The type complex whose (n+1)-generators are the rewrite rules.
    pub fn type_complex(&self) -> &Complex { &self.type_complex }

    /// Iterate over the active history entries in order.
    pub(crate) fn history(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.history[..self.active_len].iter()
    }

    /// Render the active proof as a re-parseable *source expression* — one
    /// rewrite step per line, `d₁ #ₙ … #ₙ dₘ` in session order (reversed for
    /// backward). A zero-step proof is the identity on the initial diagram,
    /// rendered as that diagram.
    ///
    /// This is the durable, step-structured form: what `store` writes into the
    /// `.ali` and what `resume` consumes.
    pub fn proof_expr(&self) -> String {
        let render = |d: &Diagram| crate::output::render_diagram(d, &self.type_complex);
        let steps = self.steps();
        if steps.is_empty() {
            return render(&self.initial_diagram);
        }
        let rendered: Vec<String> = if self.backward {
            steps.iter().rev().map(render).collect()
        } else {
            steps.iter().map(render).collect()
        };
        rendered.join(&format!("\n#{} ", self.initial_diagram.top_dim()))
    }

    /// Canonical path to the `.ali` source file for this session.
    pub fn source_file(&self) -> &str { &self.source_file }

    /// Name of the type whose generators serve as rewrite rules.
    pub fn type_name(&self) -> &str { &self.type_name }

    /// Name of the source n-diagram (as declared in the type or module).
    pub fn initial_diagram_name(&self) -> &str { &self.initial_diagram_name }

    /// Name of the target diagram, or `None` if no goal was declared.
    pub fn target_diagram_name(&self) -> Option<&str> { self.target_diagram_name.as_deref() }

    // ── Proof cache ──────────────────────────────────────────────────────

    /// Return the assembled proof diagram, extending the cached prefix when it
    /// is still active and assembling afresh otherwise. At step 0, the source.
    pub fn proof_diagram(&mut self) -> Result<Diagram, String> {
        let snapshot = match self.proof_cache.take() {
            Some(cache) if cache.at_step == self.active_len => cache.snapshot,
            // A step-0 snapshot is the n-dimensional source, not a prefix proof.
            Some(cache) if 0 < cache.at_step && cache.at_step < self.active_len => {
                let n = self.initial_diagram.top_dim();
                self.steps[cache.at_step..self.active_len].iter()
                    .try_fold(cache.snapshot, |acc, step| {
                        let (left, right) = if self.backward { (step, &acc) } else { (&acc, step) };
                        Diagram::paste(n, left, right)
                            .map_err(|e| format!("proof cache extend failed: {}", e))
                    })?
            }
            _ => self.assemble_proof()?,
        };
        self.proof_cache = Some(ProofCache { snapshot: snapshot.clone(), at_step: self.active_len });
        Ok(snapshot)
    }

    /// Returns true when the current diagram is isomorphic to the target — including
    /// at step 0, where the initial diagram already meets the target (the identity /
    /// degenerate proof, which is a perfectly valid filler).
    pub fn target_reached(&self) -> bool {
        self.target_diagram.as_ref()
            .map(|t| Diagram::isomorphic(&self.current_diagram, t))
            .unwrap_or(false)
    }

    /// Typecheck the current proof diagram.
    ///
    /// Runs two checks:
    /// 1. Initial boundary: the proof's boundary on the initial side is
    ///    isomorphic to `initial_diagram` (source in forward, target in backward).
    /// 2. Round-trip: sourcefies the proof, re-interprets it through the interpreter, and
    ///    confirms the result is isomorphic to the constructed proof.
    ///
    /// Returns `Ok(())` if both pass, `Err(message)` on any failure.
    pub fn typecheck_proof(&self) -> Result<(), String> {
        let assembled = self.assemble_proof()?;
        let diagram = &assembled;

        let n = self.initial_diagram.top_dim();
        let check_sign = if self.backward { Sign::Output } else { Sign::Input };
        let boundary = Diagram::boundary(check_sign, n, diagram)
            .map_err(|e| format!("boundary check failed: {}", e))?;
        if !Diagram::isomorphic(&boundary, &self.initial_diagram) {
            return Err(format!(
                "proof boundary does not match initial diagram '{}' — \
                 this is a bug in the rewrite engine",
                self.initial_diagram_name,
            ));
        }

        // Check 2: round-trip through the interpreter.
        let source_expr = crate::output::render_diagram(diagram, &self.type_complex);
        let ast = crate::language::parse_diagram(&source_expr)
            .map_err(|e| format!("sourcefier produced unparseable expression '{}': {}", source_expr, e))?;
        let ctx = crate::interpreter::Context::new_with_resolutions(
            self.source_file.clone(),
            std::sync::Arc::new(crate::aux::loader::ModuleResolutions::empty()),
            Arc::clone(&self.store),
            Arc::new(String::new()),
        );
        let (interp_opt, interp_result) =
            crate::interpreter::interpret_diagram(&ctx, &self.type_complex, &ast);
        if interp_result.has_errors() {
            let msgs: Vec<String> = interp_result.errors.iter()
                .map(|e| format!("{}", e))
                .collect();
            return Err(format!(
                "interpreter rejected proof expression '{}': {}",
                source_expr, msgs.join("; "),
            ));
        }
        let interp = interp_opt.ok_or_else(|| format!(
            "interpreter produced no diagram for expression '{}'", source_expr,
        ))?;
        if !Diagram::isomorphic(&interp, diagram) {
            return Err(format!(
                "round-trip check failed: expression '{}' does not reconstruct the proof — \
                 this is a bug in the sourcefier",
                source_expr,
            ));
        }

        Ok(())
    }

    /// Register the current running proof as a named diagram (let-binding)
    /// in the type complex, returning the updated store for the caller to share.
    pub fn register_proof(&mut self, name: &str) -> Result<Arc<GlobalStore>, String> {
        let diagram = self.proof_diagram()?;

        if self.type_complex.name_in_use(name) || self.type_complex.find_generator(name).is_some() {
            return Err(format!("name '{}' is already in use", name));
        }

        let type_gid = resolve_type_id(&self.store, &self.source_file, &self.type_name)?;

        Arc::make_mut(&mut self.store)
            .modify_type_complex(type_gid, |cx| {
                cx.add_diagram(name.to_owned(), diagram);
            })
            .ok_or_else(|| format!("type '{}' not found in store", self.type_name))?;

        self.type_complex = self.store
            .find_type(type_gid)
            .map(|e| Arc::clone(&e.complex))
            .ok_or_else(|| format!("type '{}' missing after registration", self.type_name))?;

        // No need to recompute rewrites — a let-binding doesn't add new rewrite rules.

        Ok(Arc::clone(&self.store))
    }
}

#[cfg(test)]
mod rewrite_order_independent_of_route {
    use super::*;
    use crate::aux::loader::Loader;
    use crate::interactive::fill::{self, FillSession};
    use crate::output::render_diagram;
    use std::path::PathBuf;

    // Preserve candidate and member order: this is exactly what `apply n` indexes.
    fn menu(engine: &RewriteEngine) -> Vec<(Vec<String>, Vec<usize>)> {
        engine.rewrites().iter().map(|candidate| {
            (
                candidate.members.iter().map(|member| member.rule_name.clone()).collect(),
                candidate.image_positions.clone(),
            )
        }).collect()
    }

    fn fresh_from_rendered(engine: &RewriteEngine) -> RewriteEngine {
        let expr = render_diagram(engine.current_diagram(), &engine.type_complex);
        RewriteEngine::from_store(
            Arc::clone(&engine.store), Arc::clone(&engine.type_complex), &expr, None,
            engine.source_file.clone(), engine.type_name.clone(), engine.backward,
        ).expect("start from rendered current diagram")
    }

    fn assert_fresh_menu(engine: &RewriteEngine, route: &str) {
        let fresh = fresh_from_rendered(engine);
        assert!(Diagram::isomorphic(engine.current_diagram(), fresh.current_diagram()),
            "rendered endpoint differs after {route}");
        assert_eq!(menu(engine), menu(&fresh), "rewrite menu differs after {route}");
    }

    fn dining_philosophers() -> RewriteEngine {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("examples/DiningPhilosophers.ali").to_string_lossy().into_owned();
        let file = crate::interpreter::InterpretedFile::load(&Loader::default(vec![]), &path).expect("load DiningPhilosophers");
        let (store, path) = (file.state, file.path);
        let tc = resolve_type(&store, &path, "Table").expect("resolve Table");
        RewriteEngine::from_store(
            store, tc, "5places", None, path, "Table".to_owned(), false,
        ).expect("start Table from 5places")
    }

    fn dining_philosophers_manual_route() -> RewriteEngine {
        let mut engine = dining_philosophers();
        engine.step(3).expect("apply 3");
        engine.step(1).expect("apply 1");
        engine
    }

    #[test]
    fn dining_philosophers_steps_3_1() {
        let engine = dining_philosophers_manual_route();
        assert_fresh_menu(&engine, "step(3); step(1)");
    }

    #[test]
    fn dining_philosophers_resume_running_proof() {
        let engine = dining_philosophers_manual_route();
        let proof = engine.proof_expr();
        let resumed = RewriteEngine::resume(
            Arc::clone(&engine.store), Arc::clone(&engine.type_complex), &proof, None,
            engine.source_file.clone(), engine.type_name.clone(), false,
        ).expect("resume running proof");
        assert_eq!(resumed.step_count(), engine.step_count());
        assert!(Diagram::isomorphic(engine.current_diagram(), resumed.current_diagram()),
            "resume must reach the running proof's endpoint");
        let fresh = fresh_from_rendered(&engine);
        assert_eq!(menu(&resumed), menu(&fresh), "resumed menu differs from fresh endpoint");
        assert_eq!(menu(&resumed), menu(&engine), "resume changed the running menu");
    }

    #[test]
    fn dining_philosophers_undo() {
        let mut engine = dining_philosophers_manual_route();
        let endpoint = engine.current_diagram().clone();
        engine.step(0).expect("apply 0 after steps 3, 1");
        engine.undo().expect("undo step 0");
        assert_eq!(engine.step_count(), 2);
        assert!(Diagram::isomorphic(&endpoint, engine.current_diagram()),
            "undo must return to the steps 3, 1 endpoint");
        assert_fresh_menu(&engine, "step(3); step(1); step(0); undo()");
    }

    #[test]
    fn dining_philosophers_auto_2() {
        let mut engine = dining_philosophers();
        assert_eq!(engine.auto(2).expect("auto(2)").0, 2);
        // Parallel auto reaches its own endpoint, not the steps 3, 1 endpoint.
        assert_fresh_menu(&engine, "auto(2)");
    }

    #[test]
    fn dining_philosophers_backward() {
        let forward = dining_philosophers_manual_route();
        let mut backward = RewriteEngine::from_diagrams(
            Arc::clone(&forward.store), Arc::clone(&forward.type_complex),
            forward.current_diagram().clone(), None, forward.source_file.clone(),
            forward.type_name.clone(), "forward endpoint".to_owned(), None, true,
        ).expect("start backward from forward endpoint");
        assert_fresh_menu(&backward, "backward start from forward endpoint");
        backward.step(0).expect("apply a backward rewrite");
        assert_fresh_menu(&backward, "backward step(0)");
    }

    fn visit_fill_routes(engine: &mut RewriteEngine, route: &mut Vec<usize>, visited: &mut [usize; 4]) {
        visited[route.len()] += 1;
        assert_fresh_menu(engine, &format!("IdemFill route {route:?}"));
        if route.len() == 3 {
            return;
        }
        // Do not merge equal endpoints: every history must give the same menu.
        let choices = engine.rewrites().len();
        for choice in 0..choices {
            route.push(choice);
            engine.step(choice).expect("apply fill route choice");
            visit_fill_routes(engine, route, visited);
            engine.undo().expect("return to fill route prefix");
            route.pop();
        }
    }

    #[test]
    fn idem_fill_all_routes_up_to_depth_three() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/IdemFill.ali").to_string_lossy().into_owned();
        let file = crate::interpreter::InterpretedFile::load(&Loader::default(vec![]), &path).expect("load IdemFill");
        let program = crate::language::parse(&file.source).expect("parse IdemFill source");
        let (store, canonical) = (file.state, file.path);
        let (context, session) = fill::start_fill(&store, &program, &canonical, 0, false)
            .expect("start IdemFill hole");
        assert_eq!((context.type_name.as_str(), context.map_name.as_str(), context.source_name.as_str()),
            ("Idem", "H", "c"));
        let FillSession::Rewrite(mut engine) = session else {
            panic!("IdemFill's c hole must start a rewrite engine");
        };
        assert_eq!(engine.rewrites().len(), 4, "five ids have four idem redexes");
        assert!(engine.rewrites().iter().enumerate().any(|(i, a)| {
            engine.rewrites()[i + 1..].iter().any(|b| {
                a.image_positions.iter().any(|pos| b.image_positions.contains(pos))
            })
        }), "the fill fixture must have overlapping redexes");
        let mut visited = [0; 4];
        visit_fill_routes(&mut engine, &mut Vec::new(), &mut visited);
        assert_eq!(visited, [1, 4, 12, 24], "visit every route through three steps");
    }
}

#[cfg(test)]
mod resume_tests {
    use super::*;
    use crate::aux::loader::Loader;
    use crate::interpreter::InterpretedFile;
    use std::path::PathBuf;

    const TYPE: &str = "Assoc";

    /// Load the Assoc fixture and return its store + complex.
    fn load() -> (Arc<GlobalStore>, Arc<Complex>, String) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/Assoc.ali").to_string_lossy().into_owned();
        let file = InterpretedFile::load(&Loader::default(vec![]), &path)
            .expect("fixture should load");
        let store = Arc::clone(&file.state);
        let module = store.find_module(&file.path).expect("module");
        let gid = match module.find_generator(TYPE) {
            Some((Tag::Global(gid), _)) => *gid,
            _ => panic!("type not found"),
        };
        let tc = store.find_type(gid).expect("type entry").complex.clone();
        (store, tc, path)
    }

    fn resume(proof: &str, target: Option<&str>, backward: bool) -> (RewriteEngine, Diagram) {
        let (store, tc, path) = load();
        let proof_diag = eval_diagram_expr(&store, &tc, &path, proof).expect("proof diagram");
        let engine = RewriteEngine::resume(
            store, tc, proof, target, path, TYPE.to_owned(), backward,
        ).expect("resume");
        (engine, proof_diag)
    }

    /// `inter = (alpha alpha) #0 alpha` is not pseudo-normal: its pasting at
    /// dimension 0 sits above a dimension-1 paste.  Resuming it must
    /// pseudo-normalise first, yielding two interchange steps `[alpha·alpha, alpha]`
    /// whose reassembly reconstructs `inter`.
    #[test]
    fn reconstructs_non_pseudo_normal_proof() {
        let (engine, proof) = resume("inter", None, false);
        assert_eq!(engine.step_count(), 2, "interchange splits into two steps");
        assert_eq!(engine.history().count(), 2);
        // The first step pastes two parallel `alpha` rewrites, the second one.
        let labels: Vec<&str> = engine.history().map(|e| e.rule_name.as_str()).collect();
        assert_eq!(labels, vec!["alpha, alpha", "alpha"]);

        let assembled = engine.assemble_proof().expect("assemble");
        assert!(Diagram::isomorphic(&assembled, &proof), "assembled proof ≠ original");
    }

    /// The target is the supplied goal, not inferred from the proof: with no
    /// target the session has none; with a matching one it is already reached;
    /// a non-parallel target is rejected.
    #[test]
    fn target_is_explicit() {
        // `lhs2 = alpha alpha alpha`; its boundaries are the 1-cell `a`.
        let (open, _) = resume("lhs2", None, false);
        assert!(open.target_diagram().is_none(), "no target supplied ⇒ no target");
        assert!(!open.target_reached());

        let (goal, _) = resume("lhs2", Some("a"), false);
        assert!(goal.target_diagram().is_some());
        assert!(goal.target_reached(), "current = lhs2.out = a = target");

        let (store, tc, path) = load();
        let err = RewriteEngine::resume(
            store, tc, "lhs2", Some("pt"), path, TYPE.to_owned(), false,
        ).err().expect("a 0-cell target is not parallel to the 1-cell initial");
        assert!(err.to_lowercase().contains("parallel") || err.contains("dimension"), "got: {err}");
    }

    /// The save → resume loop: a session's `proof_expr` (what the
    /// `proof` response surfaces and the editor saves) resumes to an isomorphic
    /// proof with the same step count.
    #[test]
    fn proof_expr_round_trips() {
        let (store, tc, path) = load();
        let first = RewriteEngine::resume(
            Arc::clone(&store), Arc::clone(&tc), "inter", None, path.clone(), TYPE.to_owned(), false,
        ).unwrap();
        let expr = first.proof_expr();
        let proof1 = first.assemble_proof().unwrap();

        let again = RewriteEngine::resume(
            store, tc, &expr, None, path, TYPE.to_owned(), false,
        ).expect("resume from the saved proof expression");
        assert_eq!(again.step_count(), first.step_count());
        let proof2 = again.assemble_proof().unwrap();
        assert!(Diagram::isomorphic(&proof1, &proof2), "round-tripped proof differs");
    }

    /// Backward resumption reverses the steps and still reassembles the proof.
    #[test]
    fn backward_reassembles() {
        let (engine, proof) = resume("lhs2", Some("a"), true);
        assert_eq!(engine.step_count(), 3);
        let assembled = engine.assemble_proof().expect("assemble");
        assert!(Diagram::isomorphic(&assembled, &proof));
        assert!(engine.target_reached());
    }

    /// A resumed session behaves like a normal one: undo to the start and redo
    /// back to the end restore the corresponding diagrams.
    #[test]
    fn undo_redo_roundtrip() {
        let (mut engine, _) = resume("lhs2", None, false);
        let steps = engine.step_count();
        assert_eq!(steps, 3);
        let end = engine.current_diagram().clone();

        engine.undo_to(0).expect("undo all");
        assert_eq!(engine.step_count(), 0);
        assert!(Diagram::isomorphic(engine.current_diagram(), engine.initial_diagram()));

        engine.redo_to(steps).expect("redo to end");
        assert_eq!(engine.step_count(), steps);
        assert!(Diagram::isomorphic(engine.current_diagram(), &end));
    }

    /// `dimension 0` and missing diagrams are rejected.
    #[test]
    fn rejects_dimension_zero_and_unknown() {
        let (store, tc, path) = load();
        // `pt` is a 0-cell.
        let err = RewriteEngine::resume(
            Arc::clone(&store), Arc::clone(&tc), "pt", None, path.clone(), TYPE.to_owned(), false,
        ).err().expect("dimension-0 diagram should be rejected");
        assert!(err.contains("dimension"), "got: {err}");

        let err = RewriteEngine::resume(
            store, tc, "nope", None, path, TYPE.to_owned(), false,
        ).err().expect("unknown diagram should be rejected");
        assert!(err.contains("nope"), "got: {err}");
    }
}
